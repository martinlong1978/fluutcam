// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reproducible release builds (D14 §5.4).
//!
//! - `cargo xtask rustflags` prints the `RUSTFLAGS` of a release build:
//!   path remapping and, on Windows, control-flow guard (S 06 §6.3).
//! - `cargo xtask repro-check` builds the CLI twice from scratch with those
//!   flags and `SOURCE_DATE_EPOCH`, and fails if the binaries differ.

use crate::util;
use std::path::Path;
use std::process::Command;

/// Where the checkout appears in a remapped build.
const BUILD_DIR: &str = "/build";
const CARGO_DIR: &str = "/cargo";
/// Used when `SOURCE_DATE_EPOCH` is unset.
const FALLBACK_EPOCH: u64 = 0;

/// The release `rustc` flags, one element per argument.
pub fn rustflags(os: &str, root: &Path, cargo_home: Option<&Path>) -> Vec<String> {
    let mut flags = Vec::new();
    let mut remap = |from: &Path, to: &str| {
        flags.push(format!("--remap-path-prefix={}={to}", from.display()));
    };
    remap(root, BUILD_DIR);
    if let Some(c) = cargo_home {
        remap(c, CARGO_DIR);
    }
    if os == "windows" {
        flags.extend(["-C".into(), "control-flow-guard".into()]);
    }
    flags
}

fn cargo_home() -> Option<std::path::PathBuf> {
    std::env::var_os("CARGO_HOME").map(Into::into).or_else(|| {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|h| Path::new(&h).join(".cargo"))
    })
}

fn root() -> Result<std::path::PathBuf, String> {
    util::workspace_root()
        .canonicalize()
        .map(|p| {
            // `\\?\C:\…` is not what cargo passes to rustc.
            let s = p.to_string_lossy().into_owned();
            s.strip_prefix(r"\\?\").map_or(p, |s| s.into())
        })
        .map_err(|e| e.to_string())
}

/// `cargo xtask rustflags`: the flags separated by spaces. Fails for paths
/// with spaces, which `RUSTFLAGS` cannot hold.
pub fn print_rustflags() -> Result<(), String> {
    let flags = rustflags(std::env::consts::OS, &root()?, cargo_home().as_deref());
    if let Some(bad) = flags.iter().find(|f| f.contains(' ')) {
        return Err(format!("RUSTFLAGS cannot hold a path with spaces: {bad}"));
    }
    println!("{}", flags.join(" "));
    Ok(())
}

/// Describes how two builds compare.
pub fn compare(a: &[u8], b: &[u8]) -> Result<String, String> {
    let (ha, hb) = (util::sha256_hex(a), util::sha256_hex(b));
    if ha == hb {
        Ok(ha)
    } else {
        Err(format!(
            "builds differ ({} and {} bytes)\n  first : {ha}\n  second: {hb}",
            a.len(),
            b.len()
        ))
    }
}

fn build_once(root: &Path, target: &Path, epoch: u64, flags: &[String]) -> Result<Vec<u8>, String> {
    let _ = std::fs::remove_dir_all(target);
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "--locked", "-p", "fluut-cli"])
        .current_dir(root)
        .env("CARGO_TARGET_DIR", target)
        .env("SOURCE_DATE_EPOCH", epoch.to_string())
        .env("CARGO_ENCODED_RUSTFLAGS", flags.join("\x1f"))
        .env_remove("RUSTFLAGS")
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !status.success() {
        return Err("cargo build failed".into());
    }
    let exe = target
        .join("release")
        .join(format!("fluut{}", std::env::consts::EXE_SUFFIX));
    std::fs::read(&exe).map_err(|e| format!("{}: {e}", exe.display()))
}

/// `cargo xtask repro-check`.
pub fn repro_check() -> Result<(), String> {
    let root = root()?;
    let flags = rustflags(std::env::consts::OS, &root, cargo_home().as_deref());
    let epoch = util::source_date_epoch().unwrap_or(FALLBACK_EPOCH);
    // The same directory both times: the build path may end up in the
    // binary, remapped or not.
    let target = root.join("target").join("repro");
    let first = build_once(&root, &target, epoch, &flags)?;
    let second = build_once(&root, &target, epoch, &flags)?;
    let hash = compare(&first, &second)?;
    println!("repro-check: two builds of fluut are identical (sha256 {hash})");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_remap_the_checkout_and_cargo_home() {
        let f = rustflags(
            "linux",
            Path::new("/src/fluut"),
            Some(Path::new("/home/u/.cargo")),
        );
        assert_eq!(
            f,
            [
                "--remap-path-prefix=/src/fluut=/build",
                "--remap-path-prefix=/home/u/.cargo=/cargo"
            ]
        );
    }

    #[test]
    fn windows_adds_control_flow_guard() {
        let f = rustflags("windows", Path::new("/r"), None);
        assert_eq!(&f[f.len() - 2..], ["-C", "control-flow-guard"]);
        assert!(
            !rustflags("macos", Path::new("/r"), None)
                .iter()
                .any(|f| f.contains("guard"))
        );
    }

    #[test]
    fn equal_builds_return_the_hash_and_different_ones_fail() {
        assert_eq!(
            compare(b"abc", b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let err = compare(b"abc", b"abd").unwrap_err();
        assert!(err.contains("differ"), "{err}");
    }
}
