// SPDX-License-Identifier: MIT OR Apache-2.0

//! `fluut tools import <file> -o <library.toml> [--id <id>] [--name <text>] [--force]`
//! (design D16 §7). Converts a Fusion tool-library JSON file to a native
//! tool library. Nothing is run and nothing is sent anywhere.

use fluut_model::{Diagnostic, Severity};
use fluut_post_import::{ImportOptions, fusion, limits, slug, valid_library_id};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Default)]
pub struct ImportArgs {
    pub input: PathBuf,
    pub output: PathBuf,
    pub id: Option<String>,
    pub name: Option<String>,
    pub force: bool,
}

/// Parses the arguments after `tools`.
pub fn parse(mut it: impl Iterator<Item = String>) -> Result<ImportArgs, String> {
    match it.next().as_deref() {
        Some("import") => {}
        Some(s) => return Err(format!("unknown tools command {s}")),
        None => return Err("no tools command (import)".into()),
    }
    let mut a = ImportArgs::default();
    let (mut input, mut output) = (None, None);
    while let Some(arg) = it.next() {
        let mut value = |n: &str| it.next().ok_or(format!("{n} needs a value"));
        match arg.as_str() {
            "-o" | "--output" => output = Some(PathBuf::from(value("-o")?)),
            "--id" => a.id = Some(value("--id")?),
            "--name" => a.name = Some(value("--name")?),
            "--force" => a.force = true,
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s if input.is_none() => input = Some(PathBuf::from(s)),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    a.input = input.ok_or("no input file")?;
    a.output = output.ok_or("no output file (-o)")?;
    Ok(a)
}

fn print(d: &[Diagnostic]) -> bool {
    for x in d {
        eprintln!("{x}");
    }
    d.iter().any(|x| x.severity == Severity::Error)
}

fn fail(code: &str, msg: String) -> ExitCode {
    print(&[Diagnostic::error(code, msg)]);
    ExitCode::from(1)
}

/// Reads at most the size limit plus one byte, so a huge file is refused
/// without being read whole.
fn read_limited(p: &Path) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    std::fs::File::open(p)?
        .take(limits::MAX_FILE_BYTES + 1)
        .read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn run(a: &ImportArgs) -> ExitCode {
    let ext = a
        .input
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match ext.as_deref() {
        Some("json") => {}
        Some("vtdb") => {
            return fail(
                "import.format.unsupported",
                "Vectric .vtdb tool databases cannot be imported yet (design D16 §7)".into(),
            );
        }
        _ => {
            return fail(
                "import.format.unsupported",
                format!(
                    "{}: expected a Fusion tool library (.json)",
                    a.input.display()
                ),
            );
        }
    }
    let stem = a
        .input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let id =
        a.id.clone()
            .unwrap_or_else(|| format!("fusion/{}", slug(&stem, 40, "tools")));
    if !valid_library_id(&id) {
        return fail(
            "import.library.bad_id",
            format!("`{id}` is not a library id (lower-case letters, digits, `-`, `_`, `/`)"),
        );
    }
    let opts = ImportOptions {
        library_name: a.name.clone().unwrap_or_else(|| stem.clone()),
        library_id: id,
    };
    if a.output.exists() && !a.force {
        return fail(
            "import.output.exists",
            format!("{} exists; give --force to replace it", a.output.display()),
        );
    }
    let bytes = match read_limited(&a.input) {
        Ok(b) => b,
        Err(e) => {
            return fail(
                "import.file.unreadable",
                format!("{}: {e}", a.input.display()),
            );
        }
    };
    let r = match fusion::import(&bytes, &opts) {
        Ok(r) => r,
        Err(d) => {
            print(&d);
            return ExitCode::from(1);
        }
    };
    print(&r.diagnostics);
    if let Err(e) = std::fs::write(&a.output, &r.library) {
        return fail(
            "import.output.unwritable",
            format!("{}: {e}", a.output.display()),
        );
    }
    println!(
        "{}: {} of {} tools imported as library {} ({} skipped)",
        a.output.display(),
        r.report.tools_written,
        r.report.tools_read,
        opts.library_id,
        r.report.tools_skipped
    );
    ExitCode::SUCCESS
}
