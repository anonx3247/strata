extern c type Counter

extern c fn neg8(x: I8) -> I8

extern c fn inc8(x: U8) -> U8

extern c fn umax() -> U64

extern c fn imin() -> I64

extern c fn half(x: U64) -> U64

extern c fn even(x: I32) -> Bool

extern c fn scale(x: Float, twice: Bool) -> Float

extern c fn shout(s: String) -> String

extern c fn counter_open(start: I32, mut c: Option[Counter]) -> I32

extern c fn counter_bump(c: Counter) -> I32

extern c fn counter_close(c: Counter)

extern c fn nothing() -> Counter

extern c fn twice(mut x: I32)

pub fn numbers() -> List[Int] {
  [
    neg8(5).to_int(),
    neg8(-128).to_int(),
    inc8(254).to_int(),
    umax().to_int(),
    imin().to_int(),
    half(umax()).to_int(),
    if even(4) {
      1
    } else {
      0
    },
  ]
}

pub fn texts() -> String {
  f"{shout("hello, c")} {scale(1.5, true)} {scale(1.5, false)}"
}

pub fn counting() -> List[Int] {
  mut c: Option[Counter] = None
  code := counter_open(41, mut c)
  mut x: I32 = 21
  twice(mut x)
  match c {
    Some(h) => {
      first := counter_bump(h)
      second := counter_bump(h)
      counter_close(h)
      [code.to_int(), first.to_int(), second.to_int(), x.to_int()]
    },
    None => [code.to_int()],
  }
}

pub fn refused() -> List[Int] {
  mut c: Option[Counter] = None
  code := counter_open(-1, mut c)
  [code.to_int(), if c == None {
    1
  } else {
    0
  }]
}

pub fn null_handle() -> Int {
  counter_bump(nothing()).to_int()
}

pub fn nul_string() -> String {
  shout("a" + chr(0) + "b")
}
