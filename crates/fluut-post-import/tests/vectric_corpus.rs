// SPDX-License-Identifier: MIT OR Apache-2.0

//! Converts every `.pp` file under `FLUUT_VECTRIC_PP_DIR` (searched
//! recursively) and posts the reference toolpath suite through each
//! converted post (D10 §9). Skipped when the variable is not set.
//!
//! Vectric's own post processors are not ours to ship, so the corpus stays
//! on the maintainer's computer. The test prints only counts and diagnostic
//! codes, never file contents (with `FLUUT_VECTRIC_PP_VERBOSE` set it also
//! prints each file's name and diagnostics, and with `FLUUT_VECTRIC_PP_OUT`
//! it writes each converted post and its reference output to that folder,
//! for local diagnosis only). It
//! fails when a conversion panics, when a
//! converted post does not load (`import.pp.internal`), or when posting the
//! reference suite fails.
//!
//! ```text
//! FLUUT_VECTRIC_PP_DIR="C:\ProgramData\Vectric\Aspire\V10.5\PostP" \
//!     cargo test -p fluut-post-import --test vectric_corpus -- --nocapture
//! ```

use fluut_model::{DefRef, MemStore, Severity};
use fluut_post::reference::{SuiteOptions, reference_job};
use fluut_post::{CompiledPost, Post, Provenance, post, resolve_post};
use fluut_post_import::vectric::{self, Options};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn pp_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            pp_files(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pp")) {
            out.push(p);
        }
    }
}

/// Posts the reference suite with a converted post; the error text if it
/// fails.
fn post_reference(text: &str, id: &str) -> Result<String, String> {
    let mut s = MemStore::new();
    s.insert_toml(text).map_err(|e| e.to_string())?;
    let r = resolve_post(&s, &DefRef::parse(id).unwrap(), None).map_err(|e| e.to_string())?;
    let cp = CompiledPost::compile(Post::load(&r).map_err(|e| e.to_string())?)
        .map_err(|e| format!("{e:?}"))?;
    let job = reference_job(SuiteOptions::for_post(&cp), Provenance::of(&r, id));
    let out = post(&job, &cp).map_err(|e| e.to_string())?;
    Ok(out.files.concat())
}

#[test]
fn corpus() {
    let Some(dir) = std::env::var_os("FLUUT_VECTRIC_PP_DIR") else {
        eprintln!("FLUUT_VECTRIC_PP_DIR not set; corpus test skipped");
        return;
    };
    let mut files = Vec::new();
    pp_files(Path::new(&dir), &mut files);
    assert!(!files.is_empty(), "no .pp files under the folder");
    let (mut clean, mut warned, mut refused) = (0, 0, 0);
    let mut warn_codes: BTreeMap<String, usize> = BTreeMap::new();
    let mut err_codes: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut posted = 0;
    let verbose = std::env::var_os("FLUUT_VECTRIC_PP_VERBOSE").is_some();
    // Local inspection: converted posts and their reference output.
    let out_dir = std::env::var_os("FLUUT_VECTRIC_PP_OUT").map(PathBuf::from);
    for (i, f) in files.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        let folder = f.parent().unwrap().to_owned();
        let mut base = |name: &str| std::fs::read(folder.join(name)).ok();
        let id = format!("corpus/post-{i}");
        let opts = Options {
            post_id: id.clone(),
            name: None,
        };
        let r = vectric::import_with_base(&bytes, &opts, &mut base);
        if verbose {
            let d = match &r {
                Ok(c) => &c.diagnostics,
                Err(d) => d,
            };
            eprintln!("{}", f.display());
            for x in d {
                eprintln!("    {x}");
            }
        }
        match r {
            Ok(c) => {
                let codes: std::collections::BTreeSet<&str> =
                    c.diagnostics.iter().map(|d| d.code.as_str()).collect();
                for code in &codes {
                    *warn_codes.entry((*code).to_owned()).or_default() += 1;
                }
                if c.diagnostics.is_empty() {
                    clean += 1;
                } else {
                    warned += 1;
                }
                if codes.contains("import.pp.internal") {
                    failures.push(format!(
                        "file #{i}: converted post reports internal warnings"
                    ));
                }
                match post_reference(&c.post, &id) {
                    Ok(nc) => {
                        posted += 1;
                        if let Some(d) = &out_dir {
                            let stem = f.file_stem().unwrap().to_string_lossy();
                            std::fs::create_dir_all(d).unwrap();
                            std::fs::write(d.join(format!("{stem}.post.toml")), &c.post).unwrap();
                            std::fs::write(d.join(format!("{stem}.nc")), nc).unwrap();
                        }
                    }
                    Err(e) => failures.push(format!("file #{i}: posting failed: {e}")),
                }
            }
            Err(d) => {
                refused += 1;
                for x in d.iter().filter(|x| x.severity == Severity::Error) {
                    *err_codes.entry(x.code.clone()).or_default() += 1;
                    if x.code == "import.pp.internal" {
                        failures.push(format!("file #{i}: {}", x.message));
                    }
                }
            }
        }
    }
    eprintln!("Vectric corpus: {} files", files.len());
    eprintln!(
        "  converted clean: {clean}\n  converted with warnings: {warned}\n  refused: {refused}"
    );
    eprintln!("  reference suite posted: {posted}");
    eprintln!("  warning codes (files): {warn_codes:?}");
    eprintln!("  refusal codes (files): {err_codes:?}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
