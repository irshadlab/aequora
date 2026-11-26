#[allow(dead_code)]
#[path = "src/codegen.rs"]
mod codegen;
#[allow(dead_code)]
#[path = "src/types.rs"]
mod types;

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=../../registry");
    println!("cargo:rerun-if-changed=src/registry.rs");
    println!("cargo:rerun-if-env-changed=AEQUORA_REGENERATE_REGISTRY");
    let crate_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = crate_dir.join("../..");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap_or_default()).join("registry.rs");
    let snapshot = crate_dir.join("src/registry.rs");

    // A crates.io package is verified in isolation, without the workspace-level canonical RON
    // files. Keep a checked-in generated snapshot in the package, while still validating it
    // against the canonical registry on every workspace build.
    if root.join("registry/manifest.ron").is_file() {
        let set = codegen::load_registry(&root)
            .unwrap_or_else(|error| panic!("canonical registry validation failed: {error}"));
        let lock = codegen::load_lock(&root)
            .unwrap_or_else(|error| panic!("registry lock load failed: {error}"));
        codegen::verify_lock(&set, &lock)
            .unwrap_or_else(|error| panic!("registry lock verification failed: {error}"));
        let generated = codegen::generated_rust(&set);
        if env::var_os("AEQUORA_REGENERATE_REGISTRY").as_deref() == Some(std::ffi::OsStr::new("1"))
        {
            fs::write(&snapshot, &generated).unwrap_or_else(|error| {
                panic!("generated registry snapshot write failed: {error}")
            });
        } else {
            let committed = fs::read_to_string(&snapshot)
                .unwrap_or_else(|error| panic!("generated registry snapshot read failed: {error}"));
            assert_eq!(
                committed, generated,
                "generated registry snapshot is stale; run `AEQUORA_REGENERATE_REGISTRY=1 cargo build -p aequora-registry`"
            );
        }
        fs::write(out, generated)
            .unwrap_or_else(|error| panic!("generated registry write failed: {error}"));
    } else {
        fs::copy(&snapshot, &out)
            .unwrap_or_else(|error| panic!("packaged registry snapshot copy failed: {error}"));
    }
}
