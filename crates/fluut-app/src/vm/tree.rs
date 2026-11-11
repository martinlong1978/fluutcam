// SPDX-License-Identifier: MIT OR Apache-2.0

//! The browser (S 08 §8.2, §8.6): Setups → toolpaths with status icons, and
//! the model's bodies.

use super::Selection;
use super::tool_libs::{self, ToolLibRow};
use crate::jobs::{JobKind, Running};
use crate::session::{Snapshot, ToolpathStatus};
use crate::text::{data, tr, tr_args};
use fluut_model::PValue;
use fluut_model::edit::{Edit, apply};
use std::path::Path;

/// Status icon of a toolpath row (S 08 §8.6). Each has its own shape, so
/// colour is not the only signal.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum RowStatus {
    NotCalculated,
    /// Percent of the running job.
    Calculating(u8),
    Done,
    Stale,
    Warning,
    Error,
    Disabled,
}

impl RowStatus {
    pub fn text(self) -> String {
        match self {
            RowStatus::NotCalculated => tr("status-not-calculated"),
            RowStatus::Calculating(p) => {
                tr_args("status-calculating", &[("percent", &p.to_string())])
            }
            RowStatus::Done => tr("status-done"),
            RowStatus::Stale => tr("status-stale"),
            RowStatus::Warning => tr("status-warning"),
            RowStatus::Error => tr("status-error"),
            RowStatus::Disabled => tr("status-disabled"),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct ToolpathRow {
    pub id: String,
    pub label: String,
    /// Toolpath type.
    pub detail: String,
    pub status: RowStatus,
    pub status_text: String,
    pub selected: bool,
    pub enabled: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct SetupRow {
    pub id: String,
    pub label: String,
    /// Setup kind.
    pub detail: String,
    pub selected: bool,
    pub toolpaths: Vec<ToolpathRow>,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct TreeVm {
    pub setups: Vec<SetupRow>,
    /// Body labels under "Model".
    pub bodies: Vec<String>,
    /// The tool libraries the project refers to (D06 §7.2.1).
    pub tool_libraries: Vec<ToolLibRow>,
    /// Shown instead of the tree when no project is open.
    pub empty: Option<String>,
}

impl TreeVm {
    pub fn new(snap: Option<&Snapshot>, running: Option<&Running>, sel: &Selection) -> TreeVm {
        let Some(s) = snap else {
            return TreeVm {
                empty: Some(tr("browser-empty")),
                ..TreeVm::default()
            };
        };
        let calculating = running.and_then(|r| {
            let calc = matches!(r.kind, JobKind::Calculate | JobKind::Post);
            let id = r.toolpath.clone().filter(|_| calc)?;
            let pct = (r.cx.progress.fraction() * 100.0).round().clamp(0.0, 100.0) as u8;
            Some((id, pct))
        });
        let setups = s
            .project
            .setups
            .iter()
            .map(|st| SetupRow {
                id: st.id.clone(),
                label: data(&st.name),
                detail: data(st.kind.map_or("", |k| k.as_str())),
                selected: *sel == Selection::Setup(st.id.clone()),
                toolpaths: st
                    .toolpaths
                    .iter()
                    .map(|t| {
                        let status = match &calculating {
                            Some((id, p)) if *id == t.id => RowStatus::Calculating(*p),
                            _ => match s.status(&t.id) {
                                ToolpathStatus::NotCalculated => RowStatus::NotCalculated,
                                ToolpathStatus::Done => RowStatus::Done,
                                ToolpathStatus::Warning => RowStatus::Warning,
                                ToolpathStatus::Stale => RowStatus::Stale,
                                ToolpathStatus::Error => RowStatus::Error,
                                ToolpathStatus::Disabled => RowStatus::Disabled,
                            },
                        };
                        ToolpathRow {
                            id: t.id.clone(),
                            label: data(&t.name),
                            detail: data(t.kind().as_str()),
                            status,
                            status_text: status.text(),
                            selected: *sel == Selection::Toolpath(t.id.clone()),
                            enabled: t.is_enabled(),
                        }
                    })
                    .collect(),
            })
            .collect();
        let bodies = s
            .project
            .geometry
            .iter()
            .flat_map(|g| g.bodies.iter())
            .map(|b| match &b.name {
                Some(n) if !n.is_empty() => data(n),
                _ => tr_args("browser-body", &[("id", &b.id)]),
            })
            .collect();
        let dir = s.path.parent().map(Path::to_path_buf).unwrap_or_default();
        TreeVm {
            setups,
            bodies,
            tool_libraries: tool_libs::rows(&s.project, &dir, &s.warnings),
            empty: None,
        }
    }
}

/// The edit that renames a row. A blank name is refused.
pub fn rename_edit(item: &Selection, name: &str) -> Option<Edit> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let text = PValue::Enum(name.to_owned()).to_toml();
    match item {
        Selection::Setup(id) => Some(Edit::set_setup_field(id, "name", Some(&text))),
        Selection::Toolpath(id) => Some(Edit::set_toolpath_field(id, "name", Some(&text))),
        _ => None,
    }
}

/// The name a rename starts from.
pub fn name_of(s: &Snapshot, item: &Selection) -> Option<String> {
    match item {
        Selection::Setup(id) => s.project.setup(id).map(|x| x.name.clone()),
        Selection::Toolpath(id) => s.project.toolpath(id).map(|(_, t)| t.name.clone()),
        _ => None,
    }
}

/// The edit that removes a row.
pub fn delete_edit(item: &Selection) -> Option<Edit> {
    match item {
        Selection::Setup(id) => Some(Edit::RemoveSetup(id.clone())),
        Selection::Toolpath(id) => Some(Edit::RemoveToolpath(id.clone())),
        _ => None,
    }
}

/// The edit that turns a toolpath on or off. Enabled is the default, so on
/// clears the key.
pub fn enable_edit(toolpath: &str, on: bool) -> Edit {
    Edit::set_toolpath_field(toolpath, "enabled", (!on).then_some("false"))
}

/// Keeps an edit the model takes: a move that would break a `based_on`
/// chain has none (spec 03 §3.9.4).
fn allowed(s: &Snapshot, e: Edit) -> Option<Edit> {
    apply(&s.project, &e).ok().map(|_| e)
}

fn move_setup(s: &Snapshot, id: &str, to: usize) -> Option<Edit> {
    let from = s.project.setup_index(id)?;
    (from != to).then_some(())?;
    allowed(
        s,
        Edit::MoveSetup {
            id: id.to_owned(),
            to: to as u32,
        },
    )
}

fn move_toolpath(s: &Snapshot, id: &str, setup: &str, to: usize) -> Option<Edit> {
    let (from, _) = s.project.toolpath(id)?;
    if from.id == setup && from.toolpaths.iter().position(|t| t.id == id) == Some(to) {
        return None;
    }
    allowed(
        s,
        Edit::MoveToolpath {
            id: id.to_owned(),
            setup: setup.to_owned(),
            to: to as u32,
        },
    )
}

/// The edit that moves a row up (`up`) or down within its list, if it can.
pub fn step_edit(s: &Snapshot, item: &Selection, up: bool) -> Option<Edit> {
    let shift = |i: usize, n: usize| {
        if up {
            i.checked_sub(1)
        } else {
            Some(i + 1).filter(|j| *j < n)
        }
    };
    match item {
        Selection::Setup(id) => {
            let i = s.project.setup_index(id)?;
            move_setup(s, id, shift(i, s.project.setups.len())?)
        }
        Selection::Toolpath(id) => {
            let (st, _) = s.project.toolpath(id)?;
            let i = st.toolpaths.iter().position(|t| t.id == *id)?;
            move_toolpath(s, id, &st.id, shift(i, st.toolpaths.len())?)
        }
        _ => None,
    }
}

/// The edit for dropping row `item` on row `on`: a toolpath takes the place
/// of the toolpath it lands on, or goes last in the Setup it lands on; a
/// Setup takes the place of the Setup it lands on.
pub fn drop_edit(s: &Snapshot, item: &Selection, on: &Selection) -> Option<Edit> {
    match (item, on) {
        (Selection::Setup(a), Selection::Setup(b)) => move_setup(s, a, s.project.setup_index(b)?),
        (Selection::Toolpath(a), Selection::Toolpath(b)) => {
            let (st, _) = s.project.toolpath(b)?;
            let to = st.toolpaths.iter().position(|t| t.id == *b)?;
            move_toolpath(s, a, &st.id, to)
        }
        (Selection::Toolpath(a), Selection::Setup(b)) => {
            let st = s.project.setup(b)?;
            let same = st.toolpaths.iter().any(|t| t.id == *a);
            move_toolpath(s, a, b, st.toolpaths.len() - usize::from(same))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::CalcTargets;
    use crate::session::tests::opened;

    fn run(j: &mut crate::jobs::Jobs, e: Edit) -> std::sync::Arc<Snapshot> {
        j.edit(crate::jobs::Edit::Model(e));
        j.wait_snapshot().unwrap()
    }

    fn order(s: &Snapshot) -> Vec<String> {
        s.project.setups[0]
            .toolpaths
            .iter()
            .map(|t| t.id.clone())
            .collect()
    }

    #[test]
    fn rename_refuses_a_blank_name() {
        let mut j = opened("block");
        let t = Selection::Toolpath("t1".into());
        assert_eq!(rename_edit(&t, "   "), None);
        let s = run(&mut j, rename_edit(&t, " Roughing ").unwrap());
        assert_eq!(s.project.toolpath("t1").unwrap().1.name, "Roughing");
        let st = Selection::Setup("s1".into());
        let s = run(&mut j, rename_edit(&st, "First \"op\"").unwrap());
        assert_eq!(s.project.setup("s1").unwrap().name, "First \"op\"");
        assert_eq!(name_of(&s, &st).as_deref(), Some("First \"op\""));
    }

    #[test]
    fn steps_stay_in_the_list_and_undo() {
        let mut j = opened("block");
        let s = j.snapshot.clone().unwrap();
        let t1 = Selection::Toolpath("t1".into());
        let t3 = Selection::Toolpath("t3".into());
        assert_eq!(step_edit(&s, &t1, true), None);
        assert_eq!(step_edit(&s, &t3, false), None);
        let s = run(&mut j, step_edit(&s, &t1, false).unwrap());
        assert_eq!(order(&s), ["t2", "t1", "t3"]);
        let s = run(&mut j, step_edit(&s, &t3, true).unwrap());
        assert_eq!(order(&s), ["t2", "t3", "t1"]);
        j.undo();
        j.wait();
        assert_eq!(order(j.snapshot.as_ref().unwrap()), ["t2", "t1", "t3"]);
    }

    #[test]
    fn drops_move_rows_and_respect_based_on() {
        let mut j = opened("block");
        j.add_setup(fluut_model::project::SetupKind::ThreeAxis, Some("s1"));
        j.wait();
        let s = j.snapshot.clone().unwrap();
        let child = s.project.setups[1].id.clone();
        let (a, b) = (
            Selection::Setup("s1".into()),
            Selection::Setup(child.clone()),
        );
        // A parent must stay before its child, either way round.
        assert_eq!(drop_edit(&s, &a, &b), None);
        assert_eq!(drop_edit(&s, &b, &a), None);
        assert_eq!(step_edit(&s, &a, false), None);
        // A toolpath dropped on a toolpath takes its place.
        let t1 = Selection::Toolpath("t1".into());
        let t3 = Selection::Toolpath("t3".into());
        assert_eq!(drop_edit(&s, &t1, &t1), None);
        let s = run(&mut j, drop_edit(&s, &t3, &t1).unwrap());
        assert_eq!(order(&s), ["t3", "t1", "t2"]);
        // On a Setup row it goes last: there, and here.
        let s = run(&mut j, drop_edit(&s, &t3, &a).unwrap());
        assert_eq!(order(&s), ["t1", "t2", "t3"]);
        let s = run(&mut j, drop_edit(&s, &t1, &b).unwrap());
        assert_eq!(order(&s), ["t2", "t3"]);
        assert_eq!(s.project.setup(&child).unwrap().toolpaths[0].id, "t1");
    }

    #[test]
    fn delete_and_enable_undo() {
        let mut j = opened("block");
        let t = Selection::Toolpath("t2".into());
        let s = run(&mut j, enable_edit("t2", false));
        let row = |s: &Snapshot| {
            TreeVm::new(Some(s), None, &Selection::None).setups[0].toolpaths[1].clone()
        };
        assert!(!row(&s).enabled);
        assert_eq!(row(&s).status, RowStatus::Disabled);
        let s = run(&mut j, enable_edit("t2", true));
        assert!(row(&s).enabled);
        assert_eq!(s.project.toolpath("t2").unwrap().1.enabled, None);
        let s = run(&mut j, delete_edit(&t).unwrap());
        assert_eq!(order(&s), ["t1", "t3"]);
        j.undo();
        j.wait();
        assert_eq!(order(j.snapshot.as_ref().unwrap()), ["t1", "t2", "t3"]);
        // A parent cannot go while a child names it: the refusal is the edit's.
        assert_eq!(delete_edit(&Selection::None), None);
    }

    #[test]
    fn no_project() {
        let t = TreeVm::new(None, None, &Selection::None);
        assert!(t.setups.is_empty());
        assert_eq!(
            t.empty.as_deref(),
            Some("Open a project to see its Setups and toolpaths.")
        );
    }

    #[test]
    fn block_before_and_after_calculating() {
        let mut j = opened("block");
        let sel = Selection::Toolpath("t2".into());
        let t = TreeVm::new(j.snapshot.as_deref(), None, &sel);
        assert_eq!(t.setups.len(), 1);
        let s = &t.setups[0];
        assert_eq!((s.label.as_str(), s.detail.as_str()), ("Top", "three_axis"));
        let names: Vec<_> = s.toolpaths.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(names, ["Pocket", "Pocket floor raster", "Cut out"]);
        assert!(
            s.toolpaths
                .iter()
                .all(|r| r.status == RowStatus::NotCalculated)
        );
        assert!(s.toolpaths[1].selected && !s.toolpaths[0].selected);
        assert_eq!(t.bodies, ["pocket_block"]);

        j.calculate(CalcTargets::All);
        // While running, the current toolpath shows a progress ring.
        while j.running.as_ref().is_some_and(|r| r.toolpath.is_none()) {
            assert!(j.wait_event());
        }
        let t = TreeVm::new(j.snapshot.as_deref(), j.running.as_ref(), &sel);
        assert!(
            t.setups[0]
                .toolpaths
                .iter()
                .any(|r| matches!(r.status, RowStatus::Calculating(_))),
            "{t:?}"
        );
        j.wait();
        let t = TreeVm::new(j.snapshot.as_deref(), None, &sel);
        let st: Vec<RowStatus> = t.setups[0].toolpaths.iter().map(|r| r.status).collect();
        // The cut-out goes below the stock: a warning (as in the CLI test).
        assert_eq!(st, [RowStatus::Done, RowStatus::Done, RowStatus::Warning]);
        assert_eq!(
            t.setups[0].toolpaths[2].status_text,
            "Calculated with warnings"
        );
    }
}
