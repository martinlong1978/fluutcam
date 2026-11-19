// SPDX-License-Identifier: MIT OR Apache-2.0

//! Picks by name (spec 03 §3.5 item 4, D06 §5.1): finding a named face or
//! edge in a body with one of RFC 0001's outcomes, naming a pick, and
//! telling whether a pick still points at the entity it names.
//!
//! Names are opaque; the only syntax used is that of the exchange format
//! (`fluut_io_craftx::format`): pieces `<name>#[…]` and edges
//! `<face A>|<face B>`.

use fluut_cam::geometry::{BodyGeom, SetupBody};
use fluut_io_craftx::format;
use fluut_topo::names::edge_faces;
use fluut_topo::{BrepBody, Names};

/// How a named pick was found in a new revision (RFC 0001 §4).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Outcome {
    /// The name is there.
    Exact,
    /// Another face answers to the name as an alias.
    Merged,
    /// The name matched only a face marked `exact: false`.
    ByGeometry,
    /// Several pieces carry the name; the pick becomes all of them.
    Split,
    /// Nothing answers to the name: the toolpath is broken.
    Missing,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Exact => "exact",
            Outcome::Merged => "merged",
            Outcome::ByGeometry => "by_geometry",
            Outcome::Split => "split",
            Outcome::Missing => "missing",
        }
    }
}

/// The entities a name was found on, by index into the body's faces or
/// edges, and how.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Found {
    pub outcome: Outcome,
    pub entities: Vec<u32>,
}

impl Found {
    fn missing() -> Found {
        Found {
            outcome: Outcome::Missing,
            entities: Vec::new(),
        }
    }
}

/// Finds the face named `name`: by its own name, as pieces of it, as an
/// alias, or (for the name of a piece) as the face the pieces merged back
/// into.
pub fn find_face(n: &Names, name: &str) -> Found {
    let (outcome, faces) = {
        let own = n.faces_named(name);
        let pieces = n.pieces_of(name);
        let aliased = n.faces_aliased(name);
        if !own.is_empty() {
            (Outcome::Exact, own)
        } else if pieces.len() == 1 {
            (Outcome::Exact, pieces)
        } else if pieces.len() > 1 {
            (Outcome::Split, pieces)
        } else if aliased.len() == 1 {
            (Outcome::Merged, aliased)
        } else if aliased.len() > 1 {
            (Outcome::Split, aliased)
        } else if let Some(parent) = format::piece_of(name)
            && let whole = n.faces_named(parent)
            && !whole.is_empty()
        {
            (Outcome::Merged, whole)
        } else {
            return Found::missing();
        }
    };
    let inexact = faces.iter().any(|&f| n.face(f).is_some_and(|x| !x.exact));
    let outcome = match outcome {
        Outcome::Exact | Outcome::Merged if inexact => Outcome::ByGeometry,
        o => o,
    };
    Found {
        outcome,
        entities: faces,
    }
}

/// Finds the edge named `name`: by a name the source gave it, else as the
/// edge between its two named faces, so it survives a split of one of
/// them. Where several edges lie between the two faces, `hint` (the edge
/// geometry matching found) chooses among them.
pub fn find_edge(b: &BrepBody, name: &str, hint: Option<u32>) -> Found {
    let n = &b.names;
    let given: Vec<u32> = n
        .edges_named(name)
        .into_iter()
        .filter(|&e| n.edge(e).is_some_and(|x| !x.derived))
        .collect();
    if let [e] = given[..] {
        let exact = n.edge(e).is_some_and(|x| x.exact);
        return Found {
            outcome: if exact {
                Outcome::Exact
            } else {
                Outcome::ByGeometry
            },
            entities: vec![e],
        };
    }
    let Some((a, c)) = format::edge_faces(name) else {
        return Found::missing();
    };
    let (fa, fc) = (find_face(n, a), find_face(n, c));
    if fa.outcome == Outcome::Missing || fc.outcome == Outcome::Missing {
        return Found::missing();
    }
    let edges: Vec<u32> = edge_faces(b)
        .iter()
        .enumerate()
        .filter(|(_, fs)| {
            fs.iter().any(|f| {
                fa.entities.contains(f) && fs.iter().any(|g| g != f && fc.entities.contains(g))
            })
        })
        .map(|(e, _)| e as u32)
        .collect();
    let one = |e: u32| {
        let worst = fa.outcome.max(fc.outcome);
        Found {
            outcome: if worst == Outcome::Split {
                Outcome::Exact
            } else {
                worst
            },
            entities: vec![e],
        }
    };
    match edges[..] {
        [] => Found::missing(),
        [e] => one(e),
        _ => match hint.filter(|h| edges.contains(h)) {
            Some(e) => one(e),
            None => Found {
                outcome: Outcome::Split,
                entities: edges,
            },
        },
    }
}

/// The name a pick on face `f` stores.
pub fn face_name(n: &Names, f: u32) -> Option<String> {
    n.face(f).map(|x| x.name.clone())
}

/// The name a pick on edge `e` stores: its own, or the names of its two
/// faces.
pub fn edge_name(b: &BrepBody, e: u32) -> Option<String> {
    if let Some(x) = b.names.edge(e) {
        return Some(x.name.clone());
    }
    let adj = edge_faces(b);
    let [f0, f1] = adj.get(e as usize)?[..] else {
        return None;
    };
    Some(format::edge_name(
        &b.names.face(f0)?.name,
        &b.names.face(f1)?.name,
    ))
}

/// A face or edge pick: `(face?, body id, index)` from `f-<stem>.<i>` or
/// `e-<stem>.<i>`.
pub fn split_pick(id: &str) -> Option<(bool, String, u32)> {
    let (face, rest) = match id.strip_prefix("f-") {
        Some(r) => (true, r),
        None => (false, id.strip_prefix("e-")?),
    };
    let (stem, i) = rest.rsplit_once('.')?;
    Some((face, format!("b-{stem}"), i.parse().ok()?))
}

/// The B-rep body `id` among `bodies`.
pub fn brep<'a>(bodies: impl IntoIterator<Item = &'a SetupBody>, id: &str) -> Option<&'a BrepBody> {
    bodies
        .into_iter()
        .find(|b| b.id == id)
        .and_then(|b| match &b.geom {
            BodyGeom::Brep(x) => Some(x),
            BodyGeom::Wire(_) => None,
        })
}

/// The name stored for pick `id` in `bodies`, if its body names it.
pub fn pick_name<'a>(bodies: impl IntoIterator<Item = &'a SetupBody>, id: &str) -> Option<String> {
    let (face, body, i) = split_pick(id)?;
    let b = brep(bodies, &body)?;
    if face {
        face_name(&b.names, i)
    } else {
        edge_name(b, i)
    }
}

/// Whether pick `id` still points at an entity that answers to `name`.
/// `None` when it cannot tell: the body is not loaded or has no names.
pub fn pick_holds<'a>(
    bodies: impl IntoIterator<Item = &'a SetupBody>,
    id: &str,
    name: &str,
) -> Option<bool> {
    let (face, body, i) = split_pick(id)?;
    let b = brep(bodies, &body)?;
    if !b.names.has_faces() {
        return None;
    }
    let found = if face {
        (i as usize) < b.faces.len() && find_face(&b.names, name).entities.contains(&i)
    } else {
        (i as usize) < b.edges.len() && find_edge(b, name, Some(i)).entities.contains(&i)
    };
    Some(found)
}

/// The middle of the box of pick `id`'s face or edge, for "where it was".
pub fn place<'a>(bodies: impl IntoIterator<Item = &'a SetupBody>, id: &str) -> Option<[f64; 3]> {
    let (face, body, i) = split_pick(id)?;
    let b = brep(bodies, &body)?;
    let bx = if face {
        (i as usize) < b.faces.len()
    } else {
        (i as usize) < b.edges.len()
    }
    .then(|| {
        if face {
            b.face_box(i, 1e-2)
        } else {
            fluut_geom::Aabb3::from_points(&b.edge_points(i, 1e-2))
        }
    })?;
    if bx.is_empty() {
        return None;
    }
    let (lo, hi) = (bx.min, bx.max);
    Some([
        (lo.x() + hi.x()) / 2.0,
        (lo.y() + hi.y()) / 2.0,
        (lo.z() + hi.z()) / 2.0,
    ])
}
