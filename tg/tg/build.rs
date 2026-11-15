//! Links `tg` at user addresses with Zoraya's own link script for native programs
//! (`user/user.ld` of the checkout the crates come from).

use std::path::Path;

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let script = Path::new(&dir).join("../../../zoraya-tgw/user/user.ld");
    assert!(script.is_file(), "no {}", script.display());
    println!("cargo:rustc-link-arg-bins=-T{}", script.display());
    println!("cargo:rerun-if-changed={}", script.display());
}
