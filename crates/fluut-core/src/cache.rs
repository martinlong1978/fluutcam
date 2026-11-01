// SPDX-License-Identifier: MIT OR Apache-2.0

//! The result cache sidecar, `<project>.fluut-cache/` (D06 §7.5, spec 03
//! §3.4, OQ-03).
//!
//! One file per `ToolpathResult`, named by its node hash (D01 §4):
//!
//! ```text
//! <hex>.tpr = "FLUT" | u16 schema (LE) | postcard(File) | u32 crc32 (LE)
//! File      = { fluut version, node hash, passes, diagnostics }
//! ```
//!
//! The passes are stored through mirror types, so `fluut-path` needs no
//! serde. A file that is missing, damaged, of another schema or version,
//! or for another hash is a miss: the cache is disposable. Decoding
//! rebuilds the result with `ToolpathResult::new`, which checks every
//! invariant again.

use fluut_geom::{Point3, UnitVec3, Vec3};
use fluut_model::{Diagnostic, Severity};
use fluut_path::{
    AxisPos, CompSide, CycleKind, CycleParams, FeedRef, MarkerKind, Motion, MotionInfo, Pass,
    PassKind, PersistId, Segment, ToolpathResult,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const MAGIC: &[u8; 4] = b"FLUT";
/// Bumped when the file layout or the mirror types change.
pub const SCHEMA: u16 = 1;
/// Unreferenced files younger than this survive garbage collection.
pub const KEEP_UNUSED: Duration = Duration::from_secs(7 * 24 * 3600);

/// The FluutCAM version stored in each file; another version is a miss.
pub fn fluut_version() -> String {
    format!(
        "fluut {} / {}",
        env!("CARGO_PKG_VERSION"),
        crate::ALGORITHM_VERSION
    )
}

/// `<project>.fluut-cache` beside the project file.
pub fn cache_dir(project: &Path) -> PathBuf {
    let mut s = project.as_os_str().to_owned();
    s.push("-cache");
    PathBuf::from(s)
}

#[derive(Serialize, Deserialize)]
struct File {
    version: String,
    hash: String,
    passes: Vec<P>,
    diagnostics: Vec<D>,
}

#[derive(Serialize, Deserialize)]
struct P {
    kind: u8,
    comp: Option<u8>,
    segments: Vec<S>,
}

#[derive(Serialize, Deserialize)]
struct A {
    xyz: [f64; 3],
    a: Option<f64>,
    b: Option<f64>,
}

#[derive(Serialize, Deserialize)]
struct M {
    motion: u8,
    feed: u8,
    tol: f32,
    face: Option<u128>,
}

#[derive(Serialize, Deserialize)]
struct C {
    kind: u8,
    top_z: f64,
    depth_z: f64,
    retract_z: f64,
    peck: Option<f64>,
    dwell_s: Option<f64>,
}

#[derive(Serialize, Deserialize)]
enum S {
    Line {
        a: A,
        b: A,
        m: M,
    },
    Arc {
        a: A,
        b: A,
        centre: [f64; 3],
        normal: [f64; 3],
        sweep: f64,
        m: M,
        helix: bool,
    },
    Rotary {
        a: A,
        b: A,
        radius: f64,
        m: M,
    },
    Cycle {
        at: A,
        cycle: C,
        m: M,
    },
    Marker {
        kind: u8,
        text: String,
        value: f64,
    },
}

#[derive(Serialize, Deserialize)]
struct D {
    code: String,
    error: bool,
    message: String,
    toolpath: Option<String>,
    field: Option<String>,
}

const PASS_KINDS: [PassKind; 5] = [
    PassKind::Cut,
    PassKind::Lead,
    PassKind::Link,
    PassKind::Ramp,
    PassKind::Retract,
];
const MOTIONS: [Motion; 8] = [
    Motion::Rapid,
    Motion::Cut,
    Motion::Plunge,
    Motion::Ramp,
    Motion::LeadIn,
    Motion::LeadOut,
    Motion::Link,
    Motion::Retract,
];
const FEEDS: [FeedRef; 7] = [
    FeedRef::Rapid,
    FeedRef::Cut,
    FeedRef::Plunge,
    FeedRef::Ramp,
    FeedRef::LeadIn,
    FeedRef::LeadOut,
    FeedRef::Transition,
];
const CYCLES: [CycleKind; 7] = [
    CycleKind::Drill,
    CycleKind::Spot,
    CycleKind::Peck,
    CycleKind::ChipBreak,
    CycleKind::Tap,
    CycleKind::Ream,
    CycleKind::Bore,
];

fn index<T: PartialEq>(all: &[T], x: &T) -> u8 {
    all.iter().position(|y| y == x).unwrap_or(0) as u8
}

fn get<T: Copy>(all: &[T], i: u8) -> Option<T> {
    all.get(i as usize).copied()
}

fn p3(p: Point3) -> [f64; 3] {
    [p.x(), p.y(), p.z()]
}

fn a_of(p: &AxisPos) -> A {
    A {
        xyz: p3(p.xyz),
        a: p.a,
        b: p.b,
    }
}

fn m_of(m: &MotionInfo) -> M {
    M {
        motion: index(&MOTIONS, &m.motion),
        feed: index(&FEEDS, &m.feed),
        tol: m.tol,
        face: m.face.map(|f| f.0),
    }
}

/// A unit vector that reads back bit for bit, or `None` (the result is
/// then not cached).
fn unit(n: UnitVec3) -> Option<[f64; 3]> {
    let v = n.get();
    let back = UnitVec3::new(v)?.get();
    let same = [v.x(), v.y(), v.z()]
        .iter()
        .zip([back.x(), back.y(), back.z()])
        .all(|(a, b)| a.to_bits() == b.to_bits());
    same.then(|| [v.x(), v.y(), v.z()])
}

fn seg_of(s: &Segment) -> Option<S> {
    Some(match s {
        Segment::Line { a, b, m } => S::Line {
            a: a_of(a),
            b: a_of(b),
            m: m_of(m),
        },
        Segment::Arc {
            a,
            b,
            centre,
            normal,
            sweep,
            m,
        }
        | Segment::Helix {
            a,
            b,
            centre,
            normal,
            sweep,
            m,
        } => S::Arc {
            a: a_of(a),
            b: a_of(b),
            centre: p3(*centre),
            normal: unit(*normal)?,
            sweep: *sweep,
            m: m_of(m),
            helix: matches!(s, Segment::Helix { .. }),
        },
        Segment::Rotary { a, b, radius, m } => S::Rotary {
            a: a_of(a),
            b: a_of(b),
            radius: *radius,
            m: m_of(m),
        },
        Segment::Cycle { at, cycle, m } => S::Cycle {
            at: a_of(at),
            cycle: C {
                kind: index(&CYCLES, &cycle.kind),
                top_z: cycle.top_z,
                depth_z: cycle.depth_z,
                retract_z: cycle.retract_z,
                peck: cycle.peck,
                dwell_s: cycle.dwell_s,
            },
            m: m_of(m),
        },
        Segment::Marker { kind } => {
            let (k, text, value) = match kind {
                MarkerKind::Stop => (0, String::new(), 0.0),
                MarkerKind::OptionalStop => (1, String::new(), 0.0),
                MarkerKind::Comment(t) => (2, t.clone(), 0.0),
                MarkerKind::Dwell(d) => (3, String::new(), *d),
                MarkerKind::PassThrough(t) => (4, t.clone(), 0.0),
            };
            S::Marker {
                kind: k,
                text,
                value,
            }
        }
        // Spline segments are not emitted in v1 (OQ-07): not cached.
        Segment::Spline { .. } => return None,
    })
}

fn pos(a: &A) -> AxisPos {
    AxisPos {
        xyz: Point3::new(a.xyz[0], a.xyz[1], a.xyz[2]),
        a: a.a,
        b: a.b,
    }
}

fn motion(m: &M) -> Option<MotionInfo> {
    Some(MotionInfo {
        motion: get(&MOTIONS, m.motion)?,
        feed: get(&FEEDS, m.feed)?,
        tol: m.tol,
        face: m.face.map(PersistId),
    })
}

fn seg(s: &S) -> Option<Segment> {
    Some(match s {
        S::Line { a, b, m } => Segment::Line {
            a: pos(a),
            b: pos(b),
            m: motion(m)?,
        },
        S::Arc {
            a,
            b,
            centre,
            normal,
            sweep,
            m,
            helix,
        } => {
            let n = UnitVec3::new(Vec3::new(normal[0], normal[1], normal[2]))?;
            let (a, b, centre, m) = (
                pos(a),
                pos(b),
                Point3::new(centre[0], centre[1], centre[2]),
                motion(m)?,
            );
            if *helix {
                Segment::Helix {
                    a,
                    b,
                    centre,
                    normal: n,
                    sweep: *sweep,
                    m,
                }
            } else {
                Segment::Arc {
                    a,
                    b,
                    centre,
                    normal: n,
                    sweep: *sweep,
                    m,
                }
            }
        }
        S::Rotary { a, b, radius, m } => Segment::Rotary {
            a: pos(a),
            b: pos(b),
            radius: *radius,
            m: motion(m)?,
        },
        S::Cycle { at, cycle: c, m } => Segment::Cycle {
            at: pos(at),
            cycle: CycleParams {
                kind: get(&CYCLES, c.kind)?,
                top_z: c.top_z,
                depth_z: c.depth_z,
                retract_z: c.retract_z,
                peck: c.peck,
                dwell_s: c.dwell_s,
            },
            m: motion(m)?,
        },
        S::Marker { kind, text, value } => Segment::Marker {
            kind: match kind {
                0 => MarkerKind::Stop,
                1 => MarkerKind::OptionalStop,
                2 => MarkerKind::Comment(text.clone()),
                3 => MarkerKind::Dwell(*value),
                4 => MarkerKind::PassThrough(text.clone()),
                _ => return None,
            },
        },
    })
}

/// The file bytes, or `None` when the result cannot be stored exactly.
pub fn encode(hash: &str, r: &ToolpathResult, diags: &[Diagnostic]) -> Option<Vec<u8>> {
    let passes = r
        .passes()
        .iter()
        .map(|p| {
            Some(P {
                kind: index(&PASS_KINDS, &p.kind),
                comp: p.comp.map(|c| match c {
                    CompSide::Left => 0,
                    CompSide::Right => 1,
                }),
                segments: p.segments.iter().map(seg_of).collect::<Option<_>>()?,
            })
        })
        .collect::<Option<_>>()?;
    let f = File {
        version: fluut_version(),
        hash: hash.to_owned(),
        passes,
        diagnostics: diags
            .iter()
            .map(|d| D {
                code: d.code.clone(),
                error: d.is_error(),
                message: d.message.clone(),
                toolpath: d.toolpath.clone(),
                field: d.field.clone(),
            })
            .collect(),
    };
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&SCHEMA.to_le_bytes());
    out.extend(postcard::to_allocvec(&f).ok()?);
    let crc = crc32fast::hash(&out);
    out.extend_from_slice(&crc.to_le_bytes());
    Some(out)
}

/// A cached result and its diagnostics.
pub type Entry = (ToolpathResult, Vec<Diagnostic>);

/// The result in `bytes` if it is a sound file for `hash` from this
/// FluutCAM version.
pub fn decode(bytes: &[u8], hash: &str) -> Option<Entry> {
    let n = bytes.len().checked_sub(4)?;
    let (body, crc) = bytes.split_at(n);
    if body.len() < 6 || &body[..4] != MAGIC || crc32fast::hash(body).to_le_bytes() != crc {
        return None;
    }
    if u16::from_le_bytes([body[4], body[5]]) != SCHEMA {
        return None;
    }
    let f: File = postcard::from_bytes(&body[6..]).ok()?;
    if f.hash != hash || f.version != fluut_version() {
        return None;
    }
    let passes = f
        .passes
        .iter()
        .map(|p| {
            Some(Pass {
                kind: get(&PASS_KINDS, p.kind)?,
                comp: match p.comp {
                    None => None,
                    Some(0) => Some(CompSide::Left),
                    Some(1) => Some(CompSide::Right),
                    Some(_) => return None,
                },
                segments: p.segments.iter().map(seg).collect::<Option<_>>()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let result = ToolpathResult::new(passes).ok()?;
    let diags = f
        .diagnostics
        .into_iter()
        .map(|d| Diagnostic {
            code: d.code,
            severity: if d.error {
                Severity::Error
            } else {
                Severity::Warning
            },
            message: d.message,
            toolpath: d.toolpath,
            field: d.field,
        })
        .collect();
    Some((result, diags))
}

/// The cache folder of one project.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiskCache {
    dir: PathBuf,
}

impl DiskCache {
    pub fn new(dir: PathBuf) -> DiskCache {
        DiskCache { dir }
    }

    /// The cache beside a project file.
    pub fn for_project(path: &Path) -> DiskCache {
        DiskCache::new(cache_dir(path))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file of a node hash (`sha256:<hex>` gives `<hex>.tpr`).
    pub fn file(&self, hash: &str) -> PathBuf {
        let name = hash.rsplit(':').next().unwrap_or(hash);
        self.dir.join(format!("{name}.tpr"))
    }

    /// The cached result for `hash`; a hit marks the file as used now.
    pub fn get(&self, hash: &str) -> Option<Entry> {
        let path = self.file(hash);
        let bytes = std::fs::read(&path).ok()?;
        let e = decode(&bytes, hash)?;
        if let Ok(f) = std::fs::File::options().append(true).open(&path) {
            let _ = f.set_modified(SystemTime::now());
        }
        Some(e)
    }

    /// Stores a result. Returns `false` when it cannot be stored exactly
    /// (it is then simply recalculated next time).
    pub fn put(&self, hash: &str, r: &ToolpathResult, diags: &[Diagnostic]) -> io::Result<bool> {
        let Some(bytes) = encode(hash, r, diags) else {
            return Ok(false);
        };
        std::fs::create_dir_all(&self.dir)?;
        let path = self.file(hash);
        let tmp = path.with_extension("tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&bytes)?;
        }
        std::fs::rename(&tmp, &path)?;
        Ok(true)
    }

    /// Deletes cache files whose hash is not in `referenced` and that were
    /// not used within [`KEEP_UNUSED`] of `now` (D06 §7.5). Returns how
    /// many were deleted.
    pub fn gc(&self, referenced: &BTreeSet<String>, now: SystemTime) -> io::Result<usize> {
        let keep: BTreeSet<PathBuf> = referenced.iter().map(|h| self.file(h)).collect();
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let mut n = 0;
        let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            let ext = p.extension().and_then(|x| x.to_str());
            if !matches!(ext, Some("tpr" | "tmp")) || keep.contains(&p) {
                continue;
            }
            let used = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
            let old = used.is_none_or(|t| now.duration_since(t).is_ok_and(|d| d >= KEEP_UNUSED));
            if old {
                std::fs::remove_file(&p)?;
                n += 1;
            }
        }
        Ok(n)
    }
}
