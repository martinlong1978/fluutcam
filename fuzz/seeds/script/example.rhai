//! fluut >= 0.1
let p = fluut.project;
let top = p.setups[0];
let holes = [];
for body in p.model.bodies {
    for f in body.faces {
        if f.kind == "cylinder" && f.params.radius <= 3.0 && f.params.axis_is_vertical {
            holes.push(f.id);
        }
    }
}
top.add_toolpath("drill", #{
    tool: "user/drills#d6",
    geometry: #{ drive_faces: holes },
    passes: #{ cycle: "peck", peck_depth: 2.0 },
});
fluut.log(`Added drilling for ${holes.len} holes`);
