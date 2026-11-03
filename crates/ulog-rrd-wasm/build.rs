//! Embed `blueprint.rbl`, which `scripts/blueprint.py` writes beside this file, or nothing when it
//! is missing, so the crate still type-checks and `blueprint()` reports the missing file instead.
use std::path::Path;

fn main() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("blueprint.rbl");
    let dst = Path::new(&std::env::var("OUT_DIR").unwrap()).join("blueprint.rbl");
    let bytes = std::fs::read(&src).unwrap_or_default();
    std::fs::write(dst, bytes).unwrap();
    println!("cargo:rerun-if-changed={}", src.display());
}
