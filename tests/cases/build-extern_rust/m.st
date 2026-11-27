from std.json use Json

from std.rust use RustError

pub type Plain = { x: Int }

derive Json for Plain

type Natural = Int where it >= 0

extern rust "worker" fn inc(x: Int) -> Result[Int, RustError]

extern rust "worker" fn plain(n: Int) -> Result[Plain, RustError]

extern rust "worker" fn double(p: Plain) -> Result[Plain, RustError]

extern rust "worker" fn wrong() -> Result[Plain, RustError]

extern rust "worker" fn negative() -> Result[Natural, RustError]

extern rust "worker" fn big() -> Result[Int, RustError]

extern rust "worker" fn noisy() -> Result[(), RustError]

extern rust "worker" fn parsed(s: String) -> Result[Int, RustError]

extern rust "worker" fn panics(x: Int) -> Result[Int, RustError]

extern rust "worker" fn takes_int(x: String) -> Result[Int, RustError]

extern rust "worker::words" fn count(s: String) -> Result[Int, RustError]

extern rust "worker" fn huge(x: Int) -> Result[Int, RustError]

pub capability Worker {
  fn inc(x: Int) -> Result[Int, RustError] {
    inc(x)
  }

  fn all() -> List[String] {
    [
      f"{inc(41)}",
      f"{plain(3)}",
      f"{double(Plain { x: 21 })}",
      f"{wrong()}",
      f"{negative()}",
      f"{big()}",
      f"{noisy()}",
      f"{parsed("12")}",
      f"{parsed("twelve")}",
      f"{panics(0)}",
      f"{panics(7)}",
      f"{panics(0)}",
      f"{takes_int("one")}",
      f"{count("three words here")}",
      f"{huge(10000000000000000)}",
    ]
  }

  fn many() -> List[Result[Int, RustError]] {
    scope {
      await [spawn inc(k) for k in 0..20]
    }
  }
}
