use std::env;
use std::fs;
use std::path::Path;

fn main() {
    emit_package_version();
}

fn emit_package_version() {
    let version_path = Path::new("VERSION");
    println!("cargo:rerun-if-changed=VERSION");

    let version = fs::read_to_string(version_path)
        .unwrap_or_else(|err| panic!("failed to read VERSION: {err}"))
        .trim()
        .to_string();

    if !is_semver(&version) {
        panic!("VERSION must be semantic MAJOR.MINOR.PATCH, got '{version}'");
    }

    let manifest_version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    if manifest_version != version {
        panic!(
            "VERSION file ({version}) does not match Cargo.toml ({manifest_version}); run 'make sync-version'"
        );
    }

    println!("cargo:rustc-env=MAXIO_VERSION={version}");
}

fn is_semver(version: &str) -> bool {
    let mut parts = version.splitn(2, ['-', '+']);
    let core = parts.next().unwrap_or("");
    let mut nums = core.split('.');
    let major = nums
        .next()
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
    let minor = nums
        .next()
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
    let patch = nums
        .next()
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
    major.is_some() && minor.is_some() && patch.is_some() && nums.next().is_none()
}
