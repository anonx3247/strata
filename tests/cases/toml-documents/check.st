from std.fs use Files, Disk
from std.process use Environment, Process
from std.toml use parse, stringify, TomlValue, Text, Integer, Number, Boolean, Moment, Array, Table, DateTime, Utc, Minutes

fn main(disk: Disk, process: Process) {
  for p in process.args() {
    print("=== " + p)
    match disk.read(p) {
      Ok(text) => match parse(text) {
        Ok(doc) => {
          print(canon(Table(doc)))
          match parse(stringify(doc)) {
            Ok(again) => print(canon(Table(again))),
            Err(e) => print(f"rewritten: {e}"),
          }
        },
        Err(e) => print(f"error: {e}"),
      },
      Err(_) => print("error: unreadable"),
    }
  }
}

fn text(s: String) -> String {
  join([f"{c}" for c in s.code_points()], ".")
}

fn canon(v: TomlValue) -> String {
  match v {
    Text(s) => "s:" + text(s),
    Integer(n) => f"i:{n}",
    Number(x) => match parse_float(x) {
      Some(f) => f"f:{float_bits(f)}",
      None => "f:?",
    },
    Boolean(b) => f"b:{b}",
    Moment(d) => "d:" + moment(d),
    Array(xs) => "[" + join(xs.map(x -> canon(x)), ",") + "]",
    Table(fs) => "{" + join((fs.map(f -> text(f.0) + "=" + canon(f.1))).sorted(), ",") + "}",
  }
}

fn moment(d: DateTime) -> String {
  date := match d.date {
    Some(x) => f"{x.year}-{x.month}-{x.day}",
    None => "-",
  }
  time := match d.time {
    Some(t) => f"{t.hour}:{t.minute}:{t.second}.{t.nanosecond}",
    None => "-",
  }
  offset := match d.offset {
    Some(Utc) => "Z",
    Some(Minutes(m)) => f"{m}",
    None => "-",
  }
  date + "T" + time + "O" + offset
}
