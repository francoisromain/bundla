use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::Path,
};

use lol_html::{RewriteStrSettings, element, html_content::Element, rewrite_str};

// collect `<link rel="stylesheet">` hrefs and `<script type="module">` srcs, in document order
pub fn html_asset_refs_extract(html: &str, page: &Path) -> Result<Vec<String>, String> {
    let asset_refs: RefCell<Vec<String>> = RefCell::new(Vec::new());

    let settings = RewriteStrSettings::new()
        .append_element_content_handler(element!("script", |el| {
            let Some(src) = el.get_attribute("src") else {
                return Ok(());
            };

            if src.trim().is_empty() {
                return Ok(());
            }

            if is_module_script(el) {
                asset_refs.borrow_mut().push(src.to_string());
            } else {
                skip_script_warn(src.trim(), page);
            }

            Ok(())
        }))
        .append_element_content_handler(element!("link", |el| {
            if is_stylesheet_link(el)
                && let Some(href) = el.get_attribute("href")
                && !href.trim().is_empty()
            {
                asset_refs.borrow_mut().push(href);
            }

            Ok(())
        }));

    rewrite_str(html, settings).map_err(|err| format!("html parse error: {err}"))?;

    Ok(asset_refs.into_inner())
}

fn is_module_script(el: &Element<'_, '_>) -> bool {
    el.get_attribute("type")
        .is_some_and(|t| t.trim().eq_ignore_ascii_case("module"))
}

fn is_stylesheet_link(el: &Element<'_, '_>) -> bool {
    el.get_attribute("rel").is_some_and(|rel| {
        rel.split_whitespace()
            .any(|token| token.eq_ignore_ascii_case("stylesheet"))
    })
}

fn skip_script_warn(src: &str, page: &Path) {
    eprintln!(
        "warning: skipping classic <script> (type=\"module\" required): {src} in {}",
        page.display()
    );
}

// rewrite the discovered references to their bundled output
// deduping duplicate references within the page to one
pub fn html_rewrite(html: &str, map: &HashMap<String, String>) -> Result<String, String> {
    if map.is_empty() {
        return Ok(html.to_string());
    }

    struct State<'a> {
        map: &'a HashMap<String, String>,
        seen: HashSet<String>,
    }

    fn handler(
        el: &mut Element,
        attr: &str,
        state: &RefCell<State>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(value) = el.get_attribute(attr) {
            let new = state.borrow().map.get(&value).cloned();
            if let Some(new) = new {
                let mut state = state.borrow_mut();
                if state.seen.insert(new.clone()) {
                    el.set_attribute(attr, &new)?;
                } else {
                    el.remove();
                }
            }
        }

        Ok(())
    }

    let state = RefCell::new(State {
        map,
        seen: HashSet::new(),
    });
    let settings = RewriteStrSettings::new()
        .append_element_content_handler(element!("script", |el| handler(el, "src", &state)))
        .append_element_content_handler(element!("link", |el| handler(el, "href", &state)));

    rewrite_str(html, settings).map_err(|err| format!("html rewrite error: {err}"))
}

// minify an html document with minify-html, keeping closing tags and
// structural (`<html>`, `<head>`) opening tags so the output stays readable
pub fn html_minify(html: &str) -> Result<String, String> {
    let mut cfg = minify_html::Cfg::new();
    cfg.keep_closing_tags = true;
    cfg.keep_html_and_head_opening_tags = true;
    cfg.minify_css = true;
    cfg.minify_js = true;
    let minified = minify_html::minify(html.as_bytes(), &cfg);
    String::from_utf8(minified).map_err(|err| format!("html minify error: {err}"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn page() -> PathBuf {
        PathBuf::from("index.html")
    }

    #[test]
    fn extract_collects_module_script_then_stylesheet() {
        let html = r#"
            <html><head>
                <script type="module" src="./scripts/main.js"></script>
                <link rel="stylesheet" href="./styles/main.css" />
            </head></html>
        "#;

        let refs = html_asset_refs_extract(html, &page()).unwrap();
        assert_eq!(refs, ["./scripts/main.js", "./styles/main.css"]);
    }

    #[test]
    fn extract_skips_classic_scripts() {
        let html = r#"<script src="./legacy.js"></script>"#;

        let refs = html_asset_refs_extract(html, &page()).unwrap();
        assert!(refs.is_empty());
    }

    #[test]
    fn extract_skips_empty_refs() {
        let html = r#"
            <script type="module" src=""></script>
            <script type="module" src="   "></script>
            <link rel="stylesheet" href="" />
        "#;

        let refs = html_asset_refs_extract(html, &page()).unwrap();
        assert!(refs.is_empty());
    }

    #[test]
    fn extract_ignores_non_stylesheet_links() {
        let html = r#"
            <link rel="icon" href="/favicon.ico" />
            <link rel="preload" href="/app.css" />
            <a href="./styles/main.css">css</a>
        "#;

        let refs = html_asset_refs_extract(html, &page()).unwrap();
        assert!(refs.is_empty());
    }

    #[test]
    fn extract_keeps_remote_refs() {
        let html = r#"
            <link rel="stylesheet" href="https://cdn.example.com/x.css" />
            <script type="module" src="//cdn.example.com/app.js"></script>
        "#;

        let refs = html_asset_refs_extract(html, &page()).unwrap();
        assert_eq!(
            refs,
            ["https://cdn.example.com/x.css", "//cdn.example.com/app.js"]
        );
    }

    #[test]
    fn rewrite_replaces_mapped_refs() {
        let html = r#"<script type="module" src="./scripts/main.js"></script>
<link rel="stylesheet" href="./styles/main.css" />"#;
        let map = HashMap::from([
            (
                "./scripts/main.js".to_string(),
                "./scripts/main-abc.js".to_string(),
            ),
            (
                "./styles/main.css".to_string(),
                "./styles/main-abc.css".to_string(),
            ),
        ]);

        let out = html_rewrite(html, &map).unwrap();
        assert!(out.contains("./scripts/main-abc.js"));
        assert!(out.contains("./styles/main-abc.css"));
        assert!(!out.contains("./scripts/main.js"));
        assert!(!out.contains("./styles/main.css"));
    }

    #[test]
    fn rewrite_dedupes_duplicate_refs() {
        let html = r#"<script type="module" src="./a.js"></script>
<script type="module" src="./a.js"></script>"#;
        let map = HashMap::from([("./a.js".to_string(), "./a-hash.js".to_string())]);

        let out = html_rewrite(html, &map).unwrap();
        assert_eq!(out.matches("./a-hash.js").count(), 1);
    }

    #[test]
    fn rewrite_empty_map_is_unchanged() {
        let html = "<html><body>hi</body></html>";

        let out = html_rewrite(html, &HashMap::new()).unwrap();
        assert_eq!(out, html);
    }

    #[test]
    fn rewrite_leaves_unmapped_content() {
        let html = r#"<script type="module" src="./a.js"></script><p>keep</p>"#;
        let map = HashMap::from([("./b.js".to_string(), "./b-hash.js".to_string())]);

        let out = html_rewrite(html, &map).unwrap();
        assert!(out.contains("<p>keep</p>"));
        assert!(out.contains("./a.js"));
    }

    #[test]
    fn minify_compresses_and_keeps_structure() {
        let html = "<html>\n  <head>\n    <title>t</title>\n  </head>\n  <body>\n    <p>hello world</p>\n  </body>\n</html>";

        let out = html_minify(html).unwrap();
        assert!(out.contains("<html>"));
        assert!(out.contains("<head>"));
        assert!(out.contains("hello world"));
        assert!(out.len() < html.len());
    }
}
