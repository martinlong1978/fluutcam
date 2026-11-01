// SPDX-License-Identifier: MIT OR Apache-2.0

//! Records the git commit the build was made from in `FLUUT_GIT_COMMIT`
//! (D14 §5.1). `FLUUT_GIT_COMMIT` in the environment wins, so a build from a
//! source archive without `.git` can still name its commit. Without either,
//! the variable stays unset and the version carries no commit.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (!s.is_empty()).then_some(s)
}

fn main() {
    println!("cargo:rerun-if-env-changed=FLUUT_GIT_COMMIT");
    let commit = std::env::var("FLUUT_GIT_COMMIT")
        .ok()
        .filter(|c| !c.trim().is_empty())
        .or_else(|| git(&["rev-parse", "--short=12", "HEAD"]));
    if let Some(c) = commit {
        println!("cargo:rustc-env=FLUUT_GIT_COMMIT={}", c.trim());
    }
    // Rebuild when the checked-out commit changes. The reflog changes on
    // every commit, also in a linked worktree.
    for p in ["HEAD", "logs/HEAD"] {
        if let Some(path) = git(&["rev-parse", "--git-path", p]) {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}
