use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

fn main() {
    emit_package_version();
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-env-changed=SKIP_FRONTEND");
    println!("cargo:rerun-if-changed=ui/package.json");
    println!("cargo:rerun-if-changed=ui/bun.lock");
    println!("cargo:rerun-if-changed=ui/svelte.config.js");
    println!("cargo:rerun-if-changed=ui/vite.config.ts");
    println!("cargo:rerun-if-changed=ui/src");
    println!("cargo:rerun-if-changed=ui/static");
    println!("cargo:rerun-if-changed=ui/build");

    let ui_dir = Path::new("ui");
    let build_dir = ui_dir.join("build");

    if std::env::var("SKIP_FRONTEND").ok().as_deref() == Some("1") {
        ensure_minimal_frontend(&build_dir);
        return;
    }

    let existing_artifact = build_dir.join("200.html");
    if existing_artifact.exists() && !frontend_inputs_newer_than(&existing_artifact) {
        return;
    }

    if !run_bun_install_if_needed(ui_dir) || !run(ui_dir, "bun", &["run", "build"]) {
        println!(
            "cargo:warning=bun unavailable or frontend build failed; embedding minimal UI (set SKIP_FRONTEND=1 to silence)"
        );
        ensure_minimal_frontend(&build_dir);
    }
}

fn run_bun_install_if_needed(ui_dir: &Path) -> bool {
    let node_modules = ui_dir.join("node_modules");
    let package_json = ui_dir.join("package.json");
    let bun_lock = ui_dir.join("bun.lock");

    let needs_install = !node_modules.exists()
        || mtime(&package_json) > mtime(&node_modules)
        || mtime(&bun_lock) > mtime(&node_modules);

    if needs_install {
        return run(ui_dir, "bun", &["install", "--frozen-lockfile"]);
    }

    true
}

fn run(cwd: &Path, program: &str, args: &[&str]) -> bool {
    match Command::new(program).args(args).current_dir(cwd).status() {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!(
                "cargo:warning={program} {} failed with {status}",
                args.join(" ")
            );
            false
        }
        Err(err) => {
            eprintln!("cargo:warning=failed to run {program}: {err}");
            false
        }
    }
}

fn frontend_inputs_newer_than(artifact: &Path) -> bool {
    let artifact_mtime = mtime(artifact);
    let roots = [
        PathBuf::from("ui/package.json"),
        PathBuf::from("ui/bun.lock"),
        PathBuf::from("ui/svelte.config.js"),
        PathBuf::from("ui/vite.config.ts"),
        PathBuf::from("ui/src"),
        PathBuf::from("ui/static"),
    ];

    roots.iter().any(|path| newest_mtime(path) > artifact_mtime)
}

fn newest_mtime(path: &Path) -> SystemTime {
    if path.is_dir() {
        fs::read_dir(path)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .map(|entry| newest_mtime(&entry.path()))
            .max()
            .unwrap_or(SystemTime::UNIX_EPOCH)
    } else {
        mtime(path)
    }
}

fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
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
    let mut parts = version.splitn(2, |c| c == '-' || c == '+');
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

fn ensure_minimal_frontend(build_dir: &Path) {
    fs::create_dir_all(build_dir).expect("create ui/build for SKIP_FRONTEND");
    let fallback = build_dir.join("200.html");
    if !fallback.exists() {
        fs::write(
            &fallback,
            "<!doctype html><title>MaxIO</title><body>UI build skipped</body>",
        )
        .expect("write minimal ui/build/200.html");
    }
}
