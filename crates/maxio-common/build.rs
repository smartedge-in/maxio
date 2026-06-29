use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let version_path = Path::new("../../VERSION");
    println!("cargo:rerun-if-changed=../../VERSION");

    let version = fs::read_to_string(version_path)
        .unwrap_or_else(|err| panic!("failed to read VERSION: {err}"))
        .trim()
        .to_string();

    if version.split('.').count() != 3 {
        panic!("VERSION must be semantic MAJOR.MINOR.PATCH, got '{version}'");
    }

    let manifest_version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    if !manifest_version.is_empty() && manifest_version != version {
        panic!(
            "VERSION file ({version}) does not match Cargo.toml ({manifest_version}); run 'make sync-version'"
        );
    }

    println!("cargo:rustc-env=MAXIO_VERSION={version}");
}
