// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ribbon (S 08 §8.3): workflow stages, their groups of commands, and
//! when each command is enabled. The ribbon holds commands only, never
//! parameters.
//!
//! A command whose operation does not exist yet ([`Command::wired`] is
//! false, today the Library stage) shows, disabled, with a hover text
//! saying it is coming.

use super::Selection;
use super::playback::{Playback, PlaybackVm, StockColour};
use crate::jobs::Jobs;
use crate::session::Snapshot;
use crate::text::{tr, tr_args};
use fluut_model::ToolpathTypeId;
use fluut_model::project::SetupKind;

/// A workflow stage: one ribbon tab (D13 §2).
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Stage {
    #[default]
    Model,
    Setup,
    Toolpaths,
    Simulate,
    Post,
    Library,
}

impl Stage {
    pub const ALL: [Stage; 6] = [
        Stage::Model,
        Stage::Setup,
        Stage::Toolpaths,
        Stage::Simulate,
        Stage::Post,
        Stage::Library,
    ];

    pub fn label(self) -> String {
        tr(match self {
            Stage::Model => "ribbon-stage-model",
            Stage::Setup => "ribbon-stage-setup",
            Stage::Toolpaths => "ribbon-stage-toolpaths",
            Stage::Simulate => "ribbon-stage-simulate",
            Stage::Post => "ribbon-stage-post",
            Stage::Library => "ribbon-stage-library",
        })
    }
}

/// A toolpath gallery (04 §4.13). Multi-axis has no v1 types, so no gallery.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Category {
    TwoD,
    Holes,
    ThreeD,
    Rotary,
    Utility,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::TwoD,
        Category::Holes,
        Category::ThreeD,
        Category::Rotary,
        Category::Utility,
    ];

    /// The implemented types of the gallery, in catalogue order.
    pub fn types(self) -> &'static [ToolpathTypeId] {
        use ToolpathTypeId as T;
        match self {
            Category::TwoD => &[
                T::Face,
                T::OptimisedLoad2d,
                T::Pocket2d,
                T::Contour2d,
                T::Trace,
                T::Engrave,
                T::Chamfer2d,
                T::Bore,
            ],
            Category::Holes => &[T::Drill],
            Category::ThreeD => &[
                T::OptimisedLoad3d,
                T::Pocket3d,
                T::Parallel,
                T::Contour,
                T::Horizontal,
                T::Spiral,
                T::Scallop,
                T::SteepAndShallow,
            ],
            Category::Rotary => &[
                T::RotaryRough,
                T::RotaryAlongX,
                T::RotaryAroundA,
                T::RotarySpiral,
            ],
            Category::Utility => &[T::ManualNc],
        }
    }

    /// Whether Setups of `kind` may hold the types of this gallery
    /// (03 §3.9.2, 04 §4.13): rotary types in `rotary` Setups only, the
    /// other milling types never there, utility types everywhere.
    pub fn allowed_in(self, kind: SetupKind) -> bool {
        match self {
            Category::Rotary => kind == SetupKind::Rotary,
            Category::Utility => true,
            Category::TwoD | Category::Holes | Category::ThreeD => kind != SetupKind::Rotary,
        }
    }

    fn label(self) -> String {
        tr(match self {
            Category::TwoD => "ribbon-gallery-2d",
            Category::Holes => "ribbon-gallery-holes",
            Category::ThreeD => "ribbon-gallery-3d",
            Category::Rotary => "ribbon-gallery-rotary",
            Category::Utility => "ribbon-gallery-utility",
        })
    }
}

/// A key chord. `command` is Ctrl, or Cmd on macOS.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Shortcut {
    pub command: bool,
    pub shift: bool,
    pub key: char,
}

impl Shortcut {
    /// As shown in tooltips, for example `Ctrl+Shift+S`, or `Cmd+Shift+S`
    /// on macOS.
    pub fn text(self) -> String {
        self.text_for(cfg!(target_os = "macos"))
    }

    /// As [`Shortcut::text`], for either platform.
    pub fn text_for(self, mac: bool) -> String {
        let mut s = String::new();
        if self.command {
            s.push_str(if mac { "Cmd+" } else { "Ctrl+" });
        }
        if self.shift {
            s.push_str("Shift+");
        }
        s.push(self.key);
        s
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Command {
    NewProject,
    Open,
    Save,
    SaveAs,
    ImportModel,
    Reimport,
    Undo,
    Redo,
    NewSetup(SetupKind),
    NewSetupBasedOn,
    Stock,
    WorkZero,
    AddToolpath(ToolpathTypeId),
    CalculateSelected,
    Calculate,
    Simulate,
    StepBack,
    PlayPause,
    StepForward,
    Post,
    Cancel,
    ImportTools,
    AddToolLibrary,
    ImportPost,
    LibraryTools,
    LibraryMachines,
    LibraryPosts,
}

impl Command {
    /// Every command, each once.
    pub fn all() -> Vec<Command> {
        let mut v = vec![
            Command::NewProject,
            Command::Open,
            Command::Save,
            Command::SaveAs,
            Command::ImportModel,
            Command::Reimport,
            Command::Undo,
            Command::Redo,
        ];
        v.extend(SetupKind::ALL.map(Command::NewSetup));
        v.extend([Command::NewSetupBasedOn, Command::Stock, Command::WorkZero]);
        v.extend(ToolpathTypeId::ALL.map(Command::AddToolpath));
        v.extend([
            Command::CalculateSelected,
            Command::Calculate,
            Command::Simulate,
            Command::StepBack,
            Command::PlayPause,
            Command::StepForward,
            Command::Post,
            Command::Cancel,
            Command::ImportTools,
            Command::AddToolLibrary,
            Command::ImportPost,
            Command::LibraryTools,
            Command::LibraryMachines,
            Command::LibraryPosts,
        ]);
        v
    }

    /// Stable id, for scripts and tests (D13 §3).
    pub fn id(self) -> &'static str {
        use ToolpathTypeId as T;
        match self {
            Command::NewProject => "project.new",
            Command::Open => "project.open",
            Command::Save => "project.save",
            Command::SaveAs => "project.save_as",
            Command::ImportModel => "model.import",
            Command::Reimport => "model.reimport",
            Command::Undo => "edit.undo",
            Command::Redo => "edit.redo",
            Command::NewSetup(SetupKind::ThreeAxis) => "setup.new_3axis",
            Command::NewSetup(SetupKind::Rotary) => "setup.new_rotary",
            Command::NewSetup(SetupKind::Indexed) => "setup.new_indexed",
            Command::NewSetupBasedOn => "setup.new_based_on",
            Command::Stock => "setup.stock",
            Command::WorkZero => "setup.work_zero",
            Command::AddToolpath(t) => match t {
                T::Contour2d => "toolpath.add:contour_2d",
                T::Pocket2d => "toolpath.add:pocket_2d",
                T::Parallel => "toolpath.add:parallel",
                T::Face => "toolpath.add:face",
                T::Trace => "toolpath.add:trace",
                T::Chamfer2d => "toolpath.add:chamfer_2d",
                T::Bore => "toolpath.add:bore",
                T::Drill => "toolpath.add:drill",
                T::OptimisedLoad2d => "toolpath.add:optimised_load_2d",
                T::Engrave => "toolpath.add:engrave",
                T::Contour => "toolpath.add:contour",
                T::Horizontal => "toolpath.add:horizontal",
                T::Spiral => "toolpath.add:spiral",
                T::Scallop => "toolpath.add:scallop",
                T::SteepAndShallow => "toolpath.add:steep_and_shallow",
                T::OptimisedLoad3d => "toolpath.add:optimised_load_3d",
                T::Pocket3d => "toolpath.add:pocket_3d",
                T::RotaryRough => "toolpath.add:rotary_rough",
                T::RotaryAlongX => "toolpath.add:rotary_along_x",
                T::RotaryAroundA => "toolpath.add:rotary_around_a",
                T::RotarySpiral => "toolpath.add:rotary_spiral",
                T::ManualNc => "toolpath.add:manual_nc",
            },
            Command::CalculateSelected => "toolpath.calculate_selected",
            Command::Calculate => "toolpath.calculate_all",
            Command::Simulate => "simulation.run",
            Command::StepBack => "simulation.step_back",
            Command::PlayPause => "simulation.play_pause",
            Command::StepForward => "simulation.step_forward",
            Command::Post => "post.all",
            Command::Cancel => "job.cancel",
            Command::ImportTools => "library.import_tools",
            Command::AddToolLibrary => "library.add_tool_library",
            Command::ImportPost => "library.import_post",
            Command::LibraryTools => "library.tools",
            Command::LibraryMachines => "library.machines",
            Command::LibraryPosts => "library.posts",
        }
    }

    pub fn label(self) -> String {
        if let Command::AddToolpath(t) = self {
            return tr(&format!("toolpath-type-{}", t.as_str()));
        }
        tr(match self {
            Command::NewProject => "ribbon-new-project",
            Command::Open => "ribbon-open",
            Command::Save => "ribbon-save",
            Command::SaveAs => "ribbon-save-as",
            Command::ImportModel => "ribbon-import-model",
            Command::Reimport => "ribbon-reimport",
            Command::Undo => "ribbon-undo",
            Command::Redo => "ribbon-redo",
            Command::NewSetup(SetupKind::ThreeAxis) => "ribbon-new-setup-3axis",
            Command::NewSetup(SetupKind::Rotary) => "ribbon-new-setup-rotary",
            Command::NewSetup(SetupKind::Indexed) => "ribbon-new-setup-indexed",
            Command::NewSetupBasedOn => "ribbon-new-setup-based-on",
            Command::Stock => "ribbon-stock",
            Command::WorkZero => "ribbon-work-zero",
            Command::CalculateSelected => "ribbon-calculate-selected",
            Command::Calculate => "ribbon-calculate-all",
            Command::Simulate => "ribbon-simulate",
            Command::StepBack => "ribbon-step-back",
            Command::PlayPause => "ribbon-play-pause",
            Command::StepForward => "ribbon-step-forward",
            Command::Post => "ribbon-post",
            Command::Cancel => "ribbon-cancel",
            Command::ImportTools => "ribbon-import-tools",
            Command::AddToolLibrary => "ribbon-add-tool-library",
            Command::ImportPost => "ribbon-import-post",
            Command::LibraryTools => "ribbon-library-tools",
            Command::LibraryMachines => "ribbon-library-machines",
            Command::LibraryPosts => "ribbon-library-posts",
            Command::AddToolpath(_) => unreachable!("returned above"),
        })
    }

    /// Name of the icon: a file stem in `assets/icons/`.
    pub fn icon(self) -> &'static str {
        use ToolpathTypeId as T;
        match self {
            Command::NewProject => "new-project",
            Command::Open => "open",
            Command::Save => "save",
            Command::SaveAs => "save-as",
            Command::ImportModel => "import",
            Command::Reimport => "reimport",
            Command::Undo => "undo",
            Command::Redo => "redo",
            Command::NewSetup(SetupKind::ThreeAxis) => "new-setup-3axis",
            Command::NewSetup(SetupKind::Rotary) => "new-setup-rotary",
            Command::NewSetup(SetupKind::Indexed) => "new-setup-indexed",
            Command::NewSetupBasedOn => "new-setup-based-on",
            Command::Stock => "stock",
            Command::WorkZero => "work-zero",
            Command::AddToolpath(t) => match t {
                T::Contour2d => "tp-contour-2d",
                T::Pocket2d => "tp-pocket-2d",
                T::Parallel => "tp-parallel",
                T::Face => "tp-face",
                T::Trace => "tp-trace",
                T::Chamfer2d => "tp-chamfer-2d",
                T::Bore => "tp-bore",
                T::Drill => "tp-drill",
                T::OptimisedLoad2d => "tp-optimised-load-2d",
                T::Engrave => "tp-engrave",
                T::Contour => "tp-contour",
                T::Horizontal => "tp-horizontal",
                T::Spiral => "tp-spiral",
                T::Scallop => "tp-scallop",
                T::SteepAndShallow => "tp-steep-and-shallow",
                T::OptimisedLoad3d => "tp-optimised-load-3d",
                T::Pocket3d => "tp-pocket-3d",
                T::RotaryRough => "tp-rotary-rough",
                T::RotaryAlongX => "tp-rotary-along-x",
                T::RotaryAroundA => "tp-rotary-around-a",
                T::RotarySpiral => "tp-rotary-spiral",
                T::ManualNc => "tp-manual-nc",
            },
            Command::CalculateSelected => "calculate-selected",
            Command::Calculate => "calculate",
            Command::Simulate => "simulate",
            Command::StepBack => "step-back",
            Command::PlayPause => "play",
            Command::StepForward => "step-forward",
            Command::Post => "post",
            Command::Cancel => "cancel",
            Command::ImportTools => "import-tools",
            Command::AddToolLibrary => "add-tool-library",
            Command::ImportPost => "import-post",
            Command::LibraryTools => "library-tools",
            Command::LibraryMachines => "library-machines",
            Command::LibraryPosts => "library-posts",
        }
    }

    pub fn shortcut(self) -> Option<Shortcut> {
        let chord = |shift, key| {
            Some(Shortcut {
                command: true,
                shift,
                key,
            })
        };
        match self {
            Command::NewProject => chord(false, 'N'),
            Command::Open => chord(false, 'O'),
            Command::Save => chord(false, 'S'),
            Command::SaveAs => chord(true, 'S'),
            Command::ImportModel => chord(false, 'I'),
            Command::Undo => chord(false, 'Z'),
            Command::Redo => chord(false, 'Y'),
            _ => None,
        }
    }

    /// A second chord for the same command.
    pub fn alt_shortcut(self) -> Option<Shortcut> {
        match self {
            Command::Redo => Some(Shortcut {
                command: true,
                shift: true,
                key: 'Z',
            }),
            _ => None,
        }
    }

    /// Whether the keys of the command are left to a focused text field.
    pub fn yields_to_text(self) -> bool {
        matches!(self, Command::Undo | Command::Redo)
    }

    /// The operation exists, so the command can run.
    pub fn wired(self) -> bool {
        !matches!(
            self,
            Command::LibraryTools | Command::LibraryMachines | Command::LibraryPosts
        )
    }

    /// Shown as a large icon over its label; else a small icon beside it.
    pub fn large(self) -> bool {
        !matches!(
            self,
            Command::Save
                | Command::SaveAs
                | Command::Reimport
                | Command::Undo
                | Command::Redo
                | Command::NewSetupBasedOn
                | Command::AddToolpath(_)
                | Command::StepBack
                | Command::StepForward
        )
    }
}

/// A group of the ribbon. `Output` also holds the post choice.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum GroupId {
    File,
    Import,
    NewSetup,
    Workpiece,
    Gallery(Category),
    Calculate,
    Simulation,
    Playback,
    Output,
    Library,
    Edit,
    Job,
}

impl GroupId {
    fn label(self) -> String {
        if let GroupId::Gallery(c) = self {
            return c.label();
        }
        tr(match self {
            GroupId::File => "ribbon-group-file",
            GroupId::Import => "ribbon-group-import",
            GroupId::NewSetup => "ribbon-group-new-setup",
            GroupId::Workpiece => "ribbon-group-workpiece",
            GroupId::Calculate => "ribbon-group-calculate",
            GroupId::Simulation => "ribbon-group-simulation",
            GroupId::Playback => "ribbon-group-playback",
            GroupId::Output => "ribbon-group-output",
            GroupId::Library => "ribbon-group-library",
            GroupId::Edit => "ribbon-group-edit",
            GroupId::Job => "ribbon-group-job",
            GroupId::Gallery(_) => unreachable!("returned above"),
        })
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct CommandVm {
    pub command: Command,
    pub label: String,
    /// Name, shortcut, and whether it is still to come.
    pub tooltip: String,
    pub icon: &'static str,
    pub large: bool,
    pub enabled: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct GroupVm {
    pub id: GroupId,
    pub label: String,
    pub items: Vec<CommandVm>,
}

/// What the ribbon depends on besides the jobs.
#[derive(Copy, Clone, Debug)]
pub struct RibbonCx<'a> {
    pub stage: Stage,
    pub selection: &'a Selection,
    pub playback: &'a Playback,
}

#[derive(Clone, PartialEq, Debug)]
pub struct RibbonVm {
    pub stage: Stage,
    pub groups: Vec<GroupVm>,
    /// The resolved kind of the selected Setup, or of the first Setup.
    pub setup_kind: Option<SetupKind>,
    /// What Calculate selected calculates.
    pub selected_toolpaths: Vec<String>,
    /// The Setup the selection belongs to, else the first.
    pub setup: Option<String>,
    busy: bool,
    open: bool,
    has_geometry: bool,
    can_undo: bool,
    can_redo: bool,
    can_play: bool,
    can_back: bool,
    can_forward: bool,
}

/// The Setup a selection belongs to, else the first (as the viewport shows).
pub(crate) fn current_setup(s: &Snapshot, selection: &Selection) -> Option<String> {
    let id = match selection {
        Selection::Setup(id) | Selection::Face { setup: id, .. } => Some(id.clone()),
        Selection::Toolpath(t) => s.project.toolpath(t).map(|(st, _)| st.id.clone()),
        Selection::None => None,
    };
    id.or_else(|| s.project.setups.first().map(|st| st.id.clone()))
}

/// The toolpath selected, or all those of the Setup selected.
fn selected_toolpaths(s: &Snapshot, selection: &Selection) -> Vec<String> {
    match selection {
        Selection::None => Vec::new(),
        Selection::Toolpath(t) => s
            .project
            .toolpath(t)
            .map_or_else(Vec::new, |(_, tp)| vec![tp.id.clone()]),
        Selection::Setup(id) | Selection::Face { setup: id, .. } => {
            s.project.setup(id).map_or_else(Vec::new, |st| {
                st.toolpaths.iter().map(|t| t.id.clone()).collect()
            })
        }
    }
}

impl RibbonVm {
    pub fn new(jobs: &Jobs, cx: RibbonCx<'_>) -> RibbonVm {
        let mut vm = RibbonVm {
            stage: cx.stage,
            groups: Vec::new(),
            setup_kind: None,
            selected_toolpaths: Vec::new(),
            setup: None,
            busy: jobs.is_busy(),
            open: jobs.snapshot.is_some(),
            has_geometry: false,
            can_undo: false,
            can_redo: false,
            can_play: false,
            can_back: false,
            can_forward: false,
        };
        if let Some(s) = &jobs.snapshot {
            let setup = current_setup(s, cx.selection);
            vm.setup = setup.clone();
            vm.has_geometry = !s.project.geometry.is_empty();
            vm.can_undo = s.can_undo;
            vm.can_redo = s.can_redo;
            vm.setup_kind = setup
                .as_deref()
                .and_then(|id| fluut_model::setup::resolve_setup(&s.project, id).ok())
                .map(|r| r.kind);
            vm.selected_toolpaths = selected_toolpaths(s, cx.selection);
            // Only this stage shows them; the findings text is not free.
            if cx.stage == Stage::Simulate
                && let Some(v) = setup.as_deref().and_then(|id| s.sims.get(id))
            {
                let p = PlaybackVm::new(v, cx.playback, StockColour::Plain);
                vm.can_play = true;
                vm.can_back = p.can_back;
                vm.can_forward = p.can_forward;
            }
        }
        let groups = vm
            .layout()
            .into_iter()
            .map(|(id, commands)| GroupVm {
                id,
                label: id.label(),
                items: commands.into_iter().map(|c| vm.item(c)).collect(),
            })
            .collect();
        vm.groups = groups;
        vm
    }

    /// The groups of the stage; Cancel ends every stage, so a running job
    /// can be stopped from anywhere.
    fn layout(&self) -> Vec<(GroupId, Vec<Command>)> {
        use Command as C;
        let mut groups = match self.stage {
            Stage::Model => vec![
                (
                    GroupId::File,
                    vec![C::NewProject, C::Open, C::Save, C::SaveAs],
                ),
                (GroupId::Import, vec![C::ImportModel, C::Reimport]),
            ],
            Stage::Setup => vec![
                (
                    GroupId::NewSetup,
                    vec![
                        C::NewSetup(SetupKind::ThreeAxis),
                        C::NewSetup(SetupKind::Rotary),
                        C::NewSetup(SetupKind::Indexed),
                        C::NewSetupBasedOn,
                    ],
                ),
                (GroupId::Workpiece, vec![C::Stock, C::WorkZero]),
            ],
            Stage::Toolpaths => {
                let mut g: Vec<_> = Category::ALL
                    .into_iter()
                    .filter(|c| self.setup_kind.is_none_or(|k| c.allowed_in(k)))
                    .map(|c| {
                        let types = c.types().iter().copied();
                        (GroupId::Gallery(c), types.map(C::AddToolpath).collect())
                    })
                    .collect();
                g.push((GroupId::Calculate, vec![C::CalculateSelected, C::Calculate]));
                g
            }
            Stage::Simulate => vec![
                (GroupId::Simulation, vec![C::Simulate]),
                (
                    GroupId::Playback,
                    vec![C::PlayPause, C::StepBack, C::StepForward],
                ),
            ],
            Stage::Post => vec![(GroupId::Output, vec![C::Post])],
            Stage::Library => vec![(
                GroupId::Library,
                vec![
                    C::ImportTools,
                    C::AddToolLibrary,
                    C::ImportPost,
                    C::LibraryTools,
                    C::LibraryMachines,
                    C::LibraryPosts,
                ],
            )],
        };
        groups.push((GroupId::Edit, vec![C::Undo, C::Redo]));
        groups.push((GroupId::Job, vec![C::Cancel]));
        groups
    }

    fn item(&self, c: Command) -> CommandVm {
        let label = c.label();
        let mut tooltip = match c.shortcut() {
            Some(k) => tr_args(
                "ribbon-tooltip-shortcut",
                &[("name", &label), ("keys", &k.text())],
            ),
            None => label.clone(),
        };
        if !c.wired() {
            tooltip.push('\n');
            tooltip.push_str(&tr("ribbon-coming"));
        }
        CommandVm {
            command: c,
            label,
            tooltip,
            icon: c.icon(),
            large: c.large(),
            enabled: self.enabled(c),
        }
    }

    /// Whether `c` can run now, on whichever stage it lives.
    pub fn enabled(&self, c: Command) -> bool {
        let ready = self.open && !self.busy;
        c.wired()
            && match c {
                Command::NewProject
                | Command::Open
                | Command::ImportTools
                | Command::ImportPost => !self.busy,
                Command::Save
                | Command::SaveAs
                | Command::ImportModel
                | Command::AddToolLibrary => ready,
                Command::Reimport => ready && self.has_geometry,
                Command::Undo => ready && self.can_undo,
                Command::Redo => ready && self.can_redo,
                Command::NewSetup(_) => ready,
                Command::NewSetupBasedOn => ready && self.setup_kind.is_some(),
                Command::Stock | Command::WorkZero => ready && self.setup.is_some(),
                Command::AddToolpath(_) => ready && self.setup.is_some(),
                Command::CalculateSelected => ready && !self.selected_toolpaths.is_empty(),
                Command::Calculate | Command::Post | Command::Simulate => ready,
                Command::PlayPause => self.can_play,
                Command::StepBack => self.can_back,
                Command::StepForward => self.can_forward,
                Command::Cancel => self.busy,
                _ => false,
            }
    }

    /// The commands the stage shows, in order.
    pub fn commands(&self) -> impl Iterator<Item = &CommandVm> {
        self.groups.iter().flat_map(|g| &g.items)
    }

    /// The types in the galleries the stage shows.
    pub fn gallery_types(&self) -> Vec<ToolpathTypeId> {
        self.commands()
            .filter_map(|c| match c.command {
                Command::AddToolpath(t) => Some(t),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
