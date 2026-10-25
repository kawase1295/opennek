//! The web UI is built by npm into host/ui/dist and embedded by src/server.rs; without it the build fails with
//! a hint instead of include_str!'s missing file

use std::path::Path;

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    println!("cargo::rerun-if-changed={}", dist.display());
    for file in ["index.html", "app.js", "app.css"] {
        if !dist.join(file).is_file() {
            panic!(
                "the web UI is not built ({} is missing): run `npm ci && npm run build` in host/ui",
                dist.join(file).display()
            );
        }
    }
}
