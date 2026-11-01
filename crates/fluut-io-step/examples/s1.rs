// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spike S1 corpus measurements:
//! `FLUUT_STEP_CORPUS=<dir> cargo run -p fluut-io-step --example s1 --release`.

use std::path::{Path, PathBuf};
use std::time::Instant;

fn step_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            step_files(&p, out);
        } else if p
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("stp") || e.eq_ignore_ascii_case("step"))
        {
            out.push(p);
        }
    }
}

fn main() {
    let Ok(dir) = std::env::var("FLUUT_STEP_CORPUS") else {
        eprintln!("set FLUUT_STEP_CORPUS to the corpus directory");
        return;
    };
    let mut files = Vec::new();
    step_files(Path::new(&dir), &mut files);
    let (mut bytes, mut secs, mut ok, mut ents) = (0usize, 0.0, 0usize, 0usize);
    let mut supported = std::collections::BTreeMap::<String, usize>::new();
    let mut unsupported = std::collections::BTreeMap::<String, (usize, usize)>::new();
    let mut schemas = std::collections::BTreeMap::<String, usize>::new();
    let mut issues = std::collections::BTreeMap::<String, usize>::new();
    let mut skips = std::collections::BTreeMap::<String, usize>::new();
    let (mut n_bodies, mut n_failed, mut n_faces, mut n_skipped) = (0, 0, 0, 0);
    let mut shells_ok_declared = 0;
    let (mut shells, mut shells_ok, mut shells_ok_loose, mut closed, mut closed_ok) =
        (0, 0, 0, 0, 0);
    let (mut files_ok, mut map_secs) = (0, 0.0);
    // P-curves: totals, failures by surface kind (all, off-surface), time.
    let mut pc = fluut_topo::pcurve::Stats::default();
    let (mut pc_rejected, mut pc_declared) = (0, 0);
    let mut pc_fail = std::collections::BTreeMap::<String, (usize, usize)>::new();
    let mut pc_kinds = std::collections::BTreeMap::<String, usize>::new();
    let (mut pc_secs, mut pc_worst) = (0.0, Vec::<(f64, String)>::new());
    // Outer loops of faces bounded only by FACE_BOUND.
    let mut outer = fluut_io_step::OuterStats::default();
    let mut outer_kinds = std::collections::BTreeMap::<String, [usize; 2]>::new();
    let mut outer_lines = Vec::new();
    // The same classification on every face with several loops. Faces
    // bounded only by FACE_BOUND agree by construction, so disagreements
    // are with a FACE_OUTER_BOUND: declared outer loop contained in another
    // (per file), or a band whose declared outer loop is not the widest
    // (ties excluded).
    let (mut declared_faces, mut declared_bands) = (0, 0);
    let mut declared_wrong = std::collections::BTreeMap::<String, usize>::new();
    for f in &files {
        let src = std::fs::read(f).expect("read");
        let t = Instant::now();
        let r = fluut_io_step::parse(&src);
        let dt = t.elapsed().as_secs_f64();
        bytes += src.len();
        secs += dt;
        let name = f.file_name().unwrap_or_default().to_string_lossy();
        match r {
            Ok(m) => {
                ok += 1;
                ents += m.entities.len();
                *schemas
                    .entry(format!("{:?}", fluut_io_step::schema(&m)))
                    .or_default() += 1;
                let t = Instant::now();
                let bodies = fluut_io_step::map_bodies(&m);
                map_secs += t.elapsed().as_secs_f64();
                let mut file_ok = true;
                for r in bodies {
                    n_bodies += 1;
                    let Ok(mb) = r else {
                        n_failed += 1;
                        file_ok = false;
                        continue;
                    };
                    n_faces += mb.body.faces.len();
                    let st = &mb.pcurves;
                    pc.coedges += st.coedges;
                    pc.given += st.given;
                    pc.exact += st.exact;
                    pc.sampled += st.sampled;
                    pc.within_tol += st.within_tol;
                    pc.max_dev = pc.max_dev.max(st.max_dev);
                    pc_rejected += mb.pcurves_rejected;
                    let o = &mb.outer;
                    outer.faces += o.faces;
                    outer.single += o.single;
                    outer.contains += o.contains;
                    outer.wraps += o.wraps;
                    let kind = |f: u32| {
                        let s = mb.body.faces[f as usize].surface;
                        format!("{:?}", mb.body.surfaces[s as usize].kind())
                    };
                    for r in &o.fallback {
                        outer_kinds.entry(kind(r.face)).or_default()[0] += 1;
                        let n = mb.body.faces[r.face as usize].loops.len();
                        outer_lines.push(format!(
                            "  undecided: {name} #{} {} ({n} loops)",
                            r.id,
                            kind(r.face)
                        ));
                    }
                    for c in &o.changed {
                        outer_kinds.entry(kind(c.face.face)).or_default()[1] += 1;
                        let n = mb.body.faces[c.face.face as usize].loops.len();
                        outer_lines.push(format!(
                            "  changed: {name} #{} {} ({n} loops): bound {} instead of {}",
                            c.face.id,
                            kind(c.face.face),
                            c.chosen,
                            c.widest
                        ));
                    }
                    for (fi, face) in mb.body.faces.iter().enumerate() {
                        if face.loops.len() < 2 {
                            continue;
                        }
                        let b = &mb.body;
                        let flagged = face.loops.iter().position(|&l| b.loops[l as usize].outer);
                        let w = |i: usize| {
                            fluut_topo::trim::loop_width(
                                b,
                                &b.loops[face.loops[i] as usize].coedges,
                            )
                        };
                        match fluut_topo::trim::outer_loop(b, fi as u32) {
                            fluut_topo::trim::Outer::Contains(i) => {
                                declared_faces += 1;
                                if Some(i) != flagged {
                                    *declared_wrong.entry(name.to_string()).or_default() += 1;
                                }
                            }
                            fluut_topo::trim::Outer::Wraps(i) => {
                                declared_faces += 1;
                                let tie =
                                    flagged.is_some_and(|j| (w(i) - w(j)).abs() <= 1e-9 * w(i));
                                declared_bands += (Some(i) != flagged && !tie) as usize;
                            }
                            fluut_topo::trim::Outer::Undecided => {}
                        }
                    }
                    outer.fallback.extend_from_slice(&o.fallback);
                    outer.changed.extend_from_slice(&o.changed);
                    for f in &mb.body.faces {
                        let k = format!("{:?}", mb.body.surfaces[f.surface as usize].kind());
                        for &l in &f.loops {
                            *pc_kinds.entry(k.clone()).or_default() +=
                                mb.body.loops[l as usize].coedges.len();
                        }
                    }
                    for f in &st.failures {
                        if mb.units.uncertainty_mm.is_some_and(|u| f.dev <= u) {
                            pc_declared += 1;
                        }
                        let e = pc_fail.entry(format!("{:?}", f.kind)).or_default();
                        e.0 += 1;
                        e.1 += f.off_surface as usize;
                        pc_worst.push((
                            f.dev,
                            format!(
                                "{name} coedge {} {:?} off_surface={}",
                                f.coedge, f.kind, f.off_surface
                            ),
                        ));
                    }
                    // Time the projection alone on a copy without p-curves.
                    let mut copy = mb.body.clone();
                    copy.pcurves.clear();
                    let t = Instant::now();
                    let _ = fluut_topo::pcurve::compute(&mut copy);
                    pc_secs += t.elapsed().as_secs_f64();
                    n_skipped += mb.skipped.len();
                    for e in &mb.skipped {
                        *skips.entry(e.msg.clone()).or_default() += 1;
                        file_ok = false;
                    }
                    let strict = fluut_topo::check(&mb.body, mb.body.tol_model);
                    let loose = fluut_topo::check(&mb.body, 1e-3);
                    let declared = fluut_topo::check(
                        &mb.body,
                        mb.units.uncertainty_mm.unwrap_or(mb.body.tol_model),
                    );
                    for i in &strict {
                        let k = format!("{i:?}");
                        let k = k.split([' ', '{']).next().unwrap_or("").to_owned();
                        *issues.entry(k).or_default() += 1;
                    }
                    for (si, sh) in mb.body.shells.iter().enumerate() {
                        let mine = |is: &[fluut_topo::Issue]| {
                            is.iter().any(|i| match i {
                                fluut_topo::Issue::NotManifold { shell, .. } => {
                                    *shell as usize == si
                                }
                                fluut_topo::Issue::OuterLoops { face, .. }
                                | fluut_topo::Issue::LoopGap { face, .. }
                                | fluut_topo::Issue::EdgeOffSurface { face, .. } => {
                                    sh.faces.contains(face)
                                }
                                fluut_topo::Issue::VertexOffCurve { .. } => true,
                            })
                        };
                        shells += 1;
                        let ok = !mine(&strict);
                        shells_ok += ok as usize;
                        shells_ok_loose += !mine(&loose) as usize;
                        shells_ok_declared += !mine(&declared) as usize;
                        if sh.closed {
                            closed += 1;
                            closed_ok += ok as usize;
                        }
                    }
                }
                files_ok += file_ok as usize;
                let c = fluut_io_step::census(&m);
                for (k, n) in c.supported {
                    *supported.entry(k).or_default() += n;
                }
                for (k, n) in c.unsupported {
                    let e = unsupported.entry(k).or_default();
                    e.0 += n;
                    e.1 += 1;
                }
                println!(
                    "{name}: {:.1} MB, {} entities, {:.1} ms",
                    src.len() as f64 / 1e6,
                    m.entities.len(),
                    dt * 1e3
                );
            }
            Err(e) => println!("{name}: PARSE ERROR {e}"),
        }
    }
    println!(
        "parsed {ok}/{} files, {ents} entities, {:.1} MB in {:.3} s = {:.0} MB/s",
        files.len(),
        bytes as f64 / 1e6,
        secs,
        bytes as f64 / 1e6 / secs
    );
    println!(
        "mapping: {n_bodies} bodies ({n_failed} failed), {n_faces} faces mapped, {n_skipped} skipped, {files_ok}/{} files fully mapped, {:.3} s",
        files.len(),
        map_secs
    );
    println!(
        "validity: {shells_ok}/{shells} shells clean at tol_model (uncertainty clamped to [1e-7, 1e-2] mm), {shells_ok_declared} at the declared uncertainty, {shells_ok_loose} at 1e-3 mm; closed shells {closed_ok}/{closed}"
    );
    println!("issues at tol_model: {issues:?}");
    println!(
        "p-curves: {} coedges, {} from the file ({pc_rejected} rejected), {} exact, {} sampled; {} within tol_model ({:.2} %), worst {:.3e} mm; projection {:.3} s",
        pc.coedges,
        pc.given,
        pc.exact,
        pc.sampled,
        pc.within_tol,
        100.0 * pc.within_tol as f64 / pc.coedges.max(1) as f64,
        pc.max_dev,
        pc_secs
    );
    println!(
        "p-curves within the file's declared uncertainty: {} ({:.2} %)",
        pc.within_tol + pc_declared,
        100.0 * (pc.within_tol + pc_declared) as f64 / pc.coedges.max(1) as f64
    );
    println!("p-curve coedges by surface kind: {pc_kinds:?}");
    println!("p-curve failures by surface kind (all, edge off the surface): {pc_fail:?}");
    pc_worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (d, w) in pc_worst.iter().take(10) {
        println!("  {d:.3e} mm: {w}");
    }
    println!(
        "outer loops of FACE_BOUND-only faces: {} faces, {} with one loop, {} by containment, {} bands, {} undecided (widest kept), {} differ from the widest loop",
        outer.faces,
        outer.single,
        outer.contains,
        outer.wraps,
        outer.fallback.len(),
        outer.changed.len()
    );
    println!("outer loops by surface kind (undecided, changed): {outer_kinds:?}");
    println!(
        "faces with several loops decided in (u, v): {declared_faces}; FACE_OUTER_BOUND inside another loop, by file: {declared_wrong:?}; bands whose FACE_OUTER_BOUND is not the widest loop: {declared_bands}"
    );
    for l in &outer_lines {
        println!("{l}");
    }
    println!("skipped faces: {skips:?}");
    println!("schemas: {schemas:?}");
    println!("supported entity kinds in bodies:");
    for (k, n) in &supported {
        println!("  {k}: {n}");
    }
    println!("unsupported entity kinds in bodies (count, files):");
    for (k, (n, f)) in &unsupported {
        println!("  {k}: {n} in {f} files");
    }
}
