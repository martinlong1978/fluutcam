// SPDX-License-Identifier: MIT OR Apache-2.0

//! `fluut posts import <file.pp> -o <post.toml> [--id <id>] [--force]`
//! (design D10 §6.1). Converts a Vectric post processor to a native post.
//! Nothing in the file is run and nothing is sent anywhere. A `POST_BASE`
//! file is looked up beside the input.

use fluut_model::{Diagnostic, Severity};
use fluut_post_import::vectric::{self, Options};
use fluut_post_import::{limits, valid_library_id};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Default)]
pub struct ImportArgs {
    pub input: PathBuf,
    pub output: PathBuf,
    pub id: Option<String>,
    pub force: bool,
}

/// Parses the arguments after `posts`.
pub fn parse(mut it: impl Iterator<Item = String>) -> Result<ImportArgs, String> {
    match it.next().as_deref() {
        Some("import") => {}
        Some(s) => return Err(format!("unknown posts command {s}")),
        None => return Err("no posts command (import)".into()),
    }
    let mut a = ImportArgs::default();
    let (mut input, mut output) = (None, None);
    while let Some(arg) = it.next() {
        let mut value = |n: &str| it.next().ok_or(format!("{n} needs a value"));
        match arg.as_str() {
            "-o" | "--output" => output = Some(PathBuf::from(value("-o")?)),
            "--id" => a.id = Some(value("--id")?),
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

fn print(d: &[Diagnostic]) {
    for x in d {
        eprintln!("{x}");
    }
}

fn fail(code: &str, msg: String) -> ExitCode {
    print(&[Diagnostic::error(code, msg)]);
    ExitCode::from(1)
}

/// Reads at most the post size limit plus one byte, so a huge file is
/// refused without being read whole.
fn read_limited(p: &Path) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    std::fs::File::open(p)?
        .take(limits::MAX_POST_BYTES + 1)
        .read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn run(a: &ImportArgs) -> ExitCode {
    let ext = a
        .input
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match ext.as_deref() {
        Some("pp") => {}
        Some("cps") => {
            return fail(
                "import.format.unsupported",
                "Autodesk .cps posts are programs; FluutCAM does not import them yet (spec 05 §5.10.3)"
                    .into(),
            );
        }
        _ => {
            return fail(
                "import.format.unsupported",
                format!(
                    "{}: expected a Vectric post processor (.pp)",
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
    let mut opts = Options::for_file_stem(&stem);
    if let Some(id) = &a.id {
        opts.post_id = id.clone();
    }
    if !valid_library_id(&opts.post_id) {
        return fail(
            "import.post.bad_id",
            format!(
                "`{}` is not a post id (lower-case letters, digits, `-`, `_`, `/`)",
                opts.post_id
            ),
        );
    }
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
    let folder = a.input.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut base = |name: &str| read_limited(&folder.join(name)).ok();
    let r = match vectric::import_with_base(&bytes, &opts, &mut base) {
        Ok(r) => r,
        Err(d) => {
            print(&d);
            return ExitCode::from(1);
        }
    };
    print(&r.diagnostics);
    if let Err(e) = std::fs::write(&a.output, &r.post) {
        return fail(
            "import.output.unwritable",
            format!("{}: {e}", a.output.display()),
        );
    }
    let warnings = r
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();
    println!(
        "{}: converted `{}` as post {} ({} of {} blocks mapped, {} words, {} warnings)",
        a.output.display(),
        r.name,
        opts.post_id,
        r.report.blocks_mapped,
        r.report.blocks_read,
        r.report.words_written,
        warnings
    );
    ExitCode::SUCCESS
}
