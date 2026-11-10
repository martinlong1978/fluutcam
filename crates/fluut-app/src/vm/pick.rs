// SPDX-License-Identifier: MIT OR Apache-2.0

//! Picking selections on the model (S 08 §8.4, D15 §2.3): which entities a
//! field takes, what a click does to its selection, and what the viewport
//! highlights. Selections hold the persistent ids of spec 03 §3.5, never
//! pick indices, so re-import keeps them.

use crate::jobs::Edit;
use crate::scene::{FaceKind, SetupScene};
use crate::session::Snapshot;
use crate::vm::Selection;
use crate::vm::inspector::value_edit;
use fluut_model::{PValue, Params, Sel, ToolpathTypeId};

/// What a selection field accepts.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PickFilter {
    Faces,
    /// Cylindrical faces (hole pickers).
    Cylinders,
    /// Edges, wire curves and planar faces (their loops).
    Curves,
}

impl PickFilter {
    /// The filter of field `path` of a toolpath of type `kind`.
    pub fn of(kind: ToolpathTypeId, path: &str) -> PickFilter {
        match path {
            "geometry.drive_curves" | "geometry.boundary.curves" => PickFilter::Curves,
            "geometry.drive_faces"
                if matches!(kind, ToolpathTypeId::Drill | ToolpathTypeId::Bore) =>
            {
                PickFilter::Cylinders
            }
            _ => PickFilter::Faces,
        }
    }

    pub fn wants_edges(self) -> bool {
        self == PickFilter::Curves
    }

    fn takes_face(self, kind: FaceKind) -> bool {
        match self {
            PickFilter::Faces => true,
            PickFilter::Cylinders => kind == FaceKind::Cylinder,
            PickFilter::Curves => kind == FaceKind::Plane,
        }
    }
}

/// Pick mode: the field being picked for, in `UiState`.
#[derive(Clone, PartialEq, Debug)]
pub struct PickMode {
    pub toolpath: String,
    pub path: String,
    pub filter: PickFilter,
    /// What is under the pointer and acceptable.
    pub hover: Option<Sel>,
    /// The closed loop of the last edge picked, when it is not all selected.
    pub offer: Vec<Sel>,
}

impl PickMode {
    pub fn new(kind: ToolpathTypeId, toolpath: &str, path: &str) -> PickMode {
        PickMode {
            toolpath: toolpath.to_owned(),
            path: path.to_owned(),
            filter: PickFilter::of(kind, path),
            hover: None,
            offer: Vec::new(),
        }
    }

    /// The scene the pick happens in: the Setup of the toolpath.
    fn scene<'a>(&self, s: &'a Snapshot) -> Option<&'a SetupScene> {
        let (setup, _) = s.project.toolpath(&self.toolpath)?;
        s.scenes.get(&setup.id).map(|a| &**a)
    }

    /// Whether the entity may go in this field.
    pub fn accepts(&self, s: &Snapshot, sel: &Sel) -> bool {
        let Some(scene) = self.scene(s) else {
            return false;
        };
        match sel {
            Sel::Face(id) => scene
                .faces
                .iter()
                .position(|f| f == id)
                .is_some_and(|i| self.filter.takes_face(scene.face_kinds[i])),
            Sel::Edge(id) => self.filter.wants_edges() && scene.edge_ids.contains(id),
            Sel::Body(_) => false,
        }
    }

    /// A click on `sel`: the edit that toggles it, if the field takes it.
    /// An edge just added offers its closed loop.
    pub fn hit(&mut self, s: &Snapshot, sel: &Sel) -> Option<Edit> {
        if !self.accepts(s, sel) {
            return None;
        }
        let mut items = selected(s, &self.toolpath, &self.path);
        let added = !items.contains(sel);
        if added {
            items.push(sel.clone());
        } else {
            items.retain(|x| x != sel);
        }
        self.offer = match (sel, added, self.scene(s)) {
            (Sel::Edge(id), true, Some(scene)) => loop_of(scene, id)
                .into_iter()
                .filter(|e| !items.contains(e))
                .collect(),
            _ => Vec::new(),
        };
        Some(with_items(&self.toolpath, &self.path, items))
    }

    /// Takes up the offer: the edit that adds the rest of the loop.
    pub fn accept_offer(&mut self, s: &Snapshot) -> Option<Edit> {
        if self.offer.is_empty() {
            return None;
        }
        let mut items = selected(s, &self.toolpath, &self.path);
        for e in std::mem::take(&mut self.offer) {
            if !items.contains(&e) {
                items.push(e);
            }
        }
        Some(with_items(&self.toolpath, &self.path, items))
    }
}

/// The first closed loop through the edge, as selections: horizontal face
/// loops come first in `SetupScene::edge_loops`.
fn loop_of(scene: &SetupScene, id: &str) -> Vec<Sel> {
    let Some(at) = scene.edge_ids.iter().position(|e| e == id) else {
        return Vec::new();
    };
    scene
        .edge_loops
        .iter()
        .find(|l| l.contains(&(at as u32)))
        .map_or_else(Vec::new, |l| {
            l.iter()
                .map(|i| Sel::Edge(scene.edge_ids[*i as usize].clone()))
                .collect()
        })
}

/// The selection held by field `path`.
pub fn selected(s: &Snapshot, toolpath: &str, path: &str) -> Vec<Sel> {
    match s
        .project
        .toolpath(toolpath)
        .map(|(_, t)| t.params.get(path))
    {
        Some(Some(PValue::Selection(v))) => v.clone(),
        _ => Vec::new(),
    }
}

fn with_items(toolpath: &str, path: &str, items: Vec<Sel>) -> Edit {
    let v = (!items.is_empty()).then_some(PValue::Selection(items));
    value_edit(toolpath, path, v)
}

/// The edit that removes one item.
pub fn remove_edit(s: &Snapshot, toolpath: &str, path: &str, sel: &Sel) -> Edit {
    let mut items = selected(s, toolpath, path);
    items.retain(|x| x != sel);
    with_items(toolpath, path, items)
}

/// The edit that empties a selection.
pub fn clear_edit(toolpath: &str, path: &str) -> Edit {
    value_edit(toolpath, path, None)
}

/// What the viewport tints: pick indices of faces and edges.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Highlight {
    pub faces: Vec<u32>,
    pub edges: Vec<u32>,
    pub hover_face: Option<u32>,
    pub hover_edge: Option<u32>,
}

fn indices(scene: &SetupScene, sel: &Sel, h: &mut Highlight) {
    match sel {
        Sel::Face(id) => h
            .faces
            .extend(scene.faces.iter().position(|f| f == id).map(|i| i as u32)),
        Sel::Edge(id) => h.edges.extend(
            scene
                .edge_ids
                .iter()
                .position(|e| e == id)
                .map(|i| i as u32),
        ),
        Sel::Body(id) => {
            let stem = id.strip_prefix("b-").unwrap_or(id);
            let (f, e) = (format!("f-{stem}."), format!("e-{stem}."));
            for (i, x) in scene.faces.iter().enumerate() {
                if x.starts_with(&f) {
                    h.faces.push(i as u32);
                }
            }
            for (i, x) in scene.edge_ids.iter().enumerate() {
                if x.starts_with(&e) {
                    h.edges.push(i as u32);
                }
            }
        }
    }
}

/// The highlight of a frame: in pick mode the field being picked, else
/// every selection of the selected toolpath.
pub fn highlight(
    s: &Snapshot,
    scene: &SetupScene,
    sel: &Selection,
    pick: Option<&PickMode>,
) -> Highlight {
    let mut h = Highlight::default();
    if let Some(p) = pick {
        for x in selected(s, &p.toolpath, &p.path) {
            indices(scene, &x, &mut h);
        }
        let mut hover = Highlight::default();
        if let Some(x) = &p.hover {
            indices(scene, x, &mut hover);
        }
        h.hover_face = hover.faces.first().copied();
        h.hover_edge = hover.edges.first().copied();
    } else if let Selection::Toolpath(id) = sel
        && let Some((_, t)) = s.project.toolpath(id)
    {
        for (path, _) in Params::leaf_fields(t.kind()) {
            if let Some(PValue::Selection(items)) = t.params.get(&path) {
                for x in items {
                    indices(scene, x, &mut h);
                }
            }
        }
    }
    h
}

#[cfg(test)]
mod tests;
