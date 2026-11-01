// SPDX-License-Identifier: MIT OR Apache-2.0

//! The browser (S 08 §8.2, §8.6): Setups → toolpaths with status icons, and
//! the model's bodies.

use super::Selection;
use crate::jobs::{JobKind, Running};
use crate::session::{Snapshot, ToolpathStatus};
use crate::text::{data, tr, tr_args};

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
                detail: data(st.kind.as_str()),
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
        TreeVm {
            setups,
            bodies,
            empty: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::CalcTargets;
    use crate::session::tests::opened;

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
