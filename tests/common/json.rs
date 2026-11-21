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
