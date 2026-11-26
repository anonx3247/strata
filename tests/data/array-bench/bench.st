// std.array against NumPy (bench.py) and C (bench.c). Build:
// `strata --file bench.strata edit < bench.st && strata --file bench.strata build --release -o bench && ./bench`.
// Each line: the best of 5 runs, in microseconds. `local` is the same loop compiled in the program, where `verify` leaves its proven checks out.
from std.io use Console
from std.time use SystemClock, elapsed
from std.array use Array, ArrayI64, ArrayI32, Length

/// The fastest of five runs of `f`, in microseconds.
fn best(clock: SystemClock, f: () -> Int) -> Int {
  mut b := 1000000000000
  mut r := 0
  mut keep := 0
  while r < 5 {
    t0 := clock.now()
    keep += f()
    t1 := clock.now()
    b = min(b, elapsed(t0, t1).value / 1000)
    r += 1
  }
  if keep == 7 {
    b + 1
  } else {
    b
  }
}

/// `std.array`'s `sum_run_f64`, compiled in the program.
fn local_sum(xs: List[Float]) -> Float {
  nx := len(xs).to_i64()
  mut a: I64 = 0
  mut s0 := 0.0
  mut s1 := 0.0
  mut s2 := 0.0
  mut s3 := 0.0
  while a >= 0 && a < nx - 3 {
    s0 += xs[a]
    s1 += xs[a + 1]
    s2 += xs[a + 2]
    s3 += xs[a + 3]
    a += 4
  }
  while a >= 0 && a < nx {
    s0 += xs[a]
    a += 1
  }
  (s0 + s1) + (s2 + s3)
}

fn main(mut console: Console, clock: SystemClock) {
  let n: Length = 10000000
  f := Array.arange(n)
  i := ArrayI64.arange(n)
  j := ArrayI32.arange(n)
  fl := f.to_list()
  mut console.print(f"float64 sum {best(clock, () -> round(f.sum()) % 2)} us, local {best(clock, () -> round(local_sum(fl)) % 2)} us")
  mut console.print(f"int64 sum {best(clock, () -> i.sum().to_int() % 2)} us")
  mut console.print(f"int32 sum {best(clock, () -> j.sum().to_int() % 2)} us")
  mut console.print(f"float64 add {best(clock, () -> f.add(f).size().to_int() % 2)} us")
  mut console.print(f"int64 add {best(clock, () -> i.add(i).size().to_int() % 2)} us")
  mut console.print(f"int32 add {best(clock, () -> j.add(j).size().to_int() % 2)} us")
  fa := Array.arange(65536).reshape([256, 256]).map(x -> float(round(x) % 5))
  fb := Array.arange(65536).reshape([256, 256]).map(x -> float(round(x) % 3))
  ia := ArrayI64.arange(65536).reshape([256, 256]).map(x -> x % 5)
  ib := ArrayI64.arange(65536).reshape([256, 256]).map(x -> x % 3)
  ja := ArrayI32.arange(65536).reshape([256, 256]).map(x -> x % 5)
  jb := ArrayI32.arange(65536).reshape([256, 256]).map(x -> x % 3)
  mut console.print(f"float64 matmul {best(clock, () -> round(fa.matmul(fb).sum()) % 2)} us")
  mut console.print(f"int64 matmul {best(clock, () -> ia.matmul(ib).sum().to_int() % 2)} us")
  mut console.print(f"int32 matmul {best(clock, () -> ja.matmul(jb).sum().to_int() % 2)} us")
}
