// SPDX-License-Identifier: MIT OR Apache-2.0

//! The D15b §7 validation codes of `manual_nc`, one passing and one
//! failing case each. `param.manual_nc.post` needs the post and is tested
//! in `fluut-cli`.

use fluut_model::project::{SetupKind, Toolpath, read};
use fluut_model::resolve::{ManualKind, resolve};
use fluut_model::tools::ToolLibrary;
use fluut_model::validate::{ValidateCx, validate};

fn tp(passes: &str) -> Toolpath {
    let src = format!(
        "format = \"fluut.project/1\"\n[[setup]]\nid = \"s\"\n[[setup.toolpath]]\n\
         id = \"t\"\ntype = \"manual_nc\"\n[setup.toolpath.passes]\n{passes}"
    );
    read(&src)
        .unwrap_or_else(|e| panic!("{e}\n{src}"))
        .project
        .setups[0]
        .toolpaths[0]
        .clone()
}

fn codes(t: &Toolpath, kind: SetupKind) -> Vec<String> {
    let cx = ValidateCx {
        setup_kind: kind,
        ..ValidateCx::new(None)
    };
    validate(t, &cx).into_iter().map(|d| d.code).collect()
}

fn case(code: &str, pass: &str, fail: &str) {
    let p = codes(&tp(pass), SetupKind::ThreeAxis);
    assert!(!p.contains(&code.to_owned()), "{code}: {p:?}");
    let f = codes(&tp(fail), SetupKind::ThreeAxis);
    assert!(f.contains(&code.to_owned()), "{code}: {f:?}");
}

#[test]
fn no_tool_and_every_setup_kind() {
    // No tool: `param.tool.missing` does not apply (D15b §7).
    for k in [SetupKind::ThreeAxis, SetupKind::Rotary, SetupKind::Indexed] {
        assert_eq!(codes(&tp("kind = \"stop\"\n"), k), Vec::<String>::new());
    }
}

#[test]
fn manual_nc_codes() {
    case(
        "param.manual_nc.text.required",
        "kind = \"comment\"\ntext = \"Check clamps\"\n",
        "kind = \"comment\"\n",
    );
    case(
        "param.manual_nc.text.required",
        "kind = \"pass_through\"\ntext = \"M8\"\n",
        "kind = \"pass_through\"\ntext = \"\"\n",
    );
    case(
        "param.manual_nc.text.unused",
        "kind = \"dwell\"\n",
        "kind = \"dwell\"\ntext = \"wait\"\n",
    );
    case(
        "param.manual_nc.dwell.range",
        "kind = \"dwell\"\ndwell_s = 0.1\n",
        "kind = \"dwell\"\ndwell_s = 0\n",
    );
}

#[test]
fn resolves_defaults() {
    let lib = ToolLibrary::parse(
        "format = \"fluut.tools/1\"\nid = \"l\"\n[[tool]]\nid = \"x\"\nshape = \"flat\"\ndiameter = 1\n",
    )
    .unwrap()
    .0;
    let r = resolve(&tp("").params, &lib.tools[0], None);
    let m = r.manual_nc().unwrap();
    assert_eq!(m.kind, ManualKind::OptionalStop);
    assert_eq!(m.dwell_s, 1.0);
    assert_eq!(m.text, None);
}
