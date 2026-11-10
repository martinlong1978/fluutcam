// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use crate::jobs::Jobs;
use crate::scene::FaceKind;
use crate::session::tests::opened;
use ToolpathTypeId as T;
use std::sync::Arc;

/// `plate2d` with a new toolpath of `kind`.
fn with_toolpath(kind: T) -> (Jobs, String) {
    let mut j = opened("plate2d");
    j.add_toolpath("s1", kind);
    j.wait();
    let s = j.snapshot.clone().unwrap();
    let id = s.project.setups[0].toolpaths.last().unwrap().id.clone();
    (j, id)
}

fn run(j: &mut Jobs, e: Edit) -> Arc<Snapshot> {
    j.edit(e);
    j.wait_snapshot().unwrap()
}

#[test]
fn filters_follow_the_field() {
    let f = PickFilter::of;
    assert_eq!(f(T::Pocket2d, "geometry.drive_faces"), PickFilter::Faces);
    assert_eq!(f(T::Parallel, "geometry.check_faces"), PickFilter::Faces);
    assert_eq!(f(T::Drill, "geometry.drive_faces"), PickFilter::Cylinders);
    assert_eq!(f(T::Bore, "geometry.drive_faces"), PickFilter::Cylinders);
    assert_eq!(f(T::Drill, "geometry.drive_curves"), PickFilter::Curves);
    assert_eq!(
        f(T::Parallel, "geometry.boundary.curves"),
        PickFilter::Curves
    );
}

#[test]
fn a_click_toggles_and_each_toggle_undoes() {
    let (mut j, id) = with_toolpath(T::Pocket2d);
    let mut m = PickMode::new(T::Pocket2d, &id, "geometry.drive_faces");
    let f = Sel::Face("f-588.14".into());
    let s = j.snapshot.clone().unwrap();
    let e = m.hit(&s, &f).unwrap();
    let s = run(&mut j, e);
    assert_eq!(
        selected(&s, &id, "geometry.drive_faces"),
        std::slice::from_ref(&f)
    );
    let e = m.hit(&s, &Sel::Face("f-588.0".into())).unwrap();
    let s = run(&mut j, e);
    assert_eq!(selected(&s, &id, "geometry.drive_faces").len(), 2);
    let e = m.hit(&s, &f).unwrap();
    let s = run(&mut j, e);
    assert_eq!(
        selected(&s, &id, "geometry.drive_faces"),
        [Sel::Face("f-588.0".into())]
    );
    j.undo();
    j.wait();
    let s = j.snapshot.clone().unwrap();
    assert_eq!(selected(&s, &id, "geometry.drive_faces").len(), 2);
    // What is not in the scene or the field is refused.
    assert!(m.hit(&s, &Sel::Face("f-999.0".into())).is_none());
    assert!(m.hit(&s, &Sel::Edge("e-588.0".into())).is_none());
    assert!(m.hit(&s, &Sel::Body("b-588".into())).is_none());
}

#[test]
fn clear_and_remove() {
    let (mut j, id) = with_toolpath(T::Pocket2d);
    let path = "geometry.drive_faces";
    let mut m = PickMode::new(T::Pocket2d, &id, path);
    let (a, b) = (Sel::Face("f-588.0".into()), Sel::Face("f-588.1".into()));
    for x in [&a, &b] {
        let s = j.snapshot.clone().unwrap();
        let e = m.hit(&s, x).unwrap();
        run(&mut j, e);
    }
    let s = j.snapshot.clone().unwrap();
    let s = run(&mut j, remove_edit(&s, &id, path, &a));
    assert_eq!(selected(&s, &id, path), [b]);
    let s = run(&mut j, clear_edit(&id, path));
    assert!(selected(&s, &id, path).is_empty());
    assert!(!s.project.toolpath(&id).unwrap().1.params.is_set(path));
}

#[test]
fn hole_pickers_take_only_cylinders() {
    let j = opened("holes");
    let s = j.snapshot.clone().unwrap();
    let st = &s.project.setups[0];
    let id = st.toolpaths[0].id.clone();
    let scene = &s.scenes[&st.id];
    let of = |k: FaceKind| {
        let i = scene.face_kinds.iter().position(|x| *x == k).unwrap();
        Sel::Face(scene.faces[i].clone())
    };
    let m = PickMode::new(T::Drill, &id, "geometry.drive_faces");
    assert!(!m.accepts(&s, &of(FaceKind::Plane)));
    assert_eq!(scene.face_kinds.len(), scene.faces.len());
    assert!(m.accepts(&s, &of(FaceKind::Cylinder)));
}

#[test]
fn curve_fields_take_edges_and_planar_faces_and_offer_the_closed_loop() {
    let (mut j, id) = with_toolpath(T::Contour2d);
    let path = "geometry.drive_curves";
    let mut m = PickMode::new(T::Contour2d, &id, path);
    let s = j.snapshot.clone().unwrap();
    let scene = s.scenes["s1"].clone();
    // A top or floor loop comes first; the pocket rim has 8 edges.
    assert!(!scene.edge_loops.is_empty());
    let e16 = Sel::Edge("e-588.16".into());
    assert!(m.accepts(&s, &e16));
    assert!(m.accepts(&s, &Sel::Face("f-588.14".into())));
    let e = m.hit(&s, &e16).unwrap();
    let s = run(&mut j, e);
    assert!(!m.offer.is_empty());
    let n = m.offer.len() + 1;
    let e = m.accept_offer(&s).unwrap();
    let s = run(&mut j, e);
    let items = selected(&s, &id, path);
    assert_eq!(items.len(), n);
    assert_eq!(items[0], e16);
    assert!(m.offer.is_empty());
    assert!(m.accept_offer(&s).is_none());
    // Removing an edge offers nothing.
    let e = m.hit(&s, &e16).unwrap();
    run(&mut j, e);
    assert!(m.offer.is_empty());
}

#[test]
fn wire_curves_chain_into_closed_loops() {
    let j = opened("plate");
    let s = j.snapshot.clone().unwrap();
    let scene = s.scenes.values().next().unwrap();
    assert!(!scene.edge_ids.is_empty(), "wire curves are pickable");
    assert_eq!(scene.edges.ranges.len(), scene.edge_ids.len());
    assert!(scene.faces.is_empty());
    assert!(!scene.edge_loops.is_empty());
}

#[test]
fn highlight_shows_the_picked_field_or_the_whole_toolpath() {
    let (mut j, id) = with_toolpath(T::Pocket2d);
    let path = "geometry.drive_faces";
    let mut m = PickMode::new(T::Pocket2d, &id, path);
    let s = j.snapshot.clone().unwrap();
    let e = m.hit(&s, &Sel::Face("f-588.14".into())).unwrap();
    let s = run(&mut j, e);
    let scene = s.scenes["s1"].clone();
    let at = scene.faces.iter().position(|f| f == "f-588.14").unwrap() as u32;
    let sel = Selection::Toolpath(id.clone());
    assert_eq!(highlight(&s, &scene, &sel, None).faces, [at]);
    m.hover = Some(Sel::Face("f-588.0".into()));
    let h = highlight(&s, &scene, &sel, Some(&m));
    assert_eq!(h.faces, [at]);
    assert!(h.hover_face.is_some());
    // Another field being picked shows that field alone.
    let other = PickMode::new(T::Pocket2d, &id, "geometry.excluded_faces");
    assert!(highlight(&s, &scene, &sel, Some(&other)).faces.is_empty());
    assert!(
        highlight(&s, &scene, &Selection::Setup("s1".into()), None)
            .faces
            .is_empty()
    );
}
