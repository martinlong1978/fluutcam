// SPDX-License-Identifier: MIT OR Apache-2.0

//! The application version (D14 §5.1). One version for the whole workspace,
//! from `[workspace.package]`, plus the git commit when the build knows it
//! (see `build.rs`).

/// Workspace version, semantic versioning.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git commit of the build, if known.
pub const GIT_COMMIT: Option<&str> = option_env!("FLUUT_GIT_COMMIT");

/// `1.2.3 (abcdef123456)`, or `1.2.3` when the commit is unknown.
pub fn version_string() -> String {
    format_version(VERSION, GIT_COMMIT)
}

/// The formatting behind [`version_string`].
pub fn format_version(version: &str, commit: Option<&str>) -> String {
    match commit {
        Some(c) if !c.is_empty() => format!("{version} ({c})"),
        _ => version.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_follows_the_version_in_brackets() {
        assert_eq!(format_version("1.2.3", Some("abc123")), "1.2.3 (abc123)");
    }

    #[test]
    fn unknown_commit_leaves_the_bare_version() {
        assert_eq!(format_version("1.2.3", None), "1.2.3");
        assert_eq!(format_version("1.2.3", Some("")), "1.2.3");
    }

    #[test]
    fn version_is_the_workspace_version() {
        assert_eq!(VERSION, "0.1.0");
        assert!(version_string().starts_with(VERSION));
    }

    #[test]
    fn commit_is_hex_when_present() {
        if let Some(c) = GIT_COMMIT {
            assert!(c.chars().all(|ch| ch.is_ascii_hexdigit()), "{c}");
        }
    }
}
