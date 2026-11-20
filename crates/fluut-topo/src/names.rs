// SPDX-License-Identifier: MIT OR Apache-2.0

//! Persistent names of faces and edges (design D03 §6).
//!
//! A source file may name its faces and edges: a STEP `ADVANCED_FACE` or
//! `EDGE_CURVE` name, or the face table of a `.craftx` exchange file. The
//! names are opaque strings here. Readers that know a naming scheme fill in
//! [`EntityName::piece_of`] and [`EntityName::between`], so this module
//! never parses a name.

use crate::BrepBody;

/// The name a face or edge carries in its source file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntityName {
    /// As given by the source.
    pub name: String,
    /// Other names it answers to, for example after two faces merged.
    pub aliases: Vec<String>,
    /// False when the source found the name by geometry only, or the
    /// reader could not confirm which face it belongs to.
    pub exact: bool,
    /// For one piece of a split face: the name of the face it was cut
    /// from.
    pub piece_of: Option<String>,
    /// For an edge: the names of the two faces it lies between, when the
    /// name gives them.
    pub between: Option<(String, String)>,
    /// Made by the reader (an edge named from its two faces), not given by
    /// the source. Such names may repeat.
    pub derived: bool,
}

impl EntityName {
    /// An exact name with no aliases.
    pub fn new(name: &str) -> EntityName {
        EntityName {
            name: name.to_owned(),
            exact: true,
            ..EntityName::default()
        }
    }

    /// Whether the entity answers to `name`: its own name, an alias, or
    /// the name of the face it is a piece of.
    pub fn answers_to(&self, name: &str) -> bool {
        self.name == name
            || self.aliases.iter().any(|a| a == name)
            || self.piece_of.as_deref() == Some(name)
    }
}

/// Name tables of one B-rep body, indexed like `BrepBody::faces` and
/// `BrepBody::edges`. Both are empty for a body without names; a table
/// shorter than the entity list leaves the rest unnamed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Names {
    pub faces: Vec<Option<EntityName>>,
    pub edges: Vec<Option<EntityName>>,
}

fn put(v: &mut Vec<Option<EntityName>>, i: u32, n: Option<EntityName>) {
    let i = i as usize;
    if v.len() <= i {
        v.resize(i + 1, None);
    }
    v[i] = n;
}

fn get(v: &[Option<EntityName>], i: u32) -> Option<&EntityName> {
    v.get(i as usize).and_then(Option::as_ref)
}

fn find(v: &[Option<EntityName>], f: impl Fn(&EntityName) -> bool) -> Vec<u32> {
    v.iter()
        .enumerate()
        .filter(|(_, n)| n.as_ref().is_some_and(&f))
        .map(|(i, _)| i as u32)
        .collect()
}

/// Clears every name (of the entities `keep` does not exempt) carried by
/// more than one entity. Returns the names cleared.
fn drop_repeats(v: &mut [Option<EntityName>], keep: impl Fn(&EntityName) -> bool) -> Vec<String> {
    let mut count = std::collections::BTreeMap::<String, usize>::new();
    for n in v.iter().flatten().filter(|n| !keep(n)) {
        *count.entry(n.name.clone()).or_default() += 1;
    }
    let repeated: Vec<String> = count
        .into_iter()
        .filter(|(_, c)| *c > 1)
        .map(|(n, _)| n)
        .collect();
    for slot in v.iter_mut() {
        if slot
            .as_ref()
            .is_some_and(|n| !keep(n) && repeated.contains(&n.name))
        {
            *slot = None;
        }
    }
    repeated
}

impl Names {
    /// No face and no edge carries a name.
    pub fn is_empty(&self) -> bool {
        self.faces.iter().chain(&self.edges).all(Option::is_none)
    }

    /// Whether any face carries a name.
    pub fn has_faces(&self) -> bool {
        self.faces.iter().any(Option::is_some)
    }

    pub fn face(&self, f: u32) -> Option<&EntityName> {
        get(&self.faces, f)
    }

    pub fn edge(&self, e: u32) -> Option<&EntityName> {
        get(&self.edges, e)
    }

    pub fn set_face(&mut self, f: u32, n: Option<EntityName>) {
        put(&mut self.faces, f, n);
    }

    pub fn set_edge(&mut self, e: u32, n: Option<EntityName>) {
        put(&mut self.edges, e, n);
    }

    /// Faces whose own name is `name`.
    pub fn faces_named(&self, name: &str) -> Vec<u32> {
        find(&self.faces, |n| n.name == name)
    }

    /// Faces that list `name` as an alias.
    pub fn faces_aliased(&self, name: &str) -> Vec<u32> {
        find(&self.faces, |n| n.aliases.iter().any(|a| a == name))
    }

    /// Pieces of the face named `name`.
    pub fn pieces_of(&self, name: &str) -> Vec<u32> {
        find(&self.faces, |n| n.piece_of.as_deref() == Some(name))
    }

    /// Edges whose own name is `name`.
    pub fn edges_named(&self, name: &str) -> Vec<u32> {
        find(&self.edges, |n| n.name == name)
    }

    /// Clears face names carried by more than one face, and edge names
    /// (other than derived ones) carried by more than one edge: such a
    /// name identifies nothing. Returns the names cleared.
    pub fn drop_repeated(&mut self) -> Vec<String> {
        let mut out = drop_repeats(&mut self.faces, |_| false);
        out.extend(drop_repeats(&mut self.edges, |n| n.derived));
        out
    }
}

/// The faces each edge bounds, indexed like `BrepBody::edges`: two for a
/// manifold edge, one on a sheet boundary.
pub fn edge_faces(b: &BrepBody) -> Vec<Vec<u32>> {
    let mut out = vec![Vec::new(); b.edges.len()];
    for (f, face) in b.faces.iter().enumerate() {
        for &l in &face.loops {
            let Some(lp) = b.loops.get(l as usize) else {
                continue;
            };
            for &c in &lp.coedges {
                if let Some(ce) = b.coedges.get(c as usize)
                    && let Some(fs) = out.get_mut(ce.edge as usize)
                    && !fs.contains(&(f as u32))
                {
                    fs.push(f as u32);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_to_its_name_aliases_and_parent() {
        let mut n = EntityName::new("F2:top#[a]");
        n.aliases.push("F7:top".into());
        n.piece_of = Some("F2:top".into());
        assert!(n.answers_to("F2:top#[a]"));
        assert!(n.answers_to("F7:top"));
        assert!(n.answers_to("F2:top"));
        assert!(!n.answers_to("F2:side"));
    }

    #[test]
    fn lookups_and_repeats() {
        let mut t = Names::default();
        assert!(t.is_empty());
        t.set_face(3, Some(EntityName::new("a")));
        t.set_face(1, Some(EntityName::new("b")));
        t.set_face(2, Some(EntityName::new("b")));
        assert_eq!(t.faces.len(), 4);
        assert_eq!(t.faces_named("b"), [1, 2]);
        assert_eq!(t.drop_repeated(), ["b"]);
        assert_eq!(t.faces_named("b"), Vec::<u32>::new());
        assert_eq!(t.face(3).map(|n| n.name.as_str()), Some("a"));
        assert!(t.face(9).is_none());
        let mut d = EntityName::new("a|b");
        d.derived = true;
        t.set_edge(0, Some(d.clone()));
        t.set_edge(1, Some(d));
        assert!(t.drop_repeated().is_empty());
        assert_eq!(t.edges_named("a|b"), [0, 1]);
    }
}
