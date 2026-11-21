//! The program `std.json` is tested with, by the interpreter (cli.rs) and
//! compiled (build.rs).

/// The program `std.json` is tested with: records, sums, choices, options,
/// defaults, lists, dictionaries, refinements, and JSON text both ways.
pub const PROGRAM: &str = r#"use std.json.{Json, JsonValue, Reject, parse, stringify}

type Address = { city: String, zip: String where len(it) == 5 }

type Shape =
  | Dot
  | Pixels(Int)
  | Circle { r: Float, label: String }

type Level =
  | Quiet
  | Loud

type Signup = {
  email: String where contains(it, "@"),
  age: Int where it >= 13,
  nickname: Option[String],
  tags: List[String] = [],
  home: Address,
  shapes: List[Shape],
  level: Level,
  scores: Dict[String, Int],
}

derive Json for Address

derive Json for Shape

derive Json for Level

derive Json for Signup

distinct type UserId = Int
  where it > 0

type Kind =
  | Click { times: Int = 1 }
  | Key(String)

type Event = {
  user: UserId,
  at: (Int, String),
  payload: Positive | String | Address,
  kind: Kind,
}

derive Json for Kind

derive Json for Event

fn event(text: String) -> Result[Event, String] {
  Event.decode(parse(text)!)
}

fn read(text: String) -> Result[Signup, String] {
  Signup.decode(parse(text)!)
}

fn main() {
  s := Signup {
    email: "a@b",
    age: 20,
    nickname: Some("al"),
    home: Address { city: "P", zip: "75001" },
    shapes: [Dot, Pixels(3), Circle { r: 1.5, label: "c" }],
    level: Loud,
    scores: dict([("a", 1)]),
  }
  text := stringify(s.encode() !! Null)
  print(text)
  print(read(text) == Ok(s))
  home := "\"home\": {\"city\": \"L\", \"zip\": \"12345\"}"
  print(read(f"{{\"email\": \"x@y\", \"age\": 15, {home}, \"shapes\": [\"Dot\", {{\"type\": \"Pixels\", \"value\": 2}}], \"level\": \"Quiet\", \"scores\": {{}}}}"))
  print(read(f"{{\"email\": \"x@y\", \"age\": 12, {home}, \"shapes\": [], \"level\": \"Quiet\", \"scores\": {{}}}}"))
  print(read("{\"email\": \"x@y\", \"age\": 15, \"home\": {\"city\": \"L\", \"zip\": \"1234\"}, \"shapes\": [], \"level\": \"Quiet\", \"scores\": {}}"))
  print(read(f"{{\"email\": \"x@y\", \"age\": 15, {home}, \"shapes\": [{{\"type\": \"Circle\", \"r\": \"no\", \"label\": \"x\"}}], \"level\": \"Quiet\", \"scores\": {{}}}}"))
  extra := f"{{\"email\": \"x@y\", \"age\": 15, {home}, \"shapes\": [], \"level\": \"Loud\", \"scores\": {{}}, \"extra\": 1}}"
  print(read(extra) is Ok)
  print(Signup.decode_with(parse(extra) !! Null, Reject))
  print(read(f"{{\"type\": \"Address\", \"email\": \"x@y\"}}"))
  print(read("[1, 2"))
  print(stringify(parse("[1, -2.5e3, \"a\\u00e9\\n\\ud83d\\ude00\", true, null, {\"k\": []}]") !! Null))
  print(parse("99999999999999999999"))
  print(parse("9007199254740991"))
  print(parse("-9007199254740992"))
  e := Event { user: UserId(7), at: (3, "pm"), payload: Address { city: "R", zip: "00100" }, kind: Click { times: 2 } }
  print(stringify(e.encode() !! Null))
  print(event(stringify(e.encode() !! Null)) == Ok(e))
  print(event("{\"user\": 7, \"at\": [1, \"am\"], \"payload\": \"hi\", \"kind\": {\"type\": \"Click\"}}"))
  print(event("{\"user\": 7, \"at\": [1, \"am\"], \"payload\": 4, \"kind\": {\"type\": \"Key\", \"value\": \"k\"}}"))
  print(event("{\"user\": 0, \"at\": [1, \"am\"], \"payload\": 4, \"kind\": \"Click\"}"))
  print(event("{\"user\": 7, \"at\": [1], \"payload\": 4, \"kind\": \"Click\"}"))
  print(event("{\"user\": 7, \"at\": [1, \"am\"], \"payload\": [], \"kind\": \"Click\"}"))
}
"#;

/// The output of `PROGRAM`.
pub const OUTPUT: &str = r#"{"type":"Signup","email":"a@b","age":20,"nickname":"al","tags":[],"home":{"type":"Address","city":"P","zip":"75001"},"shapes":[{"type":"Dot"},{"type":"Pixels","value":3},{"type":"Circle","r":1.5,"label":"c"}],"level":"Loud","scores":{"a":1}}
true
Ok(Signup { email: "x@y", age: 15, nickname: None, tags: [], home: Address { city: "L", zip: "12345" }, shapes: [Dot, Pixels(2)], level: Quiet, scores: Dict {} })
Err("age: 12 does not satisfy `Int where it >= 13`")
Err("home.zip: \"1234\" does not satisfy `String where len(it) == 5`")
Err("shapes[0].r: expected a number, got \"no\"")
true
Err("unknown field \"extra\"")
Err("expected a `Signup`, got a `Address`")
Err("at 5: expected `,` or `]` in an array")
[1,-2500.0,"aé\n😀",true,null,{"k":[]}]
Err("at 0: 99999999999999999999 is beyond ±(2^53 - 1), the integers a JSON number holds exactly")
Ok(Integer(9007199254740991))
Err("at 0: -9007199254740992 is beyond ±(2^53 - 1), the integers a JSON number holds exactly")
{"type":"Event","user":7,"at":[3,"pm"],"payload":{"type":"Address","city":"R","zip":"00100"},"kind":{"type":"Click","times":2}}
true
Ok(Event { user: UserId(7), at: (1, "am"), payload: "hi", kind: Click { times: 1 } })
Ok(Event { user: UserId(7), at: (1, "am"), payload: 4, kind: Key("k") })
Err("user: 0 does not satisfy `Int where it > 0`")
Err("at: expected an array of 2 values, got [1]")
Err("payload: expected Positive or String or Address, got []")
"#;

/// The built-in types are `Json` too: generic code over `T: Json`.
#[allow(dead_code)]
pub const BUILTINS: &str = r#"use std.json.{Json, parse, stringify}

type P = { name: String, tags: List[String] }

derive Json for P

fn send[T: Json](x: T) -> String {
  stringify(x.encode() !! std.json.Null)
}

fn read[T: Json](text: String) -> Result[T, String] {
  T.decode(parse(text)!)
}

fn ints(text: String) -> Result[List[Int], String] {
  read(text)
}

fn people(text: String) -> Result[Dict[String, List[P]], String] {
  read(text)
}

fn main() {
  print(send([1, 2, 3]))
  print(send(dict([("a", [P { name: "x", tags: [] }])])))
  print(send(Some(2.5)))
  print(ints("[1, 2]"))
  print(ints("[1, \"x\"]"))
  print(people("{\"team\": [{\"name\": \"y\", \"tags\": [\"t\"]}]}"))
  print(people("{\"team\": [{\"name\": 3, \"tags\": []}]}"))
}
"#;

#[allow(dead_code)]
pub const BUILTINS_OUTPUT: &str = r#"[1,2,3]
{"a":[{"type":"P","name":"x","tags":[]}]}
2.5
Ok([1, 2])
Err("[1]: expected an integer, got \"x\"")
Ok(Dict { "team": [P { name: "y", tags: ["t"] }] })
Err("team[0].name: expected a string, got 3")
"#;

/// Methods called on an impl's type parameters (`T.zero()` in
/// `impl[T: Zero] Zero for List[T]`): types are passed whole at runtime.
#[allow(dead_code)]
pub const STATICS: &str = r#"trait Zero {
  fn zero() -> Self
}

impl Zero for Int {
  fn zero() -> Self { 0 }
}

impl Zero for String {
  fn zero() -> Self { "z" }
}

impl[T: Zero] Zero for List[T] {
  fn zero() -> Self { [T.zero(), T.zero()] }
}

impl[A: Zero, B: Zero] Zero for Dict[A, B] {
  fn zero() -> Self { dict([(A.zero(), B.zero())]) }
}

fn zero_of[T: Zero]() -> T { T.zero() }

fn nested[T: Zero]() -> List[List[T]] { zero_of() }

fn a() -> List[Int] { zero_of() }

fn b() -> List[List[String]] { zero_of() }

fn c() -> Dict[String, List[Int]] { zero_of() }

fn d() -> List[List[Int]] { nested() }

fn main() {
  print(a())
  print(b())
  print(c())
  print(d())
}
"#;

#[allow(dead_code)]
pub const STATICS_OUTPUT: &str = r#"[0, 0]
[["z", "z"], ["z", "z"]]
Dict { "z": [0, 0] }
[[0, 0], [0, 0]]
"#;

/// `Debug` and `Display`: values as written in code, a type's own impls.
#[allow(dead_code)]
pub const TEXT: &str = r#"type Money = { cents: Int }

impl Display for Money {
  fn to_string(self) -> String { f"${self.cents / 100}.{self.cents % 100:02}" }
}

type Shape =
  | Dot
  | Rect(Int, Int)
  | Circle { r: Float, label: String }

distinct type Id = Int

type Secret = { key: String }

impl Debug for Secret {
  fn debug(self) -> String { "Secret { key: *** }" }
}

fn show_all[T: Display](xs: List[T]) -> List[String] { xs.map(x -> x.to_string()) }

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
"#;

#[allow(dead_code)]
pub const TEXT_OUTPUT: &str = r#"plain text
[Dot, Rect(1, 2), Circle { r: 1.5, label: "c \"q\"\n" }]
Some("hi")
(1, "a", ())
Dict { "a": 1 }
Id(3)
$12.34
[Money { cents: 5 }]
total: $0.99 /    $0.07|
Secret { key: *** }
Some(Secret { key: *** })
"x"
Money { cents: 1 }
$1.00
["1", "2"]
true
Set { 1, 3 }
Dict {}
"#;

#[allow(dead_code)]
pub const SELF_METHODS: &str = r#"trait Zero {
  fn zero() -> Self
}

impl Zero for Int {
  fn zero() -> Self { 0 }
}

impl Zero for String {
  fn zero() -> Self { "z" }
}

trait Pad {
  fn padded(self, n: Int) -> Self
}

impl[T: Zero] Pad for List[T] {
  fn padded(self, n: Int) -> Self {
    mut out := self
    while len(out) < n {
      out = push(out, T.zero())
    }
    out
  }
}

type Cell[T] = { v: Option[T] }

impl[T: Zero] Cell[T] {
  fn get_or_zero(self) -> T {
    match self.v {
      Some(x) => x,
      None => T.zero(),
    }
  }
}

fn pad_all[T: Zero](xs: List[List[T]], n: Int) -> List[List[T]] {
  xs.map(x -> x.padded(n))
}

fn ints() -> List[List[Int]] { pad_all([[1], []], 3) }

fn main() {
  let e: List[String] = []
  print(e.padded(2))
  print([1].padded(3))
  print(Cell { v: None }.get_or_zero() + 1)
  let c: Cell[String] = Cell { v: None }
  print(c.get_or_zero())
  print(ints())
}
"#;

#[allow(dead_code)]
pub const SELF_METHODS_OUTPUT: &str = r#"["z", "z"]
[1, 0, 0]
1
z
[[1, 0, 0], [0, 0, 0]]
"#;

#[allow(dead_code)]
pub const GENERIC_DERIVE: &str = r#"use std.json.{Json, parse, stringify}

type Pair[T] = { a: T, b: T }

derive Json for Pair

type Box = { p: Pair[Int] }

derive Json for Box

fn main() {
  print(stringify(Box { p: Pair { a: 1, b: 2 } }.encode() !! std.json.Null))
  print(Box.decode(parse("{\"p\": {\"a\": 3, \"b\": 4}}") !! std.json.Null))
}
"#;

#[allow(dead_code)]
pub const GENERIC_DERIVE_OUTPUT: &str = r#"{"type":"Box","p":{"type":"Pair","a":1,"b":2}}
Ok(Box { p: Pair { a: 3, b: 4 } })
"#;

#[allow(dead_code)]
pub const OPTION_OPTION: &str = r#"use std.json.{Json, stringify}

fn send[T: Json](x: T) -> String {
  match x.encode() {
    Ok(j) => stringify(j),
    Err(e) => f"error: {e}",
  }
}

fn main() {
  let a: Option[Option[Int]] = Some(None)
  print(send(a))
  let b: Option[Option[Int]] = Some(Some(2))
  print(send(b))
  let c: Option[Option[Int]] = None
  print(send(c))
}
"#;

#[allow(dead_code)]
pub const OPTION_OPTION_OUTPUT: &str = r#"error: `Some` of a value written as null can't be JSON: it would read back as `None`
2
null
"#;

#[allow(dead_code)]
pub const ROOT_BUILTIN_NAME: &str = r#"use std.io.{Console, recorder, printed}

fn get(x: Int) -> Int { x * 10 }

fn main() {
  mut r := recorder([])
  mut r.print("hi")
  print(get(4))
  print(printed(r))
}
"#;

#[allow(dead_code)]
pub const ROOT_BUILTIN_NAME_OUTPUT: &str = r#"40
["hi"]
"#;

/// Refined aliases kept by name in type arguments: `List[Port]` decoded
/// through the generic impls checks `Port`; `T.check(x)` on any type.
#[allow(dead_code)]
pub const REFINED: &str = r#"use std.json.{Json, parse}

type Port = Int
  where 1 <= it && it <= 65535

fn read[T: Json](text: String) -> Result[T, String] {
  T.decode(parse(text)!)
}

fn ports(text: String) -> Result[List[Port], String] {
  read(text)
}

fn maybe(text: String) -> Result[Option[Positive], String] {
  read(text)
}

fn table(text: String) -> Result[Dict[String, Natural], String] {
  read(text)
}

fn valid[T](x: T) -> String {
  match T.check(x) {
    Ok(_) => "ok",
    Err(e) => e,
  }
}

fn main() {
  print(ports("[80, 443]"))
  print(ports("[80, 70000]"))
  print(maybe("null"))
  print(maybe("0"))
  print(table("{\"a\": 1, \"b\": -2}"))
  let p: Port = 8080
  print(valid(p))
  print(Port.check(0))
  print(Positive.check(3))
}
"#;

#[allow(dead_code)]
pub const REFINED_OUTPUT: &str = r#"Ok([80, 443])
Err("[1]: 70000 does not satisfy `Port` (where 1 <= it && it <= 65535)")
Ok(None)
Err("0 does not satisfy `Positive` (where it > 0)")
Err("b: -2 does not satisfy `Natural` (where it >= 0)")
ok
Err("0 does not satisfy `Port` (where 1 <= it && it <= 65535)")
Ok(())
"#;

/// Refined aliases with type parameters (`Many[Int]`) and refinements
/// written as type arguments (`List[Int where it % 2 == 0]`).
#[allow(dead_code)]
pub const REFINED_MORE: &str = r#"use std.json.{Json, parse}

type Many[T] = List[T]
  where len(it) >= 2

fn read[T: Json](text: String) -> Result[T, String] {
  T.decode(parse(text)!)
}

fn pairs(text: String) -> Result[Many[Int], String] {
  read(text)
}

fn evens(text: String) -> Result[List[Int where it % 2 == 0], String] {
  read(text)
}

fn names(text: String) -> Result[Dict[String, String where len(it) > 0], String] {
  read(text)
}

fn main() {
  print(pairs("[1, 2, 3]"))
  print(pairs("[1]"))
  print(evens("[2, 4]"))
  print(evens("[2, 3]"))
  print(names("{\"a\": \"x\", \"b\": \"\"}"))
  let m: Many[String] = ["a", "b"]
  print(len(m))
}
"#;

#[allow(dead_code)]
pub const REFINED_MORE_OUTPUT: &str = r#"Ok([1, 2, 3])
Err("[1] does not satisfy `Many` (where len(it) >= 2)")
Ok([2, 4])
Err("[1]: 3 does not satisfy `Int where it % 2 == 0`")
Err("b: \"\" does not satisfy `String where len(it) > 0`")
2
"#;

/// `for` over values implementing `Iterator`: a concrete type, a generic one,
/// one calling `T.zero()` in `next`, through a generic function, nested, with
/// `break` / `continue`, and a tuple pattern.
#[allow(dead_code)]
pub const ITERATORS: &str = r#"trait Zero {
  fn zero() -> Self
}

impl Zero for Int {
  fn zero() -> Self { 0 }
}

impl Zero for String {
  fn zero() -> Self { "z" }
}

type Count = { at: Int, end: Int }

impl Iterator[Int] for Count {
  fn next(mut self) -> Option[Int] {
    if self.at < self.end {
      self.at = self.at + 1
      Some(self.at - 1)
    } else {
      None
    }
  }
}

type ListIter[T] = { items: List[T], pos: Int }

impl[T] Iterator[T] for ListIter[T] {
  fn next(mut self) -> Option[T] {
    if self.pos < len(self.items) {
      x := self.items[self.pos]
      self.pos = self.pos + 1
      Some(x)
    } else {
      None
    }
  }
}

type Padded[T] = { inner: ListIter[T], extra: Int }

impl[T: Zero] Iterator[T] for Padded[T] {
  fn next(mut self) -> Option[T] {
    match mut self.inner.next() {
      Some(x) => Some(x),
      None => if self.extra > 0 {
        self.extra = self.extra - 1
        Some(T.zero())
      } else {
        None
      },
    }
  }
}

fn collect[T, I: Iterator[T]](it: I) -> List[T] {
  mut out := []
  for x in it {
    out = push(out, x)
  }
  out
}

fn sum(it: Count) -> Int {
  mut total := 0
  for i in it {
    if i == 2 {
      continue
    }
    if i == 6 {
      break
    }
    inner := Count { at: 0, end: i }
    for j in inner {
      total = total + j
    }
  }
  total
}

fn main() {
  print(collect(ListIter { items: [1, 2, 3], pos: 0 }))
  print(collect(Padded { inner: ListIter { items: [4, 5], pos: 0 }, extra: 2 }))
  print(collect(Padded { inner: ListIter { items: ["a"], pos: 0 }, extra: 1 }))
  print(sum(Count { at: 0, end: 10 }))
  pairs := ListIter { items: [(1, "x"), (2, "y")], pos: 0 }
  for (i, s) in pairs {
    print(f"{i}{s}")
  }
}
"#;

#[allow(dead_code)]
pub const ITERATORS_OUTPUT: &str = r#"[1, 2, 3]
[4, 5, 0, 0]
["a", "z"]
19
1x
2y
"#;

/// `FiniteIterator` (`len`, on a type parameter too), and the lazy `map` /
/// `filter` / `fold` of every iterator, unbounded ones included; a list's
/// `map` and `len` are still the builtins.
#[allow(dead_code)]
pub const LAZY_ITERATORS: &str = r#"type Count = { at: Int, end: Int }

impl Iterator[Int] for Count {
  fn next(mut self) -> Option[Int] {
    if self.at < self.end {
      self.at += 1
      Some(self.at - 1)
    } else {
      None
    }
  }
}

impl FiniteIterator[Int] for Count {
  fn len(self) -> Natural {
    if self.end > self.at { self.end - self.at } else { 0 }
  }
}

type Naturals = { at: Int }

impl Iterator[Int] for Naturals {
  fn next(mut self) -> Option[Int] {
    self.at += 1
    Some(self.at - 1)
  }
}

fn total(c: Count) -> Int {
  mut t := 0
  for i in c {
    t += i
  }
  t
}

fn size[I: FiniteIterator[Int]](it: I) -> Int {
  mut k := 0
  for _ in it {
    k += 1
  }
  k * 100 + it.len()
}

fn first_square_over(n: Int) -> Int {
  squares := Naturals { at: 0 }.map(x -> x * x)
  for s in squares {
    if s > n {
      return s
    }
  }
  0
}

fn main() {
  c := Count { at: 2, end: 5 }
  print(total(c))
  print(size(c))
  print(first_square_over(50))
  evens := Naturals { at: 0 }.filter(n -> n % 2 == 0).map(n -> f"<{n}>")
  for e in evens {
    if e == "<8>" {
      break
    }
    print(e)
  }
  print(Count { at: 0, end: 5 }.map(n -> n + 1).filter(n -> n != 3).fold(0, (a, b) -> a + b))
  xs := [1, 2, 3]
  print(xs.map(x -> x * 2))
  print(len(xs))
}
"#;

#[allow(dead_code)]
pub const LAZY_ITERATORS_OUTPUT: &str = r#"9
303
64
<0>
<2>
<4>
<6>
12
[2, 4, 6]
3
"#;
