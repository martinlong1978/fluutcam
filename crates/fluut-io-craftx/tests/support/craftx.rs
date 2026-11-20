// SPDX-License-Identifier: MIT OR Apache-2.0

//! Writes craft exchange files (`.craftx`) for tests, and the revisions of
//! one test part that the import and re-import tests use. Nothing here is
//! a hand-made binary: the STEP text comes from the shared test writer
//! (`fluut-cam/tests/support/step.rs`, declared as `crate::step` by the
//! test that includes this file), the archive from [`zip`].
//!
//! The part is an 80 × 60 × 20 block with a 40 × 30 pocket, floor at Z 12.
//! Its face names follow SolidCraft's scheme: `F1` is the block, `F2` the
//! pocket, `F3` a hole, `F5` a fillet, `F6` a rib.

#![allow(dead_code)]

use crate::step::{Kind, Profile, Writer};
use sha2::{Digest, Sha256};
use std::io::Write as _;

/// A ZIP archive of `entries`, deflated or stored.
pub fn zip(entries: &[(String, Vec<u8>)], deflate: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let packed = if deflate {
            let mut e =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(data).unwrap();
            e.finish().unwrap()
        } else {
            data.clone()
        };
        let method: u16 = if deflate { 8 } else { 0 };
        let crc = crc32fast::hash(data);
        let offset = out.len() as u32;
        let head = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend(sig.to_le_bytes());
            if central {
                h.extend(20u16.to_le_bytes());
            }
            h.extend(20u16.to_le_bytes());
            h.extend(0x0800u16.to_le_bytes());
            h.extend(method.to_le_bytes());
            h.extend(0u16.to_le_bytes());
            h.extend(0x21u16.to_le_bytes());
            h.extend(crc.to_le_bytes());
            h.extend((packed.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes());
            if central {
                // Comment length, disk, internal and external attributes.
                h.extend([0u8; 10]);
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        out.extend(head(0x0403_4b50, false));
        out.extend(&packed);
        central.extend(head(0x0201_4b50, true));
    }
    let cd_offset = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((entries.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(cd_offset.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

pub fn sha256(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

/// One face-table entry.
#[derive(Clone, Debug)]
pub struct Face {
    pub index: u32,
    pub name: String,
    pub exact: bool,
    pub aliases: Vec<String>,
    pub point: Option<[f64; 3]>,
    pub surface: Option<String>,
}

impl Face {
    pub fn new(index: u32, name: &str) -> Face {
        Face {
            index,
            name: name.to_owned(),
            exact: true,
            aliases: Vec::new(),
            point: None,
            surface: None,
        }
    }

    pub fn at(mut self, p: [f64; 3]) -> Face {
        self.point = Some(p);
        self
    }
}

fn json_str(s: &str) -> String {
    serde_json::Value::String(s.to_owned()).to_string()
}

/// An exchange file to write.
#[derive(Clone, Debug)]
pub struct Craftx {
    pub source_id: String,
    pub revision: u32,
    pub format: String,
    pub step: String,
    pub faces: Vec<Face>,
    /// Edge-table entries: EDGE_CURVE index and name.
    pub edges: Vec<(u32, String)>,
    pub params: Vec<(String, f64, String)>,
    pub requires: Vec<String>,
    /// More parts, listed in the manifest with their hash.
    pub extra: Vec<(String, Vec<u8>)>,
    pub deflate: bool,
}

impl Craftx {
    pub fn new(step: String, faces: Vec<Face>, revision: u32) -> Craftx {
        Craftx {
            source_id: "3f6c0d4e-design".to_owned(),
            revision,
            format: "craft-exchange/1".to_owned(),
            step,
            faces,
            edges: Vec::new(),
            params: vec![("wall".to_owned(), 3.0, "mm".to_owned())],
            requires: vec!["step/ap214".to_owned(), "faces.names/1".to_owned()],
            extra: Vec::new(),
            deflate: true,
        }
    }

    fn faces_json(&self) -> String {
        let faces: Vec<String> = self
            .faces
            .iter()
            .map(|f| {
                let mut s = format!(
                    "{{\"index\": {}, \"name\": {}, \"exact\": {}",
                    f.index,
                    json_str(&f.name),
                    f.exact
                );
                if !f.aliases.is_empty() {
                    let a: Vec<String> = f.aliases.iter().map(|a| json_str(a)).collect();
                    s.push_str(&format!(", \"aliases\": [{}]", a.join(", ")));
                }
                if let Some(p) = f.point {
                    s.push_str(&format!(
                        ", \"point\": [{:?}, {:?}, {:?}]",
                        p[0], p[1], p[2]
                    ));
                }
                if let Some(t) = &f.surface {
                    s.push_str(&format!(", \"surface\": {}", json_str(t)));
                }
                s.push('}');
                s
            })
            .collect();
        let edges: Vec<String> = self
            .edges
            .iter()
            .map(|(i, n)| format!("{{\"index\": {i}, \"name\": {}}}", json_str(n)))
            .collect();
        format!(
            "{{\"bodies\": [{{\"body\": 0, \"faces\": [{}], \"edges\": [{}]}}]}}",
            faces.join(", "),
            edges.join(", ")
        )
    }

    /// The parts other than the manifest, in archive order.
    pub fn parts(&self) -> Vec<(String, Vec<u8>)> {
        let params: Vec<String> = self
            .params
            .iter()
            .map(|(n, v, u)| {
                format!(
                    "{{\"name\": {}, \"value\": {v:?}, \"unit\": {}}}",
                    json_str(n),
                    json_str(u)
                )
            })
            .collect();
        let mut p = vec![
            ("bodies/0.step".to_owned(), self.step.clone().into_bytes()),
            ("faces.json".to_owned(), self.faces_json().into_bytes()),
            (
                "params.json".to_owned(),
                format!("{{\"params\": [{}]}}", params.join(", ")).into_bytes(),
            ),
            (
                "meta.json".to_owned(),
                b"{\"material\": \"MDF\", \"part_number\": \"FC-1\"}".to_vec(),
            ),
        ];
        p.extend(self.extra.iter().cloned());
        p
    }

    /// The manifest listing `parts` with their SHA-256.
    pub fn manifest(&self, parts: &[(String, Vec<u8>)]) -> String {
        let listed: Vec<String> = parts
            .iter()
            .map(|(n, d)| {
                format!(
                    "{{\"path\": {}, \"sha256\": \"{}\"}}",
                    json_str(n),
                    sha256(d)
                )
            })
            .collect();
        let req: Vec<String> = self.requires.iter().map(|r| json_str(r)).collect();
        format!(
            "{{\"format\": {}, \"source\": {{\"app\": \"SolidCraft\", \"version\": \"0.12.0\"}}, \
             \"source_id\": {}, \"revision\": {}, \"units\": \"mm\", \
             \"parts\": [{}], \"requires\": [{}]}}",
            json_str(&self.format),
            json_str(&self.source_id),
            self.revision,
            listed.join(", "),
            req.join(", ")
        )
    }

    /// Manifest first, then the parts.
    pub fn entries(&self) -> Vec<(String, Vec<u8>)> {
        let parts = self.parts();
        let mut e = vec![(
            "manifest.json".to_owned(),
            self.manifest(&parts).into_bytes(),
        )];
        e.extend(parts);
        e
    }

    pub fn bytes(&self) -> Vec<u8> {
        zip(&self.entries(), self.deflate)
    }
}

// ── The test part and its revisions ───────────────────────────────────

const OUTER: [f64; 4] = [0.0, 0.0, 80.0, 60.0];
const POCKET: [f64; 4] = [20.0, 15.0, 60.0, 45.0];
/// Z of the pocket floor.
pub const FLOOR_Z: f64 = 12.0;

fn rect(r: [f64; 4]) -> Profile {
    Profile::rect(r[0], r[1], r[2], r[3])
}

/// Names of the four walls of a rectangle profile, in profile order
/// (`l1` at low Y, then counter-clockwise).
fn sides(f: &str) -> [String; 4] {
    [1, 2, 3, 4].map(|i| format!("{f}:side:l{i}"))
}

/// Writes walls, bottom and top of the block with `holes` in its top.
fn block(w: &mut Writer, outer: &Profile, top_holes: &[Profile], bottom_holes: &[Profile]) {
    w.walls(outer, 0.0, 20.0, true);
    w.cap(outer, bottom_holes, 0.0, false);
    w.cap(outer, top_holes, 20.0, true);
}

fn pocket(w: &mut Writer, p: &Profile) {
    w.walls(p, FLOOR_Z, 20.0, false);
    w.cap(p, &[], FLOOR_Z, true);
}

fn block_names() -> Vec<String> {
    let mut v = sides("F1").to_vec();
    v.push("F1:bottom".into());
    v.push("F1:top".into());
    v
}

fn faces(names: &[String]) -> Vec<Face> {
    names
        .iter()
        .enumerate()
        .map(|(i, n)| Face::new(i as u32, n))
        .collect()
}

/// Revision 1: the block with the pocket. Faces 0–3 block walls, 4
/// bottom, 5 top, 6–9 pocket walls, 10 the pocket floor.
pub fn rev1() -> Craftx {
    let mut w = Writer::new([0.0; 3]);
    let p = rect(POCKET);
    block(&mut w, &rect(OUTER), std::slice::from_ref(&p), &[]);
    pocket(&mut w, &p);
    let mut names = block_names();
    names.extend(sides("F2"));
    names.push("F2:floor".into());
    let mut f = faces(&names);
    f[10] = f[10].clone().at([40.0, 30.0, FLOOR_Z]);
    f[5] = f[5].clone().at([5.0, 5.0, 20.0]);
    Craftx::new(w.finish("block"), f, 1)
}

/// Revision 2: a Ø8 through hole at (10, 30) is added, and written first,
/// so every face moves in the file. Faces 0–1 the hole, 2–5 block walls, 6
/// bottom, 7 top, 8–11 pocket walls, 12 the floor.
pub fn hole() -> Craftx {
    let mut w = Writer::new([0.0; 3]);
    let h = Profile::circle(10.0, 30.0, 4.0);
    let p = rect(POCKET);
    w.walls(&h, 0.0, 20.0, false);
    block(
        &mut w,
        &rect(OUTER),
        &[p.clone(), h.clone()],
        std::slice::from_ref(&h),
    );
    pocket(&mut w, &p);
    let mut names = vec!["F3:side:a".to_owned(), "F3:side:b".to_owned()];
    names.extend(block_names());
    names.extend(sides("F2"));
    names.push("F2:floor".into());
    let mut f = faces(&names);
    f[12] = f[12].clone().at([40.0, 30.0, FLOOR_Z]);
    let mut c = Craftx::new(w.finish("block"), f, 2);
    c.params[0].1 = 4.0;
    c
}

/// Revision 3: the vertical block edge at (80, 0), between `F1:side:l1`
/// and `F1:side:l2`, is rounded (R 5): that edge is gone, and the blend
/// face `F5:blend(F1:side:l1|F1:side:l2)` is face 1.
pub fn fillet() -> Craftx {
    let mut w = Writer::new([0.0; 3]);
    let r = 5.0;
    let outer = Profile {
        pts: vec![
            [0.0, 0.0],
            [80.0 - r, 0.0],
            [80.0, r],
            [80.0, 60.0],
            [0.0, 60.0],
        ],
        kinds: vec![
            Kind::Line,
            Kind::Arc {
                centre: [80.0 - r, r],
                ccw: true,
            },
            Kind::Line,
            Kind::Line,
            Kind::Line,
        ],
    };
    let p = rect(POCKET);
    block(&mut w, &outer, std::slice::from_ref(&p), &[]);
    pocket(&mut w, &p);
    let s = sides("F1");
    let mut names = vec![
        s[0].clone(),
        "F5:blend(F1:side:l1|F1:side:l2)".to_owned(),
        s[1].clone(),
        s[2].clone(),
        s[3].clone(),
        "F1:bottom".into(),
        "F1:top".into(),
    ];
    names.extend(sides("F2"));
    names.push("F2:floor".into());
    Craftx::new(w.finish("block"), faces(&names), 3)
}

/// Revision 4: a rib (X 38–42) splits the pocket in two. The floor, and
/// the pocket walls at low and high Y, are now two pieces each. Faces 0–5
/// the block, 6–9 the left pocket's walls, 10 its floor, 11–14 the right
/// pocket's walls, 15 its floor.
pub fn split() -> Craftx {
    let mut w = Writer::new([0.0; 3]);
    let left = rect([20.0, 15.0, 38.0, 45.0]);
    let right = rect([42.0, 15.0, 60.0, 45.0]);
    block(&mut w, &rect(OUTER), &[left.clone(), right.clone()], &[]);
    pocket(&mut w, &left);
    pocket(&mut w, &right);
    let mut names = block_names();
    let piece = |n: &str, by: &str| format!("{n}#[{by}]");
    names.extend([
        piece("F2:side:l1", "F6:side:l2"),
        "F6:side:l2".to_owned(),
        piece("F2:side:l3", "F6:side:l2"),
        "F2:side:l4".to_owned(),
        piece("F2:floor", "F6:side:l2"),
        piece("F2:side:l1", "F6:side:l4"),
        "F2:side:l2".to_owned(),
        piece("F2:side:l3", "F6:side:l4"),
        "F6:side:l4".to_owned(),
        piece("F2:floor", "F6:side:l4"),
    ]);
    Craftx::new(w.finish("block"), faces(&names), 4)
}

/// Revision 5: the rib of [`split`] is removed again; the floor is one face
/// that answers to both piece names (aliases).
pub fn merged() -> Craftx {
    let mut c = rev1();
    c.revision = 5;
    c.faces[10].aliases = vec![
        "F2:floor#[F6:side:l2]".to_owned(),
        "F2:floor#[F6:side:l4]".to_owned(),
    ];
    c
}

/// Revision 6: as revision 1, but SolidCraft found the floor's name by
/// geometry only (`exact: false`).
pub fn inexact() -> Craftx {
    let mut c = rev1();
    c.revision = 6;
    c.faces[10].exact = false;
    c
}
