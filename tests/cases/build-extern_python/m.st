from std.json use Json

from std.python use PythonError

pub type Plain = { x: Int }

derive Json for Plain

type Natural = Int where it >= 0

extern python "worker" fn inc(x: Int) -> Result[Int, PythonError]

extern python "worker" fn die(code: Int) -> Result[Int, PythonError]

extern python "worker" fn nan() -> Result[Float, PythonError]

extern python "worker" fn big() -> Result[Int, PythonError]

extern python "worker" fn plain(n: Int) -> Result[Plain, PythonError]

extern python "worker" fn wrong() -> Result[Plain, PythonError]

extern python "worker" fn negative() -> Result[Natural, PythonError]

extern python "worker" fn noisy() -> Result[(), PythonError]

extern python "worker" fn unknown_value() -> Result[Int, PythonError]

extern python "worker" fn missing() -> Result[Int, PythonError]

extern python "nowhere" fn gone() -> Result[Int, PythonError]

extern python "worker" fn huge(x: Int) -> Result[Int, PythonError]

extern python "worker" fn checked(x: Int) -> Result[Int, PythonError]
  requires x > 0

pub capability Worker {
  fn inc(x: Int) -> Result[Int, PythonError] {
    inc(x)
  }

  fn all() -> List[String] {
    [
      f"{inc(41)}",
      f"{plain(3)}",
      f"{nan()}",
      f"{big()}",
      f"{wrong()}",
      f"{negative()}",
      f"{noisy()}",
      f"{unknown_value() is Err}",
      f"{missing() is Err}",
      f"{gone() is Err}",
      f"{huge(10000000000000000)}",
      f"{die(3)}",
      f"{inc(1)}",
    ]
  }

  fn many() -> List[Result[Int, PythonError]] {
    scope {
      await [spawn inc(k) for k in 0..20]
    }
  }
}
