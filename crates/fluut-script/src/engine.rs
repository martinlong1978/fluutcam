// SPDX-License-Identifier: MIT OR Apache-2.0

//! The sandboxed engine (D12 §1, D12b §8–§9): no file, network, process,
//! environment or clock access; limits on operations, depth, sizes and
//! script time; `eval` disabled; `import` only from the library.

use crate::host::{Level, ScriptHost};
use crate::perm::{LibraryResolver, Permissions};
use rhai::packages::{Package, StandardPackage};
use rhai::{Dynamic, Engine, FLOAT, INT};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Limits of one run. The defaults are never hit by normal jobs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Limits {
    pub operations: u64,
    pub call_levels: usize,
    /// Expression nesting at the top level and inside functions.
    pub expr_depth: usize,
    pub fn_expr_depth: usize,
    /// Characters in one string.
    pub string_len: usize,
    /// Elements of an array, nested arrays counted.
    pub array_len: usize,
    pub map_len: usize,
    pub variables: usize,
    pub modules: usize,
    /// Size of all variables together, in cells: one per array element or
    /// map entry, one per 16 bytes of string. Checked every few variable
    /// reads, since rhai checks a value's size only when it is passed or
    /// assigned whole.
    pub memory_cells: usize,
    /// Time spent in script code; calculation, post and import calls do
    /// not count.
    pub script_time: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            operations: 50_000_000,
            call_levels: 64,
            expr_depth: 64,
            fn_expr_depth: 32,
            string_len: 1 << 20,
            array_len: 1 << 20,
            map_len: 1 << 16,
            variables: 10_000,
            modules: 64,
            memory_cells: 16 << 20,
            script_time: Duration::from_secs(300),
        }
    }
}

/// Why a run was stopped from outside the script.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Stop {
    Cancelled,
    TimeLimit,
    Exit,
}

impl Stop {
    pub fn message(self) -> &'static str {
        match self {
            Stop::Cancelled => "the script was cancelled",
            Stop::TimeLimit => "the script ran longer than its time limit",
            Stop::Exit => "exit",
        }
    }
}

/// Run control shared by the engine's progress callback and the API:
/// cancellation, the script-time clock and the exit code.
#[derive(Debug)]
pub struct Control {
    host: Arc<dyn ScriptHost>,
    start: Instant,
    limit: Duration,
    /// Time spent in host calls (calculate, post, import).
    host_time: Mutex<Duration>,
    exited: AtomicBool,
    exit_code: AtomicI64,
    stop: Mutex<Option<Stop>>,
    memory_cells: usize,
    /// Variable reads until the next memory check.
    until_check: AtomicI64,
}

impl std::fmt::Debug for dyn ScriptHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScriptHost")
    }
}

impl Control {
    pub fn new(host: Arc<dyn ScriptHost>, limits: &Limits) -> Control {
        Control {
            host,
            start: Instant::now(),
            limit: limits.script_time,
            host_time: Mutex::new(Duration::ZERO),
            exited: AtomicBool::new(false),
            exit_code: AtomicI64::new(0),
            stop: Mutex::new(None),
            memory_cells: limits.memory_cells,
            until_check: AtomicI64::new(MEMORY_CHECK_EVERY),
        }
    }

    pub fn host(&self) -> &Arc<dyn ScriptHost> {
        &self.host
    }

    /// Runs `f` without counting its time against the script-time limit.
    pub fn host_call<T>(&self, f: impl FnOnce() -> T) -> T {
        let t = Instant::now();
        let r = f();
        if let Ok(mut h) = self.host_time.lock() {
            *h += t.elapsed();
        }
        r
    }

    /// Called on every variable read: every so often, the size of all
    /// variables in `scope` against the memory limit. The interval grows
    /// with the size found, so the check costs O(1) per read on average.
    pub fn check_memory<'a>(&self, values: impl Iterator<Item = &'a Dynamic>) -> Result<(), usize> {
        if self.until_check.fetch_sub(1, Ordering::Relaxed) > 0 {
            return Ok(());
        }
        let budget = self.memory_cells;
        let mut n = 0usize;
        for v in values {
            n = n.saturating_add(cells(v, budget.saturating_sub(n), 0));
            if n > budget {
                return Err(n);
            }
        }
        let next = (n as i64).max(MEMORY_CHECK_EVERY);
        self.until_check.store(next, Ordering::Relaxed);
        Ok(())
    }

    /// Records `fluut.exit(code)`.
    pub fn exit(&self, code: i64) {
        self.exit_code.store(code, Ordering::Relaxed);
        self.exited.store(true, Ordering::Relaxed);
    }

    pub fn exit_code(&self) -> Option<i64> {
        self.exited
            .load(Ordering::Relaxed)
            .then(|| self.exit_code.load(Ordering::Relaxed))
    }

    /// The reason the run was stopped, if it was.
    pub fn stopped(&self) -> Option<Stop> {
        self.stop.lock().ok().and_then(|s| *s)
    }

    /// Called by the engine every few operations. Once a stop is decided
    /// it holds: a script that catches an error still stops at its next
    /// operation.
    fn poll(&self, ops: u64) -> Option<Stop> {
        if let Some(s) = self.stopped() {
            return Some(s);
        }
        let s = if self.exited.load(Ordering::Relaxed) {
            Some(Stop::Exit)
        } else if self.host.cancelled() {
            Some(Stop::Cancelled)
        } else if ops.is_multiple_of(1024) && self.script_elapsed() > self.limit {
            Some(Stop::TimeLimit)
        } else {
            None
        };
        if let (Some(s), Ok(mut slot)) = (s, self.stop.lock()) {
            *slot = Some(s);
        }
        s
    }

    fn script_elapsed(&self) -> Duration {
        let h = self.host_time.lock().map(|h| *h).unwrap_or_default();
        self.start.elapsed().saturating_sub(h)
    }
}

/// Variable reads between memory checks, at least.
const MEMORY_CHECK_EVERY: i64 = 4096;

/// Cells of a value (see [`Limits::memory_cells`]), counting stops past
/// `budget`.
fn cells(v: &Dynamic, budget: usize, depth: usize) -> usize {
    if depth > 256 {
        return 1;
    }
    if v.is_shared() {
        return match v.read_lock::<Dynamic>() {
            Some(x) if !x.is_shared() => cells(&x, budget, depth + 1),
            _ => 1,
        };
    }
    if let Some(a) = v.read_lock::<rhai::Array>() {
        let mut n = a.len();
        for x in a.iter() {
            if n > budget {
                break;
            }
            n = n.saturating_add(cells(x, budget - n, depth + 1));
        }
        return n;
    }
    if let Some(m) = v.read_lock::<rhai::Map>() {
        let mut n = m.len();
        for (k, x) in m.iter() {
            if n > budget {
                break;
            }
            n = n.saturating_add(k.len() / 16 + cells(x, budget - n, depth + 1));
        }
        return n;
    }
    if let Some(s) = v.read_lock::<rhai::ImmutableString>() {
        return s.len() / 16;
    }
    if let Some(b) = v.read_lock::<rhai::Blob>() {
        return b.len() / 16;
    }
    0
}

/// The engine with the sandbox and the language packages, without the
/// FluutCAM API ([`crate::api`] adds it).
pub fn make_engine(limits: &Limits, perm: &Permissions, control: Arc<Control>) -> Engine {
    let mut e = Engine::new_raw();
    // Core, arithmetic, logic, strings, arrays, maps, iterators, math and
    // the function package; `no_time` leaves out `timestamp`.
    e.register_global_module(StandardPackage::new().as_shared_module());
    e.disable_symbol("eval");
    e.set_module_resolver(LibraryResolver::new(&perm.library_dirs));
    e.set_max_operations(limits.operations);
    e.set_max_call_levels(limits.call_levels);
    e.set_max_expr_depths(limits.expr_depth, limits.fn_expr_depth);
    e.set_max_string_size(limits.string_len);
    e.set_max_array_size(limits.array_len);
    e.set_max_map_size(limits.map_len);
    e.set_max_variables(limits.variables);
    e.set_max_modules(limits.modules);
    // Nothing a script can do should fill the interner.
    e.set_max_strings_interned(1024);
    // Operators may be overridden (`**` below).
    e.set_fast_operators(false);
    let c = control.clone();
    e.on_progress(move |ops| c.poll(ops).map(|s| Dynamic::from(s.message())));
    let c = control.clone();
    e.on_print(move |s| c.host().log(Level::Info, s));
    let c = control;
    e.on_debug(move |s, _, _| c.host().log(Level::Debug, s));
    register_libm(&mut e);
    e
}

/// Transcendental functions from `libm`, so results are the same on every
/// platform (D01 §6). Registered functions take precedence over the
/// standard package's.
fn register_libm(e: &mut Engine) {
    type Unary = fn(f64) -> f64;
    const UNARY: [(&str, Unary); 16] = [
        ("sin", libm::sin),
        ("cos", libm::cos),
        ("tan", libm::tan),
        ("sinh", libm::sinh),
        ("cosh", libm::cosh),
        ("tanh", libm::tanh),
        ("asin", libm::asin),
        ("acos", libm::acos),
        ("atan", libm::atan),
        ("asinh", libm::asinh),
        ("acosh", libm::acosh),
        ("atanh", libm::atanh),
        ("exp", libm::exp),
        ("ln", libm::log),
        ("log", libm::log10),
        ("sqrt", libm::sqrt),
    ];
    for (name, f) in UNARY {
        e.register_fn(name, move |x: FLOAT| f(x));
        e.register_fn(name, move |x: INT| f(x as f64));
    }
    e.register_fn("atan", |y: FLOAT, x: FLOAT| libm::atan2(y, x));
    e.register_fn("hypot", |x: FLOAT, y: FLOAT| libm::hypot(x, y));
    e.register_fn("log", |x: FLOAT, base: FLOAT| {
        libm::log(x) / libm::log(base)
    });
    e.register_fn("**", |x: FLOAT, y: FLOAT| libm::pow(x, y));
    e.register_fn("**", |x: FLOAT, y: INT| libm::pow(x, y as f64));
    e.register_fn("**", |x: INT, y: FLOAT| libm::pow(x as f64, y));
    e.register_fn("**=", |x: &mut FLOAT, y: FLOAT| *x = libm::pow(*x, y));
    e.register_fn("**=", |x: &mut FLOAT, y: INT| *x = libm::pow(*x, y as f64));
}
