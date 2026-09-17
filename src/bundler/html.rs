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
