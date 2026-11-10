// SPDX-License-Identifier: MIT OR Apache-2.0

//! The "Getting started" guide: the steps of a first job, which of them the
//! project already shows as done, and the ribbon command each step runs.
//!
//! The guide holds no commands of its own. Each step names a
//! [`Command`] and the window runs it exactly as the ribbon does, so the
//! dialogs, the unit question and the undo steps are the same. A step is
//! done when the project says so (a model is imported, a toolpath is
//! calculated); only posting leaves no mark in the project, so the guide
//! notes it itself.

use super::Selection;
use super::inspector::tool_label;
use super::ribbon::{Category, Command, current_setup};
use crate::jobs::{JobKind, Jobs, Outcome};
use crate::session::{Snapshot, ToolpathStatus};
use crate::text::tr;
use fluut_model::ToolpathTypeId;
use fluut_model::project::SetupKind;

/// One step of the first job.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum GuideStep {
    NewProject,
    Import,
    Setup,
    Toolpath,
    Calculate,
    Simulate,
    Post,
}

impl GuideStep {
    pub const ALL: [GuideStep; 7] = [
        GuideStep::NewProject,
        GuideStep::Import,
        GuideStep::Setup,
        GuideStep::Toolpath,
        GuideStep::Calculate,
        GuideStep::Simulate,
        GuideStep::Post,
    ];

    /// Stable id, for scripts and tests.
    pub fn id(self) -> &'static str {
        match self {
            GuideStep::NewProject => "guide.new_project",
            GuideStep::Import => "guide.import",
            GuideStep::Setup => "guide.setup",
            GuideStep::Toolpath => "guide.toolpath",
            GuideStep::Calculate => "guide.calculate",
            GuideStep::Simulate => "guide.simulate",
            GuideStep::Post => "guide.post",
        }
    }

    /// The ribbon command the step's button runs. `toolpath` is the type
    /// the guide offers for the toolpath step.
    pub fn command(self, toolpath: ToolpathTypeId) -> Command {
        match self {
            GuideStep::NewProject => Command::NewProject,
            GuideStep::Import => Command::ImportModel,
            GuideStep::Setup => Command::NewSetup(SetupKind::ThreeAxis),
            GuideStep::Toolpath => Command::AddToolpath(toolpath),
            GuideStep::Calculate => Command::Calculate,
            GuideStep::Simulate => Command::Simulate,
            GuideStep::Post => Command::Post,
        }
    }

    /// Catalog ids of the title, the hint and the button.
    fn texts(self) -> (&'static str, &'static str, &'static str) {
        match self {
            GuideStep::NewProject => (
                "guide-new-project-title",
                "guide-new-project-hint",
                "guide-new-project-button",
            ),
            GuideStep::Import => (
                "guide-import-title",
                "guide-import-hint",
                "guide-import-button",
            ),
            GuideStep::Setup => (
                "guide-setup-title",
                "guide-setup-hint",
                "guide-setup-button",
            ),
            GuideStep::Toolpath => (
                "guide-toolpath-title",
                "guide-toolpath-hint",
                "guide-toolpath-button",
            ),
            GuideStep::Calculate => (
                "guide-calculate-title",
                "guide-calculate-hint",
                "guide-calculate-button",
            ),
            GuideStep::Simulate => (
                "guide-simulate-title",
                "guide-simulate-hint",
                "guide-simulate-button",
            ),
            GuideStep::Post => ("guide-post-title", "guide-post-hint", "guide-post-button"),
        }
    }
}

/// What the guide remembers between frames. Not part of the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuideState {
    /// The card is showing.
    pub open: bool,
    /// G-code has been written for the open project. Posting leaves no mark
    /// in the project, so the guide notes the job.
    pub posted: bool,
    /// The toolpath type the toolpath step adds.
    pub toolpath_type: ToolpathTypeId,
    /// The tool the toolpath step gives the new toolpath: `library#tool`.
    /// `None` keeps the tool that adding a toolpath chose.
    pub tool: Option<String>,
    /// A tool waiting for the toolpath being added.
    pub pending_tool: Option<String>,
}

impl Default for GuideState {
    fn default() -> Self {
        GuideState {
            open: false,
            posted: false,
            // Needs nothing picked on the model, so a first job works.
            toolpath_type: ToolpathTypeId::Parallel,
            tool: None,
            pending_tool: None,
        }
    }
}

impl GuideState {
    /// Notes the outcome of the last job: posting ticks the last step,
    /// another project starts again.
    pub fn observe(&mut self, last: Option<&(JobKind, Outcome)>) {
        match last {
            Some((_, Outcome::Posted { .. })) => self.posted = true,
            Some((_, Outcome::Opened { .. } | Outcome::Created { .. })) => self.posted = false,
            // The toolpath never came, so its tool has no home.
            Some((JobKind::AddToolpath, Outcome::Failed(_))) => self.pending_tool = None,
            _ => {}
        }
    }
}

/// A step as the window shows it.
#[derive(Clone, PartialEq, Debug)]
pub struct StepVm {
    pub step: GuideStep,
    pub title: String,
    pub hint: String,
    pub button: String,
    pub command: Command,
    /// The project shows the step as done.
    pub done: bool,
    /// The first step not done: the one to do next.
    pub current: bool,
    /// The button can run now.
    pub enabled: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct GuideVm {
    pub steps: Vec<StepVm>,
    pub done: usize,
    /// Every step is done.
    pub complete: bool,
    /// The types the toolpath step offers for the Setup in use.
    pub types: Vec<(ToolpathTypeId, String)>,
    /// The type the step adds: the choice, else the first offered.
    pub toolpath_type: ToolpathTypeId,
    /// `(reference, label)`; the first, with an empty reference, keeps the
    /// tool that adding a toolpath chooses.
    pub tools: Vec<(String, String)>,
    /// The reference chosen; empty for the default.
    pub tool: String,
    /// The Setup's stock and work zero can be shown.
    pub can_show_setup: bool,
}

/// Whether every toolpath that is on has been calculated, and one is.
fn calculated(s: &Snapshot) -> bool {
    let mut any = false;
    for t in s.project.setups.iter().flat_map(|st| &st.toolpaths) {
        match s.status(&t.id) {
            ToolpathStatus::Done | ToolpathStatus::Warning => any = true,
            ToolpathStatus::Disabled => {}
            _ => return false,
        }
    }
    any
}

impl GuideVm {
    pub fn new(jobs: &Jobs, selection: &Selection, state: &GuideState) -> GuideVm {
        let s = jobs.snapshot.as_ref();
        let ready = s.is_some() && !jobs.is_busy();
        let has_setup = s.is_some_and(|s| !s.project.setups.is_empty());
        let has_toolpath =
            s.is_some_and(|s| s.project.setups.iter().any(|st| !st.toolpaths.is_empty()));
        let kind = s
            .and_then(|s| {
                let id = current_setup(s, selection)?;
                fluut_model::setup::resolve_setup(&s.project, &id).ok()
            })
            .map_or(SetupKind::ThreeAxis, |r| r.kind);
        let types: Vec<(ToolpathTypeId, String)> = Category::ALL
            .into_iter()
            .filter(|c| c.allowed_in(kind))
            .flat_map(|c| c.types().iter().copied())
            .map(|t| (t, Command::AddToolpath(t).label()))
            .collect();
        let toolpath_type = if types.iter().any(|(t, _)| *t == state.toolpath_type) {
            state.toolpath_type
        } else {
            types.first().map_or(state.toolpath_type, |(t, _)| *t)
        };
        let mut tools = vec![(String::new(), tr("guide-tool-default"))];
        if let Some(s) = s {
            let unit = s.project.units;
            for lib in s.tools.libraries() {
                let name = if lib.name.is_empty() {
                    &lib.id
                } else {
                    &lib.name
                };
                for t in &lib.tools {
                    tools.push((format!("{}#{}", lib.id, t.id), tool_label(name, t, unit)));
                }
            }
        }
        let tool = state
            .tool
            .clone()
            .filter(|r| tools.iter().any(|(v, _)| v == r))
            .unwrap_or_default();

        let done_of = |step: GuideStep| match step {
            GuideStep::NewProject => s.is_some(),
            GuideStep::Import => s.is_some_and(|s| !s.project.geometry.is_empty()),
            GuideStep::Setup => has_setup,
            GuideStep::Toolpath => has_toolpath,
            GuideStep::Calculate => s.is_some_and(|s| calculated(s)),
            GuideStep::Simulate => s.is_some_and(|s| !s.sims.is_empty()),
            GuideStep::Post => s.is_some() && state.posted,
        };
        let enabled_of = |step: GuideStep| match step {
            GuideStep::NewProject => !jobs.is_busy(),
            GuideStep::Import | GuideStep::Setup => ready,
            GuideStep::Toolpath => ready && has_setup,
            GuideStep::Calculate | GuideStep::Simulate | GuideStep::Post => ready && has_toolpath,
        };
        let mut found_current = false;
        let steps: Vec<StepVm> = GuideStep::ALL
            .into_iter()
            .map(|step| {
                let done = done_of(step);
                let current = !done && !found_current;
                found_current |= current;
                let (title, hint, button) = step.texts();
                StepVm {
                    step,
                    title: tr(title),
                    hint: tr(hint),
                    button: tr(button),
                    command: step.command(toolpath_type),
                    done,
                    current,
                    enabled: enabled_of(step),
                }
            })
            .collect();
        let done = steps.iter().filter(|x| x.done).count();
        GuideVm {
            complete: done == steps.len(),
            done,
            steps,
            types,
            toolpath_type,
            tools,
            tool,
            can_show_setup: ready && has_setup,
        }
    }

    pub fn step(&self, step: GuideStep) -> &StepVm {
        self.steps
            .iter()
            .find(|x| x.step == step)
            .expect("every step is listed")
    }
}

#[cfg(test)]
mod tests;
