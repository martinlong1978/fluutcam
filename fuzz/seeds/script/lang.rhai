fn fib(n) { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }
let a = [1, 2.5, "s", #{ k: [true, ()] }];
let m = #{ b: 1, a: 2 };
for (x, i) in a { print(`${i}: ${x}`); }
let f = |x| x * 2;
print(f.call(fib(10)));
print(sin(0.5) + 2.0 ** 0.5 + atan(1.0, 2.0));
switch m.a { 2 => print("two"), _ => () }
let s = "abc"; s += s; print(s.len); print(m.keys());
try { throw "e" } catch (e) { debug(e) }
import "nothing" as n;
