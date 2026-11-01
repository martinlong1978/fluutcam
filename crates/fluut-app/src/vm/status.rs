// SPDX-License-Identifier: MIT OR Apache-2.0

//! The job strip (D13 §8): what runs, how far it is, and how the last job
//! ended.

use crate::jobs::{JobKind, Jobs, Outcome};
use crate::text::{diagnostic, tr, tr_args};

#[derive(Clone, PartialEq, Debug)]
pub struct StatusVm {
    pub text: String,
    /// Fraction done, while a job runs.
    pub progress: Option<f32>,
    pub cancellable: bool,
    /// The last job failed.
    pub failed: bool,
}

impl StatusVm {
    pub fn new(jobs: &Jobs) -> StatusVm {
        if let Some(r) = &jobs.running {
            let (done, total) = r.cx.progress.get();
            let text = match r.kind {
                JobKind::Open => tr_args("job-opening", &[("file", &r.subject)]),
                JobKind::Post if r.toolpath.is_none() => {
                    tr_args("job-posting", &[("post", &r.subject)])
                }
                JobKind::Calculate | JobKind::Post => {
                    let name = jobs
                        .snapshot
                        .as_ref()
                        .zip(r.toolpath.as_ref())
                        .and_then(|(s, id)| s.project.toolpath(id))
                        .map_or_else(String::new, |(_, t)| t.name.clone());
                    tr_args(
                        "job-calculating",
                        &[
                            ("name", &name),
                            ("done", &(done + 1).min(total).to_string()),
                            ("total", &total.to_string()),
                        ],
                    )
                }
            };
            return StatusVm {
                text,
                progress: Some(r.cx.progress.fraction()),
                cancellable: true,
                failed: false,
            };
        }
        let (text, failed) = match &jobs.last {
            None => (tr("job-idle"), false),
            Some((_, o)) => match o {
                Outcome::Opened { file } => (tr_args("job-opened", &[("file", file)]), false),
                Outcome::Calculated { count } => (
                    tr_args("job-calculated", &[("count", &count.to_string())]),
                    false,
                ),
                Outcome::Posted { files } => {
                    let names: Vec<String> =
                        files.iter().map(|f| f.display().to_string()).collect();
                    (
                        tr_args("job-posted", &[("files", &names.join(", "))]),
                        false,
                    )
                }
                Outcome::Cancelled => (tr("job-cancelled"), false),
                Outcome::Failed(d) => {
                    let reason = d.first().map(diagnostic).unwrap_or_default();
                    (tr_args("job-failed", &[("reason", &reason)]), true)
                }
            },
        };
        StatusVm {
            text,
            progress: None,
            cancellable: false,
            failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tests::opened;

    #[test]
    fn idle_open_and_failed() {
        let j = Jobs::start(|| {});
        assert_eq!(StatusVm::new(&j).text, "Ready");
        let j = opened("boss");
        assert_eq!(StatusVm::new(&j).text, "Opened boss.fluut");
        let mut j = Jobs::start(|| {});
        j.open("/no/such.fluut".into());
        let s = StatusVm::new(&j);
        assert!(s.cancellable && s.progress.is_some());
        assert_eq!(s.text, "Opening such.fluut");
        j.wait();
        let s = StatusVm::new(&j);
        assert!(
            s.failed && s.text.starts_with("Failed: cannot read"),
            "{}",
            s.text
        );
    }
}
