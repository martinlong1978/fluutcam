// SPDX-License-Identifier: MIT OR Apache-2.0

//! Ribbon commands (S 08 §8.3) and when they are enabled. The ribbon holds
//! commands only, never parameters.

use crate::jobs::Jobs;
use crate::text::tr;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Command {
    Open,
    Calculate,
    Post,
    Simulate,
    Cancel,
}

impl Command {
    pub const ALL: [Command; 5] = [
        Command::Open,
        Command::Calculate,
        Command::Post,
        Command::Simulate,
        Command::Cancel,
    ];

    /// Stable id, for scripts and tests (D13 §3).
    pub fn id(self) -> &'static str {
        match self {
            Command::Open => "project.open",
            Command::Calculate => "toolpath.calculate_all",
            Command::Post => "post.all",
            Command::Simulate => "simulation.run",
            Command::Cancel => "job.cancel",
        }
    }

    pub fn label(self) -> String {
        tr(match self {
            Command::Open => "ribbon-open",
            Command::Calculate => "ribbon-calculate-all",
            Command::Post => "ribbon-post",
            Command::Simulate => "ribbon-simulate",
            Command::Cancel => "ribbon-cancel",
        })
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct CommandVm {
    pub command: Command,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct RibbonVm {
    pub commands: Vec<CommandVm>,
}

impl RibbonVm {
    pub fn new(jobs: &Jobs) -> RibbonVm {
        let busy = jobs.is_busy();
        let open = jobs.snapshot.is_some();
        RibbonVm {
            commands: Command::ALL
                .iter()
                .map(|&c| CommandVm {
                    command: c,
                    label: c.label(),
                    enabled: match c {
                        Command::Open => !busy,
                        Command::Calculate | Command::Post | Command::Simulate => open && !busy,
                        Command::Cancel => busy,
                    },
                })
                .collect(),
        }
    }

    pub fn enabled(&self, c: Command) -> bool {
        self.commands.iter().any(|x| x.command == c && x.enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::CalcTargets;
    use crate::session::tests::opened;

    #[test]
    fn enabling_follows_project_and_job() {
        let j = Jobs::start(|| {});
        let r = RibbonVm::new(&j);
        assert!(r.enabled(Command::Open));
        assert!(!r.enabled(Command::Calculate) && !r.enabled(Command::Post));
        assert!(!r.enabled(Command::Cancel));

        let mut j = opened("boss");
        let r = RibbonVm::new(&j);
        assert!(r.enabled(Command::Calculate) && r.enabled(Command::Post));
        assert!(r.enabled(Command::Simulate));
        j.calculate(CalcTargets::All);
        let r = RibbonVm::new(&j);
        assert!(r.enabled(Command::Cancel));
        assert!(!r.enabled(Command::Open) && !r.enabled(Command::Calculate));
        assert!(!r.enabled(Command::Simulate));
        j.wait();
        assert!(RibbonVm::new(&j).enabled(Command::Post));
    }
}
