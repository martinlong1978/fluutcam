// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a script run needs from its host (D12 §2): console output, the
//! app's dialogs and cancellation.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Console line level.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warning",
            Level::Error => "error",
        }
    }
}

/// A dialog the script asks the app to show (D12b §7).
#[derive(Clone, PartialEq, Debug)]
pub enum UiRequest {
    Message(String),
    AskNumber {
        label: String,
        default: f64,
        unit: String,
    },
    AskChoice {
        label: String,
        options: Vec<String>,
    },
    PickFaces(String),
    PickFile(String),
}

/// The user's answer.
#[derive(Clone, PartialEq, Debug)]
pub enum UiResponse {
    Done,
    Number(f64),
    Choice(String),
    Faces(Vec<String>),
    File(std::path::PathBuf),
}

/// The text of the error a headless `ui` call raises.
pub const NO_UI: &str = "no UI in headless mode";

/// The host of a run. The app implements it with its console pane and
/// dialogs; headless runs use [`Console`].
pub trait ScriptHost: Send + Sync {
    /// One console line.
    fn log(&self, level: Level, msg: &str);

    /// Shows a dialog and waits for the answer. `Err` is a cancelled
    /// dialog or a host without UI; the script may catch it.
    fn ui_request(&self, _req: UiRequest) -> Result<UiResponse, String> {
        Err(NO_UI.into())
    }

    /// True once the user has cancelled the run.
    fn cancelled(&self) -> bool {
        false
    }
}

/// A host without UI that keeps the console lines, optionally echoing
/// them to stderr (`fluut run`).
#[derive(Debug, Default)]
pub struct Console {
    lines: Mutex<Vec<(Level, String)>>,
    echo: bool,
    cancel: AtomicBool,
}

impl Console {
    pub fn new() -> Console {
        Console::default()
    }

    /// Prints each line as it comes: info to stdout, others to stderr.
    pub fn echo() -> Console {
        Console {
            echo: true,
            ..Console::default()
        }
    }

    pub fn lines(&self) -> Vec<(Level, String)> {
        self.lines.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// The text of the lines, one per line.
    pub fn text(&self) -> String {
        self.lines().iter().map(|(_, l)| format!("{l}\n")).collect()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl ScriptHost for Console {
    fn log(&self, level: Level, msg: &str) {
        if self.echo {
            match level {
                Level::Info => println!("{msg}"),
                _ => eprintln!("{}: {msg}", level.as_str()),
            }
        }
        if let Ok(mut l) = self.lines.lock() {
            l.push((level, msg.to_owned()));
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}
