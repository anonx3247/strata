// std.array against C (bench.c): build with `strata --file bench.strata build --release`, see IDEAS.md "std.array"
from std.io use Console
from std.time use SystemClock, elapsed
from std.array use Array, arange, matmul

fn main(mut console: Console, clock: SystemClock) {
  xs := arange(10000000)
  t0 := clock.now()
  mut s := 0.0
  mut r := 0
  while r < 10 {
    s = s + xs.sum()
    r += 1
  }
  t1 := clock.now()
  mut console.print(f"array sum {s} {elapsed(t0, t1).value / 10000} us per 10M")
  a := arange(65536).reshape([256, 256]).map(x -> float(round(x) % 5) * 0.25)
  b := arange(65536).reshape([256, 256]).map(x -> float(round(x) % 3) * 0.5)
  t2 := clock.now()
  mut c := matmul(a, b)
  mut q := 1
  while q < 10 {
    c = matmul(a, b)
    q += 1
  }
  t3 := clock.now()
  mut console.print(f"matmul {c.sum()} {elapsed(t2, t3).value / 10000} us")
  t4 := clock.now()
  d := a.add(b)
  t5 := clock.now()
  mut console.print(f"add 65536 {d.sum()} {elapsed(t4, t5).value / 1000} us")
}
