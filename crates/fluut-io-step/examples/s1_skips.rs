// SPDX-License-Identifier: MIT OR Apache-2.0

//! Lists the first skipped faces and check issues of one file:
//! `cargo run -p fluut-io-step --example s1_skips --release -- FILE`.

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        return;
    };
    let src = std::fs::read(&path).expect("read");
    let m = fluut_io_step::parse(&src).expect("parse");
    for b in fluut_io_step::map_bodies(&m).into_iter().flatten() {
        for e in b.skipped.iter().take(3) {
            println!("skip #{}: {}", e.id, e.msg);
        }
        for i in fluut_topo::check(&b.body, b.body.tol_model)
            .iter()
            .filter(|i| !matches!(i, fluut_topo::Issue::OuterLoops { .. }))
            .take(3)
        {
            let what = match i {
                fluut_topo::Issue::EdgeOffSurface { face, edge, .. } => format!(
                    "surface {:?}, curve {:?}",
                    b.body.surfaces[b.body.faces[*face as usize].surface as usize].kind(),
                    b.body.edges[*edge as usize]
                        .curve
                        .map(|c| std::mem::discriminant(&b.body.curves[c as usize]))
                ),
                fluut_topo::Issue::VertexOffCurve { edge, .. } => {
                    let e = &b.body.edges[*edge as usize];
                    let c = e.curve.map(|c| &b.body.curves[c as usize]);
                    format!(
                        "range {:?}, natural {:?}, circle {}",
                        e.range,
                        c.and_then(|c| c.natural_range()),
                        c.is_some_and(|c| c.as_circle().is_some())
                    )
                }
                _ => String::new(),
            };
            println!(
                "issue in body #{} (tol {}): {i:?} {what}",
                b.id, b.body.tol_model
            );
        }
    }
}
