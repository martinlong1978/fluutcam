// SPDX-License-Identifier: MIT OR Apache-2.0

//! A script builds the `block` reference project's toolpaths, calculates
//! them and posts: the same G-code as the TOML project (D12 §6).

mod support;

use fluut_model::project::write;
use support::*;

/// The three toolpaths of `block.fluut`, built by a script.
const BUILD: &str = r#"
let p = fluut.project;
let top = p.setup("Top");
for t in top.toolpaths { t.remove(); }
print(top.toolpaths.len);

let body = p.model.body("pocket_block");
// The pocket floor: the one upward plane below the top.
let top_z = body.bounds.max[2];
let floor = body.faces.filter(|f| f.kind == "plane"
    && f.params.normal_is_vertical && f.params.normal[2] > 0.0 && f.bounds.max[2] < top_z);
print(floor.len);

let pocket = top.add_toolpath("pocket_2d", #{
    id: "t1", name: "Pocket", tool: "shop#flat-6",
    tool_params: #{ coolant: "flood" },
    geometry: #{ drive_faces: floor },
    passes: #{ stepover: 2.4, step_down: 4.0, stock_to_leave: 0.2, stock_to_leave_axial: 0.1 },
});
pocket.linking.ramp = #{ kind: "helix", angle_deg: 3, radius: 2.5 };

let raster = top.add_toolpath("pocket_2d", #{
    id: "t2", name: "Pocket floor raster", tool: "shop#flat-6",
    geometry: #{ drive_faces: [floor[0].id] },
    heights: #{ top: #{ ref: "selection", offset: 0.1 } },
    passes: #{ pattern: "raster", stepover: 3.0, pass_angle_deg: 45 },
    linking: #{ retract: "stay_down", ramp: #{ kind: "zigzag" } },
});

let walls = ["f-494.0", "f-494.1", "f-494.2", "f-494.3"];
let cut = top.add_toolpath("contour_2d", #{
    id: "t3", name: "Cut out", tool: "shop#flat-6",
    geometry: #{ drive_faces: walls },
});
cut.heights.bottom = #{ ref: "stock_bottom", offset: -0.2 };
cut.passes = #{ step_down: 5.0, tabs: #{ mode: "count", count: 4, width: 6, height: 3 } };
cut.linking = #{
    lead_in: #{ kind: "arc", radius: 4, length: 2 },
    ramp: #{ kind: "profile", angle_deg: 5 },
};

for t in p.toolpaths { print(`${t.id} ${t.state}`); }
for r in p.calculate_all() { print(`${r.toolpath} ${r.state} ${r.diagnostics.map(|d| d.code)}`); }
for t in p.toolpaths { print(`${t.id} ${t.state} ${t.result_summary.cut_length > 0.0}`); }
let out = p.post("all", "out.ngc", #{ post: "fluut/linuxcnc" });
print(out.files.len);
// Inputs changed after calculation: stale.
cut.passes.step_down = 4.0;
print(cut.state);
"#;

#[test]
fn script_posts_the_same_gcode_as_the_project() {
    let (s, dir, perm) = open("block", "post_block");
    let file = std::fs::read_to_string(dir.join("block.fluut")).unwrap();
    let (o, c) = on(BUILD, s, perm);
    ok(&o, &c);
    let lines: Vec<String> = c.lines().into_iter().map(|(_, l)| l).collect();
    assert_eq!(&lines[..2], ["0", "1"], "{lines:?}");
    assert_eq!(
        &lines[2..11],
        [
            "t1 not_calculated",
            "t2 not_calculated",
            "t3 not_calculated",
            "t1 calculated []",
            "t2 calculated []",
            // The two warnings `fluut post` reports for block.fluut.
            "t3 calculated [\"param.heights.below_stock\", \"cam.linking.ramp_fallback\"]",
            "t1 done true",
            "t2 done true",
            "t3 done true",
        ]
    );
    assert_eq!(&lines[11..], ["1", "stale"]);
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fluut-cli/tests/golden/block-linuxcnc.ngc");
    let want = std::fs::read_to_string(golden).unwrap();
    let got = std::fs::read_to_string(dir.join("out.ngc")).unwrap();
    assert!(got == want, "G-code differs from the golden file");
    // The document is the TOML project, apart from the last edit.
    let mut s = o.session.unwrap();
    s.apply(fluut_model::edit::Edit::set_toolpath_field(
        "t3",
        "passes.step_down",
        Some("5.0"),
    ))
    .unwrap();
    assert_eq!(write(&s.project), file);
}

#[test]
fn failed_calculations_and_post_errors() {
    let (s, _, perm) = open("block", "post_errors");
    let (o, c) = on(
        r#"
        let t = fluut.project.toolpath("t1");
        t.tool = "shop#nope";
        let r = t.calculate();
        print(r.state); print(t.state); print(t.diagnostics[0].severity);
        print(t.result_summary);
        try { fluut.project.post("all", "x.ngc") } catch (e) { print("needs a post") }
        try { fluut.project.post("all", "x.ngc", #{ post: "fluut/linuxcnc" }) } catch (e) { print(e) }
        try { fluut.project.post("some", "x.ngc", #{ post: "fluut/linuxcnc" }) } catch (e) { print(e) }
        try { fluut.project.post("all", "x.ngc", #{ colour: 1 }) } catch (e) { print(e) }
        t.tool = "shop#flat-6";
        print(t.state);
        t.enabled = false;
        print(fluut.project.calculate_all()[0].state);
        let r = fluut.project.post(fluut.project.setups[0], "one.ngc", #{ post: "fluut/grbl", date: "d", time: "t" });
        print(r.files.len);
        "#,
        s,
        perm,
    );
    ok(&o, &c);
    let l: Vec<String> = c.lines().into_iter().map(|(_, l)| l).collect();
    assert_eq!(&l[..5], ["failed", "error", "error", "", "needs a post"]);
    assert!(
        l[5].contains("post failed") && l[5].contains("tool"),
        "{}",
        l[5]
    );
    assert!(l[6].contains("\"all\""), "{}", l[6]);
    assert!(l[7].contains("colour"), "{}", l[7]);
    assert_eq!(&l[8..], ["not_calculated", "disabled", "1"]);
}
