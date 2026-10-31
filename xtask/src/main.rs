// SPDX-License-Identifier: MIT OR Apache-2.0

//! Development tasks. Run with `cargo xtask <task>`.
//!
//! - `check-layers`: enforce the crate layering of design D01 §1.

use std::collections::BTreeMap;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some("check-layers") => match check_layers() {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => {
                eprintln!("{msg}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: cargo xtask check-layers");
            ExitCode::FAILURE
        }
    }
}

fn check_layers() -> Result<(), String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let layers_text = std::fs::read_to_string(root.join("layers.toml"))
        .map_err(|e| format!("cannot read layers.toml: {e}"))?;
    let layers = parse_layers(&layers_text)?;

    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root.join(".."))
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let meta: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("bad cargo metadata: {e}"))?;

    let mut deps = Vec::new();
    for pkg in meta["packages"].as_array().into_iter().flatten() {
        let name = pkg["name"].as_str().unwrap_or_default().to_owned();
        if name == "xtask" {
            continue;
        }
        let pkg_deps = pkg["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            // Only path dependencies are workspace crates.
            .filter(|d| d["path"].is_string())
            .filter_map(|d| d["name"].as_str().map(str::to_owned))
            .collect();
        deps.push((name, pkg_deps));
    }

    let v = violations(&layers, &deps);
    if v.is_empty() {
        println!("check-layers: {} crates OK", deps.len());
        Ok(())
    } else {
        Err(format!("check-layers failed:\n  {}", v.join("\n  ")))
    }
}

/// Parses `name = number` lines; `#` starts a comment.
fn parse_layers(text: &str) -> Result<BTreeMap<String, u32>, String> {
    let mut layers = BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (name, num) = line
            .split_once('=')
            .ok_or_else(|| format!("layers.toml:{}: expected `name = layer`", i + 1))?;
        let num = num
            .trim()
            .parse()
            .map_err(|_| format!("layers.toml:{}: bad layer number", i + 1))?;
        layers.insert(name.trim().to_owned(), num);
    }
    Ok(layers)
}

/// Returns one message per dependency that breaks the layering rule.
fn violations(layers: &BTreeMap<String, u32>, deps: &[(String, Vec<String>)]) -> Vec<String> {
    let mut out = Vec::new();
    for (krate, krate_deps) in deps {
        let Some(&own) = layers.get(krate) else {
            out.push(format!("{krate}: not listed in xtask/layers.toml"));
            continue;
        };
        for dep in krate_deps {
            match layers.get(dep) {
                None => out.push(format!(
                    "{krate} -> {dep}: {dep} not listed in xtask/layers.toml"
                )),
                Some(&l) if l >= own => out.push(format!(
                    "{krate} (layer {own}) -> {dep} (layer {l}): may only depend on lower layers"
                )),
                Some(_) => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layers() -> BTreeMap<String, u32> {
        parse_layers("fluut-geom = 0\nfluut-path = 1 # comment\n").unwrap()
    }

    #[test]
    fn upward_dependency_is_a_violation() {
        let deps = [("fluut-geom".to_owned(), vec!["fluut-path".to_owned()])];
        assert_eq!(violations(&layers(), &deps).len(), 1);
    }

    #[test]
    fn downward_dependency_is_allowed() {
        let deps = [("fluut-path".to_owned(), vec!["fluut-geom".to_owned()])];
        assert!(violations(&layers(), &deps).is_empty());
    }

    #[test]
    fn unlisted_crate_is_a_violation() {
        let deps = [("fluut-new".to_owned(), vec![])];
        assert_eq!(violations(&layers(), &deps).len(), 1);
    }
}
