fn main() {
  big := 1000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000.0
  print([
    checked_add(1.5, 2.0),
    checked_mul(big, 10.0),
    checked_mul(big, big),
    checked_div(1.0, 0.0),
    checked_div(0.0, 0.0),
    checked_sub(0.5, 2.0),
    checked_rem(-5.5, 2.0),
    checked_rem(1.0, 0.0),
  ])
  print([checked_sqrt(2.0), checked_sqrt(-1.0), checked_sqrt(0.0)])
  print([
    checked_floor(-2.5),
    checked_ceil(-2.5),
    checked_round(2.5),
    checked_round(-0.5),
  ])
  let fs: List[Float] = [0.125, 2.675, -0.0, 999.9995]
  print([f"{fs[0]:.2}", f"{fs[1]:.2}", f"{fs[2]:.1}", f"{fs[3]:.3}"])
  print(sqrt(2.25))
  print(big * big)
}
