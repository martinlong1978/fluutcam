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
use crate::vm::setup::setup_pick_edit;
use fluut_model::project::{AxisPick, Orientation, StockSpec};
use fluut_model::setup::resolve_setup;
use fluut_model::{PValue, Params, Sel, ToolpathTypeId};

/// What a selection field accepts.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PickFilter {
    Faces,
    /// Cylindrical faces (hole pickers).
    Cylinders,
    /// Edges, wire curves and planar faces (their loops).
    Curves,
    /// Whole bodies: any face or curve stands for its body.
    Bodies,
    /// A direction: planar and cylindrical faces, and edges.
    Directions,
}

/// What a Setup pick is for (spec 03 §3.9): its bodies, the stock body or
/// one of the orientation directions.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SetupPick {
    Bodies,
    StockBody,
    ToolAxis,
    XAxis,
    RotaryAxis,
    AZero,
}

impl SetupPick {
    /// The message id of the field the pick fills in.
    pub fn label_id(self) -> &'static str {
        match self {
            SetupPick::Bodies => "setup-bodies",
            SetupPick::StockBody => "stock-body",
            SetupPick::ToolAxis => "orientation-tool-axis",
            SetupPick::XAxis => "orientation-x-axis",
            SetupPick::RotaryAxis => "orientation-rotary-axis",
            SetupPick::AZero => "orientation-a-zero",
        }
    }

    /// Whole bodies are picked on the model scene, which shows them all.
    pub fn shows_model(self) -> bool {
        matches!(self, SetupPick::Bodies | SetupPick::StockBody)
    }

    /// A pick that fills one value ends after the first hit.
    fn single(self) -> bool {
        self != SetupPick::Bodies
    }

    fn filter(self) -> PickFilter {
        if self.shows_model() {
            PickFilter::Bodies
        } else {
            PickFilter::Directions
        }
    }
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
        matches!(
            self,
            PickFilter::Curves | PickFilter::Bodies | PickFilter::Directions
        )
    }

    fn takes_face(self, kind: FaceKind) -> bool {
        match self {
            PickFilter::Faces | PickFilter::Bodies => true,
            PickFilter::Cylinders => kind == FaceKind::Cylinder,
            PickFilter::Curves => kind == FaceKind::Plane,
            PickFilter::Directions => matches!(kind, FaceKind::Plane | FaceKind::Cylinder),
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
    /// For a Setup pick: the Setup and what is picked. `toolpath` is empty.
    pub setup: Option<(String, SetupPick)>,
    /// A single-value pick has taken its value: leave the mode.
    pub done: bool,
}

impl PickMode {
    pub fn new(kind: ToolpathTypeId, toolpath: &str, path: &str) -> PickMode {
        PickMode {
            toolpath: toolpath.to_owned(),
            path: path.to_owned(),
            filter: PickFilter::of(kind, path),
            hover: None,
            offer: Vec::new(),
            setup: None,
            done: false,
        }
    }

    /// A pick for a field of Setup `setup`.
    pub fn for_setup(setup: &str, role: SetupPick) -> PickMode {
        PickMode {
            toolpath: String::new(),
            path: role.label_id().to_owned(),
            filter: role.filter(),
            hover: None,
            offer: Vec::new(),
            setup: Some((setup.to_owned(), role)),
            done: false,
        }
    }

    /// What is picked now: a toolpath field's items, or the Setup's bodies,
    /// stock body or direction.
    pub fn current(&self, s: &Snapshot) -> Vec<Sel> {
        let Some((setup, role)) = &self.setup else {
            return selected(s, &self.toolpath, &self.path);
        };
        let Ok(r) = resolve_setup(&s.project, setup) else {
            return Vec::new();
        };
        let of = |p: &AxisPick| match p {
            AxisPick::Face(id) => vec![Sel::Face(id.clone())],
            AxisPick::Edge(id) => vec![Sel::Edge(id.clone())],
            AxisPick::Axis(_) => Vec::new(),
        };
        match (role, &r.orientation, &r.stock) {
            (SetupPick::Bodies, ..) => r.geometry.bodies.map_or_else(
                || {
                    s.project
                        .geometry
                        .iter()
                        .flat_map(|g| g.bodies.iter().map(|b| Sel::Body(b.id.clone())))
                        .collect()
                },
                |b| b.into_iter().map(Sel::Body).collect(),
            ),
            (SetupPick::StockBody, _, Some(StockSpec::Body { body })) => {
                vec![Sel::Body(body.clone())]
            }
            (SetupPick::ToolAxis, Orientation::ThreeAxis { tool_axis: p, .. }, _)
            | (SetupPick::XAxis, Orientation::ThreeAxis { x_axis: p, .. }, _)
            | (SetupPick::RotaryAxis, Orientation::Rotary { rotary_axis: p, .. }, _) => of(p),
            (
                SetupPick::AZero,
                Orientation::Rotary {
                    a_zero_toward: Some(p),
                    ..
                },
                _,
            ) => of(p),
            _ => Vec::new(),
        }
    }

    /// The scene the pick happens in: the Setup of the toolpath.
    fn scene<'a>(&self, s: &'a Snapshot) -> Option<&'a SetupScene> {
        if let Some((setup, role)) = &self.setup {
            return if role.shows_model() {
                s.all_bodies.as_deref()
            } else {
                s.scenes.get(setup).map(|a| &**a)
            };
        }
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

    /// Whether a click on the model chooses whole bodies.
    pub fn picks_bodies(&self) -> bool {
        self.filter == PickFilter::Bodies
    }

    /// A click on `sel`: the edit that toggles it, if the field takes it.
    /// An edge just added offers its closed loop.
    pub fn hit(&mut self, s: &Snapshot, sel: &Sel) -> Option<Edit> {
        if !self.accepts(s, sel) {
            return None;
        }
        if let Some((setup, role)) = &self.setup {
            let e = setup_pick_edit(s, setup, *role, sel).ok()?;
            self.done = role.single();
            return Some(Edit::Model(e));
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
        if self.offer.is_empty() || self.setup.is_some() {
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
        for x in p.current(s) {
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
