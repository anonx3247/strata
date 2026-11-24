from std.json use Json, JsonValue, Reject, parse, stringify

type Address = { city: String, zip: String where len(it.code_points()) == 5 }

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
  Event.from_json(parse(text)!)
}

fn read(text: String) -> Result[Signup, String] {
  Signup.from_json(parse(text)!)
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
  text := stringify(s.to_json() !! Null)
  print(text)
  print(read(text) == Ok(s))
  home := "\"home\": {\"city\": \"L\", \"zip\": \"12345\"}"
  print(
    read(
      f"{{\"email\": \"x@y\", \"age\": 15, {home}, \"shapes\": [\"Dot\", {{\"type\": \"Pixels\", \"value\": 2}}], \"level\": \"Quiet\", \"scores\": {{}}}}",
    ),
  )
  print(
    read(
      f"{{\"email\": \"x@y\", \"age\": 12, {home}, \"shapes\": [], \"level\": \"Quiet\", \"scores\": {{}}}}",
    ),
  )
  print(
    read(
      "{\"email\": \"x@y\", \"age\": 15, \"home\": {\"city\": \"L\", \"zip\": \"1234\"}, \"shapes\": [], \"level\": \"Quiet\", \"scores\": {}}",
    ),
  )
  print(
    read(
      f"{{\"email\": \"x@y\", \"age\": 15, {home}, \"shapes\": [{{\"type\": \"Circle\", \"r\": \"no\", \"label\": \"x\"}}], \"level\": \"Quiet\", \"scores\": {{}}}}",
    ),
  )
  extra := f"{{\"email\": \"x@y\", \"age\": 15, {home}, \"shapes\": [], \"level\": \"Loud\", \"scores\": {{}}, \"extra\": 1}}"
  print(read(extra) is Ok)
  print(Signup.from_json_with(parse(extra) !! Null, Reject))
  print(read(f"{{\"type\": \"Address\", \"email\": \"x@y\"}}"))
  print(read("[1, 2"))
  print(stringify(parse(
    "[1, -2.5e3, \"a\u00e9\\n\ud83d\ude00\", true, null, {\"k\": []}]",
  ) !! Null))
  print(parse("99999999999999999999"))
  print(parse("9007199254740991"))
  print(parse("-9007199254740992"))
  e := Event {
    user: UserId(7),
    at: (3, "pm"),
    payload: Address { city: "R", zip: "00100" },
    kind: Click { times: 2 },
  }
  print(stringify(e.to_json() !! Null))
  print(event(stringify(e.to_json() !! Null)) == Ok(e))
  print(
    event(
      "{\"user\": 7, \"at\": [1, \"am\"], \"payload\": \"hi\", \"kind\": {\"type\": \"Click\"}}",
    ),
  )
  print(
    event(
      "{\"user\": 7, \"at\": [1, \"am\"], \"payload\": 4, \"kind\": {\"type\": \"Key\", \"value\": \"k\"}}",
    ),
  )
  print(event(
    "{\"user\": 0, \"at\": [1, \"am\"], \"payload\": 4, \"kind\": \"Click\"}",
  ))
  print(event(
    "{\"user\": 7, \"at\": [1], \"payload\": 4, \"kind\": \"Click\"}",
  ))
  print(event(
    "{\"user\": 7, \"at\": [1, \"am\"], \"payload\": [], \"kind\": \"Click\"}",
  ))
}
