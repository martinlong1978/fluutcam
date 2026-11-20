// SPDX-License-Identifier: MIT OR Apache-2.0

//! Linked models in the window (spec 03 §3.5 item 4, S 08 §8.6): the link
//! in the Model panel, the update report, a broken toolpath's picks, and
//! what opening a `.craftx` file does.

use crate::jobs::LinkRead;
use crate::session::{LinkView, Snapshot};
use crate::text::{data, tr, tr_args};
use fluut_core::names::Outcome;
use fluut_core::reimport::{PickReport, ReimportReport};
use std::path::Path;
use std::time::SystemTime;

/// `face` or `edge` of a pick id, in words.
fn what(id: &str) -> String {
    if id.starts_with("e-") {
        tr("link-report-what-edge")
    } else {
        tr("link-report-what-face")
    }
}

/// The file name of a path as the project writes it.
fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| data(path), |n| data(n.to_string_lossy().into_owned()))
}

/// `yyyy-mm-dd hh:mm` (UTC) of a file time.
pub fn utc_minutes(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        rem / 60 % 60
    )
}

/// The short word of an outcome, as the report's first column shows it.
pub fn outcome_label(o: Outcome) -> String {
    tr(match o {
        Outcome::Exact => "link-report-outcome-exact",
        Outcome::Merged => "link-report-outcome-merged",
        Outcome::Split => "link-report-outcome-split",
        Outcome::ByGeometry => "link-report-outcome-by_geometry",
        Outcome::Missing => "link-report-outcome-missing",
    })
}

/// What an outcome means for the pick, in plain words.
pub fn outcome_text(p: &PickReport) -> String {
    let w = what(&p.id);
    let n = data(&p.name);
    let count = p.ids.len().to_string();
    let args: &[(&str, &str)] = &[("what", &w), ("name", &n), ("count", &count)];
    match p.outcome {
        Outcome::Exact => tr_args("link-report-text-exact", args),
        Outcome::Merged => tr_args("link-report-text-merged", args),
        Outcome::Split => tr_args("link-report-text-split", args),
        Outcome::ByGeometry => tr_args("link-report-text-by_geometry", args),
        Outcome::Missing => tr_args("link-report-text-missing", args),
    }
}

/// One line of the update report.
#[derive(Clone, PartialEq, Debug)]
pub struct ReportRow {
    pub outcome: Outcome,
    pub label: String,
    /// The toolpath, to select it.
    pub toolpath: String,
    pub toolpath_name: String,
    pub text: String,
}

/// One updated model in the report.
#[derive(Clone, PartialEq, Debug)]
pub struct ReportSource {
    pub heading: String,
    /// Notes: the file did not change, no named picks, unmatched picks.
    pub notes: Vec<String>,
    pub rows: Vec<ReportRow>,
}

/// The update report: every outcome of every named pick, in one window.
#[derive(Clone, PartialEq, Debug)]
pub struct ReportVm {
    pub title: String,
    pub sources: Vec<ReportSource>,
    pub summary: String,
    /// A pick is missing: some toolpath is broken.
    pub broken: bool,
}

impl ReportVm {
    pub fn new(reports: &[ReimportReport]) -> ReportVm {
        let sources = reports
            .iter()
            .map(|r| {
                let file = file_name(&r.path);
                let heading = match (&r.revision_before, &r.revision) {
                    (Some(a), Some(b)) if a != b => tr_args(
                        "link-report-source",
                        &[("file", &file), ("from", &data(a)), ("to", &data(b))],
                    ),
                    (_, Some(b)) => tr_args(
                        "link-report-source-same",
                        &[("file", &file), ("to", &data(b))],
                    ),
                    _ => file.clone(),
                };
                let mut notes = Vec::new();
                if r.unchanged {
                    notes.push(tr("link-report-unchanged"));
                }
                if r.picks.is_empty() {
                    notes.push(tr("link-report-none"));
                }
                notes.extend(r.unmatched.iter().map(|u| {
                    tr_args(
                        "link-report-unmatched",
                        &[
                            ("id", &data(&u.id)),
                            ("field", &data(&u.field)),
                            ("owner", &data(&u.owner)),
                        ],
                    )
                }));
                let rows = r
                    .picks
                    .iter()
                    .map(|p| ReportRow {
                        outcome: p.outcome,
                        label: outcome_label(p.outcome),
                        toolpath: p.owner.clone(),
                        toolpath_name: data(&p.owner_name),
                        text: outcome_text(p),
                    })
                    .collect();
                ReportSource {
                    heading,
                    notes,
                    rows,
                }
            })
            .collect();
        let picks: usize = reports.iter().map(|r| r.picks.len()).sum();
        let mut broken: Vec<&str> = reports.iter().flat_map(|r| r.broken()).collect();
        broken.sort_unstable();
        broken.dedup();
        ReportVm {
            title: tr("link-report-title"),
            sources,
            summary: tr_args(
                "link-report-summary",
                &[
                    ("picks", &picks.to_string()),
                    ("broken", &broken.len().to_string()),
                ],
            ),
            broken: !broken.is_empty(),
        }
    }
}

/// A label and value of the Model panel.
#[derive(Clone, PartialEq, Debug)]
pub struct LinkPanel {
    pub source: String,
    pub heading: String,
    /// Source app, revision, saved time, material, part number.
    pub lines: Vec<String>,
    /// Design parameters: name and value with its unit, read only.
    pub params: Vec<(String, String)>,
    /// The file has changed since the project took it.
    pub changed: bool,
}

impl LinkPanel {
    pub fn new(l: &LinkView, changed: bool) -> LinkPanel {
        let mut lines = Vec::new();
        lines.push(match (&l.app, &l.app_version) {
            (Some(a), Some(v)) => tr_args("link-from", &[("app", &data(format!("{a} {v}")))]),
            (Some(a), None) => tr_args("link-from", &[("app", &data(a))]),
            _ => tr("link-from-unknown"),
        });
        lines.push(tr_args(
            "link-revision",
            &[("revision", &data(&l.revision))],
        ));
        lines.push(l.saved.map_or_else(
            || tr("link-saved-unknown"),
            |t| tr_args("link-saved", &[("time", &utc_minutes(t))]),
        ));
        if let Some(m) = &l.material {
            lines.push(tr_args("link-material", &[("material", &data(m))]));
        }
        if let Some(n) = &l.part_number {
            lines.push(tr_args("link-part-number", &[("number", &data(n))]));
        }
        LinkPanel {
            source: l.source.clone(),
            heading: tr_args("link-heading", &[("file", &file_name(&l.path))]),
            lines,
            params: l
                .params
                .iter()
                .map(|(n, v, u)| {
                    let value = match u {
                        Some(u) => format!("{v} {u}"),
                        None => v.clone(),
                    };
                    (data(n), data(value))
                })
                .collect(),
            changed,
        }
    }
}

/// The Model panel's linked models; `changed` lists the sources whose file
/// changed.
pub fn panels(s: &Snapshot, changed: &[String]) -> Vec<LinkPanel> {
    s.links
        .iter()
        .map(|l| LinkPanel::new(l, changed.contains(&l.source)))
        .collect()
}

/// A missing pick of a broken toolpath, with how to pick it again.
#[derive(Clone, PartialEq, Debug)]
pub struct BrokenRow {
    pub field: String,
    pub id: String,
    pub text: String,
}

/// The broken picks of toolpath `id`, for the inspector.
pub fn broken_rows(s: &Snapshot, id: &str) -> Vec<BrokenRow> {
    let Some(list) = s.broken.get(id) else {
        return Vec::new();
    };
    let kind = s.project.toolpath(id).map(|(_, t)| t.kind());
    list.iter()
        .map(|b| {
            let field = kind
                .and_then(|k| fluut_model::Params::meta(k, &b.field))
                .map_or_else(|| data(&b.field), crate::text::field_label);
            BrokenRow {
                field: b.field.clone(),
                id: b.id.clone(),
                text: tr_args(
                    "broken-text",
                    &[
                        ("what", &what(&b.id)),
                        ("name", &data(&b.name)),
                        ("field", &field),
                    ],
                ),
            }
        })
        .collect()
}

/// What to do with a `.craftx` file that was read.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LinkPlan {
    /// The open project links its design: offer to update that source.
    OfferUpdate { source: String },
    /// Import it into the open project.
    Import,
    /// Start a new project for it.
    NewProject,
}

/// `launch`: the file came from the command line (opens in a new project);
/// otherwise from Import model.
pub fn plan(l: &LinkRead, has_project: bool, launch: bool) -> LinkPlan {
    match &l.linked {
        Some(source) => LinkPlan::OfferUpdate {
            source: source.clone(),
        },
        None if launch || !has_project => LinkPlan::NewProject,
        None => LinkPlan::Import,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn pick(outcome: Outcome, id: &str, ids: usize) -> PickReport {
        PickReport {
            owner: "t1".into(),
            owner_name: "Pocket".into(),
            field: "geometry.drive_faces".into(),
            id: id.into(),
            name: "F2:floor".into(),
            outcome,
            ids: vec!["f-x.1".into(); ids],
            at: None,
        }
    }

    fn report(picks: Vec<PickReport>) -> ReimportReport {
        ReimportReport {
            source: "g1".into(),
            path: "../design/block.craftx".into(),
            revision_before: Some("1".into()),
            revision: Some("2".into()),
            unchanged: false,
            matching: Default::default(),
            unmatched: Vec::new(),
            picks,
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn every_outcome_has_plain_words_and_the_toolpath_name() {
        let r = report(vec![
            pick(Outcome::Exact, "f-x.1", 1),
            pick(Outcome::Merged, "f-x.2", 1),
            pick(Outcome::Split, "f-x.3", 2),
            pick(Outcome::ByGeometry, "f-x.4", 1),
            pick(Outcome::Missing, "e-x.5", 1),
        ]);
        let vm = ReportVm::new(&[r]);
        assert_eq!(vm.title, "Model updated");
        assert_eq!(vm.sources[0].heading, "block.craftx: revision 1 → 2");
        let rows = &vm.sources[0].rows;
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["Found", "Merged", "Split", "Check", "Missing"]);
        assert!(rows.iter().all(|r| r.toolpath_name == "Pocket"));
        assert_eq!(rows[0].text, "The face F2:floor was found by its name.");
        assert!(
            rows[1].text.contains("merged into another face"),
            "{}",
            rows[1].text
        );
        assert!(rows[2].text.contains("is now 2 pieces"), "{}", rows[2].text);
        assert!(
            rows[3].text.contains("by its shape only"),
            "{}",
            rows[3].text
        );
        assert!(
            rows[4]
                .text
                .starts_with("The edge F2:floor is not in the model any more"),
            "{}",
            rows[4].text
        );
        assert!(vm.broken);
        assert_eq!(vm.summary, "5 picks; 1 toolpaths broken");
    }

    #[test]
    fn an_unchanged_file_and_no_picks_are_said() {
        let mut r = report(Vec::new());
        r.unchanged = true;
        r.revision_before = r.revision.clone();
        let vm = ReportVm::new(&[r]);
        assert_eq!(vm.sources[0].heading, "block.craftx: revision 2");
        assert_eq!(
            vm.sources[0].notes,
            [
                "The file has not changed.",
                "No toolpath picks a named face or edge of this model."
            ]
        );
        assert!(!vm.broken);
    }

    #[test]
    fn the_panel_shows_app_revision_time_and_parameters() {
        let l = LinkView {
            source: "g1".into(),
            path: "../design/block.craftx".into(),
            source_id: "d".into(),
            revision: "7".into(),
            app: Some("SolidCraft".into()),
            app_version: Some("0.12.0".into()),
            saved: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_795_000_000)),
            params: vec![("wall".into(), "4".into(), Some("mm".into()))],
            material: None,
            part_number: Some("BR-12".into()),
        };
        let p = LinkPanel::new(&l, true);
        assert_eq!(p.heading, "Linked model block.craftx");
        assert_eq!(
            p.lines,
            [
                "From SolidCraft 0.12.0",
                "Revision 7",
                "Saved 2026-11-18 11:06 UTC",
                "Part number BR-12"
            ]
        );
        assert_eq!(p.params, [("wall".to_owned(), "4 mm".to_owned())]);
        assert!(p.changed);
    }

    #[test]
    fn opening_a_linked_file_offers_to_update_a_project_that_links_it() {
        let mut l = LinkRead {
            path: "b.craftx".into(),
            source_id: "d".into(),
            revision: "2".into(),
            linked: Some("g1".into()),
        };
        assert_eq!(
            plan(&l, true, true),
            LinkPlan::OfferUpdate {
                source: "g1".into()
            }
        );
        l.linked = None;
        assert_eq!(plan(&l, true, false), LinkPlan::Import);
        assert_eq!(plan(&l, true, true), LinkPlan::NewProject);
        assert_eq!(plan(&l, false, false), LinkPlan::NewProject);
    }
}
