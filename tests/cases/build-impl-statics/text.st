type Money = { cents: Int }

impl Display for Money {
  fn to_string(self) -> String {
    f"${self.cents / 100}.{self.cents % 100:02}"
  }
}

type Shape =
  | Dot
  | Rect(Int, Int)
  | Circle { r: Float, label: String }

distinct type Id = Int

type Secret = { key: String }

impl Debug for Secret {
  fn debug(self) -> String {
    "Secret { key: *** }"
  }
}

fn show_all[T: Display](xs: List[T]) -> List[String] {
  xs.map(x -> x.to_string())
}

fn main() {
  print("plain text")
  print([Dot, Rect(1, 2), Circle { r: 1.5, label: "c \"q\"\n" }])
  print(Some("hi"))
  print((1, "a", ()))
  print(dict([("a", 1)]))
  print(Id(3))
  print(Money { cents: 1234 })
  print([Money { cents: 5 }])
  print(f"total: {Money { cents: 99 }} / {Money { cents: 7 }:>8}|")
  print(Secret { key: "k" })
  print(Some(Secret { key: "k" }))
  print(debug("x"))
  print(Money { cents: 1 }.debug())
  print(to_string(Money { cents: 100 }))
  print(show_all([1, 2]))
  print(Ok(3) == Ok(3))
  print(set([3, 1]))
  print(Dict[String, Int] {})
}
