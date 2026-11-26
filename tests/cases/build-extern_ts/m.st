from std.json use Json

from std.ts use TsError

pub type Plain = { x: Int }

derive Json for Plain

type Natural = Int where it >= 0

extern ts "worker" fn inc(x: Int) -> Result[Int, TsError]

extern ts "worker" fn die(code: Int) -> Result[Int, TsError]

extern ts "worker" fn nan() -> Result[Float, TsError]

extern ts "worker" fn big() -> Result[Int, TsError]

extern ts "worker" fn plain(n: Int) -> Result[Plain, TsError]

extern ts "worker" fn wrong() -> Result[Plain, TsError]

extern ts "worker" fn negative() -> Result[Natural, TsError]

extern ts "worker" fn noisy() -> Result[(), TsError]

extern ts "worker" fn bigint() -> Result[Int, TsError]

extern ts "worker" fn later(x: Int) -> Result[Int, TsError]

extern ts "worker" fn thrown() -> Result[Int, TsError]

extern ts "worker" fn missing() -> Result[Int, TsError]

extern ts "nowhere" fn gone() -> Result[Int, TsError]

extern ts "worker" fn huge(x: Int) -> Result[Int, TsError]

pub capability Worker {
  fn inc(x: Int) -> Result[Int, TsError] {
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
      f"{bigint()}",
      f"{later(21)}",
      match thrown() {
        Err(Thrown { name, message, .. }) => f"{name}: {message}",
        r => f"{r}",
      },
      match missing() {
        Err(Thrown { message, .. }) => message,
        r => f"{r}",
      },
      f"{gone() is Err}",
      f"{huge(10000000000000000)}",
      f"{die(3)}",
      f"{inc(1)}",
    ]
  }

  fn many() -> List[Result[Int, TsError]] {
    scope {
      await [spawn inc(k) for k in 0..20]
    }
  }
}
