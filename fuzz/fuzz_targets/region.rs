// SPDX-License-Identifier: MIT OR Apache-2.0

//! Offsets and booleans of small regions (spec 06 §6.3), as the strategies
//! use them: a region built from rectangles, circles and triangles by up to
//! eight offsets and booleans, through the `try_` methods. Coordinates are
//! on a 1/4 mm grid with a micrometre jitter, so near-degenerate shapes
//! (slivers, collinear and touching edges, tangent circles) are common.
//!
//! By default any panic in `cavalier_contours` is a finding, also one that
//! the retries of `fluut_cam::region` would have caught. With
//! `REGION_FUZZ_LENIENT` set, only a `RegionFailure` (a failure after the
//! retries) is. Either way the input is saved to `artifacts/region/`
//! first: on Windows the abort leaves libFuzzer no time to save it.

#![no_main]

use fluut_cam::region::{Region2, RegionFailure};
use libfuzzer_sys::fuzz_target;
use std::cell::RefCell;

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn lenient() -> bool {
    std::env::var_os("REGION_FUZZ_LENIENT").is_some()
}

/// Saves the current input as `artifacts/region/crash-<hash>`.
fn save_input() {
    INPUT.with(|i| {
        let data = i.borrow();
        let hash = data.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
            (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
        });
        let _ = std::fs::create_dir_all("artifacts/region");
        let path = format!("artifacts/region/crash-{hash:016x}");
        if std::fs::write(&path, &*data).is_ok() {
            eprintln!("input saved to {path}");
        }
    });
}

/// Reads bytes in order; `None` when they run out.
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn byte(&mut self) -> Option<u8> {
        let (&b, rest) = self.0.split_first()?;
        self.0 = rest;
        Some(b)
    }

    /// 0 to 15.75 on a 1/4 grid, plus 0 to 3 µm.
    fn coord(&mut self) -> Option<f64> {
        let v = self.byte()?;
        Some(f64::from(v >> 2) / 4.0 + f64::from(v & 3) * 1e-6)
    }

    fn shape(&mut self) -> Option<Region2> {
        let kind = self.byte()? % 3;
        let (x, y) = (self.coord()?, self.coord()?);
        Some(match kind {
            0 => {
                let (w, h) = (self.coord()?, self.coord()?);
                if w <= 0.0 || h <= 0.0 {
                    return None;
                }
                Region2::rect(x, y, x + w, y + h)
            }
            1 => {
                let r = self.coord()? / 2.0;
                if r <= 0.0 {
                    return None;
                }
                Region2::circle(x, y, r)
            }
            _ => {
                let p = [
                    (x, y),
                    (self.coord()?, self.coord()?),
                    (self.coord()?, self.coord()?),
                ];
                Region2::polygon(&p)
            }
        })
    }
}

fn ok(r: Result<Region2, RegionFailure>) -> Region2 {
    match r {
        Ok(r) => r,
        Err(e) => {
            save_input();
            panic!("{e}");
        }
    }
}

fuzz_target!(
    init: {
        let fuzzer_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !lenient() {
                save_input();
                fuzzer_hook(info);
            }
        }));
    },
    |data: &[u8]| {
        INPUT.with(|i| *i.borrow_mut() = data.to_vec());
        let mut b = Bytes(data);
        let Some(mut acc) = b.shape() else {
            return;
        };
        for _ in 0..8 {
            let Some(op) = b.byte() else {
                return;
            };
            acc = match op % 4 {
                0 => {
                    let Some(d) = b.byte() else {
                        return;
                    };
                    ok(acc.try_offset((f64::from(d) - 128.0) / 32.0))
                }
                k => {
                    let Some(s) = b.shape() else {
                        return;
                    };
                    match k {
                        1 => ok(acc.try_union(&s)),
                        2 => ok(acc.try_intersect(&s)),
                        _ => ok(acc.try_subtract(&s)),
                    }
                }
            };
            // Repeated offsets can multiply micrometre loops (an open issue in
            // `docs/plan/2026-11-03-cavalier-panic.md`); stop before that turns
            // into timeouts.
            if acc.loops.iter().map(|l| l.verts.len()).sum::<usize>() > 500 {
                return;
            }
        }
    }
);
