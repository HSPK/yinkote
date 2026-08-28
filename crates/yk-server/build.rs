//! Make sure the directory the workbench is embedded from exists.
//!
//! `rust-embed` reads its folder at compile time, so a clone that has never run
//! `npm run build` would fail to compile the server at all — a confusing way to
//! learn that a *frontend* step was skipped.
//!
//! So the directory is created if absent, holding one page that says what
//! happened. A build always succeeds; a server built without the workbench
//! tells whoever opens it why there is nothing there, in the one place they are
//! definitely looking.

use std::path::Path;

const PLACEHOLDER: &str = r#"<!doctype html>
<html lang="en">
  <head><meta charset="utf-8"><title>Yinkote</title></head>
  <body style="font:14px system-ui;margin:3rem;max-width:34rem">
    <h1>The workbench was not built into this binary.</h1>
    <p>The API is running and fully usable; only the web interface is missing.</p>
    <p>Build it with <code>cd web &amp;&amp; npm install &amp;&amp; npm run build</code>,
       then rebuild the server. To serve it from disk instead, start with
       <code>--web-dir web/dist</code>.</p>
  </body>
</html>
"#;

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");

    // Rebuild when the workbench does. Without this, `cargo build` after
    // `npm run build` would hand back the previously embedded assets — the
    // stale-binary trap, one layer down.
    println!("cargo:rerun-if-changed={}", dist.display());

    if !dist.join("index.html").exists() {
        let _ = std::fs::create_dir_all(&dist);
        let _ = std::fs::write(dist.join("index.html"), PLACEHOLDER);
    }
}
