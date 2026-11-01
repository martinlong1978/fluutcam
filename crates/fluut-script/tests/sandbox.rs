// SPDX-License-Identifier: MIT OR Apache-2.0

//! The sandbox (spec 09 §9.3, D12 §6): every limit stops a hostile
//! script; no `eval`, no clock; `import` only from the library; paths
//! only inside the permissions.

mod support;

use fluut_script::perm::{LibraryResolver, real_path};
use fluut_script::{Access, Console, ErrorKind, Permissions, RunOptions, ScriptHost, run};
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::*;

fn limit(src: &str) -> String {
    let (o, c) = bare(src, small());
    assert_eq!(
        kind(&o),
        Some(ErrorKind::Limit),
        "{src}: {:?} {}",
        o.result,
        c.text()
    );
    assert_eq!(o.exit_code, 1);
    o.result.unwrap_err().message
}

#[test]
fn infinite_loops_stop() {
    let m = limit("loop {}");
    assert!(m.contains("operations"), "{m}");
    limit("let x = 0; while true { x += 1; }");
    // Catching the error does not help: the next operation stops again.
    limit("loop { try { loop {} } catch (e) { } }");
    limit("fn f() { loop { try { g() } catch {} } } fn g() { loop {} } f()");
}

#[test]
fn deep_recursion_stops() {
    let m = limit("fn f(n) { f(n + 1) } f(0)");
    assert!(m.contains("tack") || m.contains("call"), "{m}");
    // Deep nesting in the text is a parse error, not a stack overflow.
    let src = format!("{}1{}", "(".repeat(5000), ")".repeat(5000));
    let (o, _) = bare(&src, small());
    assert_eq!(kind(&o), Some(ErrorKind::Parse));
    let src = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    let (o, _) = bare(&src, small());
    assert_eq!(kind(&o), Some(ErrorKind::Parse));
}

#[test]
fn huge_values_stop() {
    let m = limit(r#"let s = "x"; loop { s += s; }"#);
    assert!(m.contains("too large") || m.contains("Length"), "{m}");
    limit(r#"let s = "x"; s.pad(1000000, "y");"#);
    limit("let a = [1]; loop { a += a; }");
    limit("let a = []; a.pad(1000000, 0);");
    // Nested arrays count every element.
    limit(
        "let a = []; for i in 0..200 { a.push([0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]); }",
    );
    limit("let m = #{}; for i in 0..5000 { m[`k${i}`] = i; } print(m.len());");
    // Growing a map by index is checked by the memory limit even when the
    // map is never passed anywhere.
    let mut lim = small();
    lim.memory_cells = 5_000;
    let (o, _) = bare(
        "let m = #{}; let i = 0; loop { m[`k${i}`] = i; i += 1; }",
        lim,
    );
    assert_eq!(kind(&o), Some(ErrorKind::Limit));
    let m = o.result.unwrap_err().message;
    assert!(m.contains("variables"), "{m}");
    limit(
        "let a = []; let b = #{}; let i = 0; loop { b[`k${i}`] = [1, 2, 3]; i += 1; if i % 900 == 0 { a.push(b); b = #{}; } }",
    );
}

#[test]
fn no_eval_no_clock_no_io() {
    for src in [
        r#"eval("1 + 1")"#,
        "timestamp()",
        r#"open_file("/etc/passwd")"#,
        "read_file(\"x\")",
        "system(\"ls\")",
    ] {
        let (o, _) = bare(src, small());
        assert!(o.result.is_err(), "{src}");
    }
    let (o, _) = bare(r#"eval("1")"#, small());
    assert_eq!(kind(&o), Some(ErrorKind::Parse));
}

#[test]
fn libm_math_and_ordered_maps() {
    let (o, c) = bare(
        "print(sin(0.5)); print(2.0 ** 0.5); print(atan(1.0, 2.0)); \
         let x = 3.0; x **= 0.5; print(x); \
         let m = #{ b: 1, a: 2, c: 3 }; print(m.keys());",
        small(),
    );
    ok(&o, &c);
    let lines: Vec<String> = c.lines().into_iter().map(|(_, l)| l).collect();
    assert_eq!(lines[0], format!("{}", libm::sin(0.5)));
    assert_eq!(lines[1], format!("{}", libm::pow(2.0, 0.5)));
    assert_eq!(lines[2], format!("{}", libm::atan2(1.0, 2.0)));
    assert_eq!(lines[3], format!("{}", libm::pow(3.0, 0.5)));
    assert_eq!(lines[4], r#"["a", "b", "c"]"#);
}

#[test]
fn print_debug_log_and_exit() {
    let (o, c) = bare(
        r#"print("p"); debug("d"); fluut.log(1); fluut.warn("w"); fluut.exit(3); print("never");"#,
        small(),
    );
    ok(&o, &c);
    assert_eq!(o.exit_code, 3);
    let t = c.text();
    assert!(t.contains("p\n") && t.contains("\"d\"") && t.contains("1\n") && t.contains("w\n"));
    assert!(!t.contains("never"));
    // `exit` from inside a function, and caught exits still exit.
    let (o, _) = bare("fn f() { exit(4) } try { f() } catch {} loop {}", small());
    assert_eq!((o.exit_code, o.result.is_ok()), (4, true));
    let (o, _) = bare("fluut.exit(300)", small());
    assert_eq!(kind(&o), Some(ErrorKind::Runtime));
}

#[test]
fn version_header() {
    let opts = |v: &str| RunOptions {
        version: v.into(),
        ..RunOptions::default()
    };
    let c = Arc::new(Console::new());
    let src = "//! fluut >= 1.2\nprint(fluut.version);";
    let o = run(src, None, opts("1.1.9"), c.clone());
    let e = o.result.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Version);
    assert!(
        e.message.contains("1.2") && e.message.contains("1.1.9"),
        "{e}"
    );
    let o = run(src, None, opts("1.10"), c.clone());
    assert!(o.result.is_ok());
    let o = run("//! fluut >= banana\n", None, opts("1.0"), c.clone());
    assert_eq!(o.result.unwrap_err().kind, ErrorKind::Parse);
    let o = run("fluut.require(\"2\")", None, opts("1.0"), c.clone());
    assert_eq!(o.result.unwrap_err().kind, ErrorKind::Runtime);
    let o = run(
        "try { fluut.require(\"2\") } catch (e) { print(e) }",
        None,
        opts("1.0"),
        c.clone(),
    );
    assert!(o.result.is_ok());
    // Other doc comments are not headers.
    let o = run("//! a library\nprint(1);", None, opts("1.0"), c);
    assert!(o.result.is_ok());
}

#[derive(Default)]
struct Slow(std::sync::atomic::AtomicU64);

impl ScriptHost for Slow {
    fn log(&self, _: fluut_script::Level, _: &str) {}
    fn cancelled(&self) -> bool {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) > 5000
    }
}

#[test]
fn cancellation_and_the_time_limit() {
    let mut lim = small();
    lim.operations = u64::MAX;
    let o = run(
        "loop {}",
        None,
        opts(Permissions::default(), lim.clone()),
        Arc::new(Slow::default()),
    );
    assert_eq!(kind(&o), Some(ErrorKind::Cancelled));
    lim.script_time = Duration::from_millis(200);
    let t = Instant::now();
    let (o, _) = bare("loop { try { loop {} } catch {} }", lim);
    assert_eq!(kind(&o), Some(ErrorKind::TimeLimit), "{:?}", o.result);
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[test]
fn import_reads_the_library_only() {
    let d = tmp("import");
    let lib = d.join("lib");
    std::fs::create_dir_all(lib.join("scripts/sub")).unwrap();
    std::fs::write(
        lib.join("scripts/util.rhai"),
        "fn twice(x) { x * 2 } export const K = 7;",
    )
    .unwrap();
    std::fs::write(
        lib.join("scripts/sub/deep.rhai"),
        "fn v() { fluut.version }",
    )
    .unwrap();
    std::fs::write(d.join("secret.rhai"), "fn s() { 1 }").unwrap();
    std::fs::write(lib.join("scripts/loop.rhai"), "loop {}").unwrap();
    let perm = Permissions {
        library_dirs: vec![lib.clone()],
        ..Permissions::default()
    };
    let c = Arc::new(Console::new());
    let o = run(
        r#"import "util" as u; import "sub/deep" as d; print(u::twice(u::K)); print(d::v());"#,
        None,
        opts(perm.clone(), small()),
        c.clone(),
    );
    ok(&o, &c);
    assert!(c.text().starts_with("14\n"), "{}", c.text());
    for bad in [
        "../secret",
        "/etc/passwd",
        "sub/../../secret",
        "",
        "nope",
        "c:x",
        "a\\b",
    ] {
        let o = run(
            &format!("import {bad:?} as m;"),
            None,
            opts(perm.clone(), small()),
            c.clone(),
        );
        let e = o.result.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Runtime, "{bad}: {e:?}");
    }
    // A module's own loop is still limited.
    let o = run(
        r#"import "loop" as l;"#,
        None,
        opts(perm.clone(), small()),
        c.clone(),
    );
    assert_eq!(kind(&o), Some(ErrorKind::Limit));
    // Without library folders nothing can be imported.
    let (o, _) = bare(r#"import "util" as u;"#, small());
    assert!(o.result.is_err());
    let r = LibraryResolver::new(&[lib]);
    assert!(r.find("util").is_ok() && r.find("../secret").is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_do_not_escape() {
    let d = tmp("symlink");
    let lib = d.join("lib");
    std::fs::create_dir_all(lib.join("scripts")).unwrap();
    std::fs::write(d.join("outside.rhai"), "fn s() { 1 }").unwrap();
    std::os::unix::fs::symlink(d.join("outside.rhai"), lib.join("scripts/link.rhai")).unwrap();
    assert!(LibraryResolver::new(&[lib]).find("link").is_err());
    let proj = d.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::os::unix::fs::symlink(&d, proj.join("up")).unwrap();
    let p = Permissions {
        project_dir: Some(proj.clone()),
        ..Permissions::default()
    };
    assert!(p.check("up/outside.rhai", None, Access::Read).is_err());
}

#[test]
fn path_checks() {
    let d = tmp("paths");
    let proj = d.join("proj");
    let lib = d.join("lib");
    let arg = d.join("out.ngc");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::create_dir_all(&lib).unwrap();
    let p = Permissions {
        project_dir: Some(proj.clone()),
        library_dirs: vec![lib.clone()],
        allowed_paths: vec![arg.clone()],
        ui: false,
    };
    let ok = |s: &str, a| p.check(s, None, a).is_ok();
    assert!(ok("part.step", Access::Read));
    assert!(ok("sub/new/x.ngc", Access::Write));
    assert!(ok(&arg.to_string_lossy(), Access::Write));
    assert!(ok(
        &lib.join("posts/a.toml").to_string_lossy(),
        Access::Read
    ));
    // The library is read only.
    assert!(!ok(
        &lib.join("posts/a.toml").to_string_lossy(),
        Access::Write
    ));
    assert!(!ok("../out2.ngc", Access::Write));
    assert!(!ok("../lib/../proj/../x", Access::Read));
    assert!(ok("./a/../b", Access::Read));
    assert!(!ok("/etc/passwd", Access::Read));
    let e = p.check("../x", None, Access::Write).unwrap_err();
    assert!(e.contains("permission denied"), "{e}");
    // Nothing is allowed without permissions.
    assert!(
        Permissions::default()
            .check("x", None, Access::Read)
            .is_err()
    );
    assert_eq!(real_path(std::path::Path::new("/a/../..")), None);
}

#[test]
fn headless_ui_errors_can_be_caught() {
    let (o, c) = bare(
        r#"try { fluut.ui.message("hi") } catch (e) { print(e) }
           fluut.ui.ask_number("x", 1, "mm");"#,
        small(),
    );
    assert!(c.text().contains("no UI in headless mode"));
    assert_eq!(kind(&o), Some(ErrorKind::Runtime));
}

#[test]
fn errors_have_positions() {
    let (o, _) = bare("let x = 1;\nlet y = x +;\n", small());
    let e = o.result.unwrap_err();
    assert_eq!((e.kind, e.line), (ErrorKind::Parse, Some(2)));
    let (o, _) = bare("let x = 1;\n\nthrow \"boom\";", small());
    let e = o.result.unwrap_err();
    assert_eq!((e.kind, e.line), (ErrorKind::Runtime, Some(3)));
    assert!(e.message.contains("boom"));
    let (o, _) = bare("fluut.project", small());
    assert!(o.result.unwrap_err().message.contains("no project is open"));
}
