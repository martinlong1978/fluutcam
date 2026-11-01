let p = fluut.project;
p.name = "x"; print(p.units);
let s = p.setup("Top");
print(s.stock); print(s.zero); print(s.origin("material"));
s.stock = #{ kind: "block", margin: ["0.25in", "5mm", 0] };
let c = p.add_setup(#{ name: "C", based_on: "s1" });
c.material = "Oak"; c.reset("material"); print(c.is_inherited("material"));
let t = p.toolpath("t1");
print(t.passes.keys()); print(t.passes.to_map());
t.passes.stepover = "0.1in"; t.linking.ramp.angle_deg = 5;
t.passes = #{ step_down: 3.0 };
let n = c.add_toolpath("contour_2d", #{ tool: "shop#flat-6", passes: #{ step_down: 2 } });
print(n.state); print(n.result_summary); print(n.diagnostics);
try { n.passes.nope = 1 } catch (e) { print(e) }
n.remove(); c.remove();
print(fluut.library.posts); print(fluut.library.tool("shop#flat-6"));
try { fluut.ui.message("x") } catch (e) { print(e) }
try { fluut.read_csv("x.csv") } catch (e) { print(e) }
fluut.exit(0);
