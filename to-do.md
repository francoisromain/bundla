A few things I'd improve:

1. architecture

The empty-[lib] + build.rs pattern is fine for now, but if bundling logic grows (imports, sourcemaps, hashed outputs), consider a real bin target (cargo run) that owns the bundling loop — you'd gain --open, watch-mode reuse, and drop the dummy lib. Not urgent.

2. dev.sh

Minor: dev.sh could take --port/--host passthrough and --open the browser. 


3. build.rs

- CSS-only reload (≈Vite HMR). Today every change is window.location.reload() — full page flash even for a color tweak. Since the WS channel already exists, dev_server could send reload:css for .css writes and the injected script would just re-fetch the stylesheet. Big UX win, small change.
- Clean dist/ before writing in build.rs — otherwise stale files linger (e.g. after your index.js→scripts.js rename, dist/index.js would still be served).
Worth considering
- And lightningcss currently uses StyleSheet::parse — if you ever add multiple CSS files, switch to lightningcss's Bundler for @import resolution.
Want me to draft a plan for any of these (1–3 are my top picks)?
