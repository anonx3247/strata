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
  text := stringify(s.encode())
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
  print(stringify(e.encode()))
  print(event(stringify(e.encode())) == Ok(e))
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
{"Ok":{"email":"x@y","age":15,"nickname":"None","tags":[],"home":{"city":"L","zip":"12345"},"shapes":["Dot",{"Pixels":2}],"level":"Quiet","scores":{}}}
{"Err":"age: 12 does not satisfy `Int where it >= 13`"}
{"Err":"home.zip: \"1234\" does not satisfy `String where len(it) == 5`"}
{"Err":"shapes[0].r: expected a number, got \"no\""}
true
{"Err":"unknown field \"extra\""}
{"Err":"expected a `Signup`, got a `Address`"}
{"Err":"at 5: expected `,` or `]` in an array"}
[1,-2500.0,"aé\n😀",true,null,{"k":[]}]
{"Err":"at 0: 99999999999999999999 is beyond ±(2^53 - 1), the integers a JSON number holds exactly"}
{"Ok":{"Integer":9007199254740991}}
{"Err":"at 0: -9007199254740992 is beyond ±(2^53 - 1), the integers a JSON number holds exactly"}
{"type":"Event","user":7,"at":[3,"pm"],"payload":{"type":"Address","city":"R","zip":"00100"},"kind":{"type":"Click","times":2}}
true
{"Ok":{"user":7,"at":[1,"am"],"payload":"hi","kind":{"Click":{"times":1}}}}
{"Ok":{"user":7,"at":[1,"am"],"payload":4,"kind":{"Key":"k"}}}
{"Err":"user: 0 does not satisfy `Int where it > 0`"}
{"Err":"at: expected an array of 2 values, got [1]"}
{"Err":"payload: expected Positive or String or Address, got []"}
"#;
