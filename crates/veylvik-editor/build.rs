use std::{path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> Option<String> {
    if !root.join(".git").exists() {
        return None;
    }
    let result = Command::new("git")
        .arg("-C")
        .arg(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .output()
        .ok()?;
    result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_owned())
}
fn watch(path: &Path) {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}
fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&manifest).parent().unwrap().parent().unwrap();
    // Source packages carry the same identity even after .git is left behind.
    let metadata = std::fs::read_to_string(root.join("build-info.txt")).unwrap_or_default();
    let field = |name: &str| {
        metadata.lines().find_map(|line| {
            line.split_once('=')
                .filter(|(key, _)| *key == name)
                .map(|(_, value)| value.to_owned())
        })
    };
    let number = field("build-number")
        .or_else(|| git(root, &["rev-list", "--count", "HEAD"]))
        .filter(|n| n.parse::<u64>().is_ok())
        .unwrap_or_else(|| "unknown".into());
    let commit = field("commit")
        .or_else(|| git(root, &["rev-parse", "--short=12", "HEAD"]))
        .filter(|s| (7..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| "unknown".into());
    let modified = field("modified")
        .filter(|s| s == "true" || s == "false")
        .unwrap_or_else(|| {
            git(root, &["status", "--porcelain", "--untracked-files=normal"])
                .map_or_else(|| "unknown".into(), |s| (!s.is_empty()).to_string())
        });
    for (key, value) in [
        ("NUMBER", number),
        ("COMMIT", commit),
        ("MODIFIED", modified),
        ("PROFILE", std::env::var("PROFILE").unwrap()),
        ("TARGET", std::env::var("TARGET").unwrap()),
    ] {
        println!("cargo:rustc-env=VEYLVIK_BUILD_{key}={value}");
    }
    for path in ["HEAD", "index", "packed-refs"] {
        if let Some(path) = git(root, &["rev-parse", "--git-path", path]) {
            watch(&root.join(path));
        }
    }
    if let Some(branch) = git(root, &["symbolic-ref", "-q", "HEAD"])
        && let Some(path) = git(root, &["rev-parse", "--git-path", &branch])
    {
        watch(&root.join(path));
    }
    for path in [
        "crates",
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "bin",
        "pkgbuild",
        "README.md",
        "build-info.txt",
    ] {
        watch(&root.join(path));
    }
}
