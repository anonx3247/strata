//! `std.toml` reads TOML 1.1 as the `toml` crate does (it implements 1.1.0):
//! the same values from valid documents, an error for invalid ones, and the
//! text `stringify` writes reads back as the same value. Both sides print a
//! value in one canonical form (keys sorted, strings as code points, floats
//! as their bits). The one difference: a Strata `Float` is finite, so `inf`
//! and `nan` (and floats too large) are errors.
//!
//! `STRATA_TOML_CORPUS=<toml-test checkout>` adds every 1.1 file of
//! github.com/toml-lang/toml-test (`tests/files-toml-1.1.0`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

/// Each file's value in canonical form, then the value of the text
/// `stringify` wrote for it; or its error.
const CHECKER: &str = r#"use std.fs.{Files, Disk}
use std.process.{Environment, Process}
use std.toml.{parse, stringify, TomlValue, Text, Integer, Number, Boolean, Moment, Array, Table, DateTime, Utc, Minutes}

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
    Number(x) => f"f:{float_bits(x)}",
    Boolean(b) => f"b:{b}",
    Moment(d) => "d:" + moment(d),
    Array(xs) => "[" + join(xs.map(x -> canon(x)), ",") + "]",
    Table(fs) => "{" + join(sort(fs.map(f -> text(f.0) + "=" + canon(f.1))), ",") + "}",
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
"#;

/// Documents, each with whether it is valid TOML 1.1 (the `toml` crate
/// must agree, so the list stays honest).
const CASES: &[(&str, bool, &str)] = &[
    ("empty", true, ""),
    ("comments", true, "# a comment\n\n   # another\t\na = 1 # after\n#end"),
    ("crlf", true, "a = 1\r\nb = \"x\"\r\n[t]\r\nc = '''\r\nline\r\n'''\r\n"),
    (
        "strings",
        true,
        r#"basic = "tab\t nl\n quote\" back\\ uni\u00e9 big\U0001F600 esc\e hex\x41 \b\f\r"
literal = 'C:\Users\nodejs\templates'
multi = """
Roses are red
Violets are blue"""
joined = """\
       The quick brown \
       fox jumps over \
       the lazy dog.\
       """
quotes = """Here are two quotation marks: "". Simple enough."""
five = """"This," she said, "is just a pointless statement.""""
raw = '''
The first newline is
trimmed in raw strings.
   All other whitespace
   is preserved.
'''
rawquotes = '''Here are fifteen quotation marks: """""""""""""""'''
apos = ''''That,' she said, 'is still pointless.''''
unicode = "Ελληνικά 日本語 🎉"
empty = ""
"#,
    ),
    (
        "keys",
        true,
        r#"bare_key = 1
bare-key = 2
1234 = 3
"127.0.0.1" = 4
"ʎǝʞ" = 5
'quoted "value"' = 6
"" = 7
site."google.com" = true
a . b . c = 8
3.14159 = "pi"
"#,
    ),
    (
        "integers",
        true,
        "a = +99\nb = 42\nc = 0\nd = -17\ne = 1_000\nf = 5_349_221\ng = 1_2_3_4_5\nh = 0xDEADBEEF\ni = 0xdead_beef\nj = 0o01234567\nk = 0o755\nl = 0b11010110\nm = 9223372036854775807\nn = -9223372036854775808\no = +0\np = -0\n",
    ),
    (
        "floats",
        true,
        "a = +1.0\nb = 3.1415\nc = -0.01\nd = 5e+22\ne = 1e06\nf = -2E-2\ng = 6.626e-34\nh = 224_617.445_991_228\ni = 0.0\nj = -0.0\nk = 1e1_0\nl = 0e0\nm = 1.7976931348623157e308\n",
    ),
    ("booleans", true, "t = true\nf = false\n"),
    (
        "dates",
        true,
        "odt1 = 1979-05-27T07:32:00Z\nodt2 = 1979-05-27T00:32:00-07:00\nodt3 = 1979-05-27T00:32:00.999999+07:00\nodt4 = 1979-05-27 07:32:00z\nodt5 = 1979-05-27t07:32:00.123456789123Z\nodt6 = 1979-05-27T07:32Z\nldt1 = 1979-05-27T07:32:00\nldt2 = 1979-05-27T00:32:00.999999\nld = 1979-05-27\nlt1 = 07:32:00\nlt2 = 00:32:00.999999\nlt3 = 07:32\nleap = 2024-02-29\nsec60 = 23:59:60\noffset0 = 1979-05-27T07:32:00+00:00\n",
    ),
    (
        "arrays",
        true,
        "integers = [ 1, 2, 3 ]\ncolors = [ \"red\", \"yellow\", \"green\" ]\nnested = [ [ 1, 2 ], [3, 4, 5] ]\nmixed = [ [ 1, 2 ], [\"a\", \"b\", \"c\"] ]\nstrings = [ \"all\", 'strings', \"\"\"are the same\"\"\", '''type''' ]\nnumbers = [ 0.1, 0.2, 0.5, 1, 2, 5 ]\ncontributors = [\n  \"Foo Bar <foo@example.com>\",\n  { name = \"Baz Qux\", email = \"bazqux@example.com\" }\n]\ntrailing = [\n  1,\n  2, # two\n]\nempty = []\nempty2 = [ # c\n]\n",
    ),
    (
        "tables",
        true,
        "[table-1]\nkey1 = \"some string\"\nkey2 = 123\n\n[table-2]\nkey1 = \"another string\"\n\n[dog.\"tater.man\"]\ntype.name = \"pug\"\n\n[a.b.c]\n[ d.e.f ]\n[ g .  h  . i ]\n[ j . \"ʞ\" . 'l' ]\n\n[x.y.z.w]\n[x]\n\n[fruit]\napple.color = \"red\"\napple.taste.sweet = true\n\n[fruit.apple.texture]\nsmooth = true\n",
    ),
    (
        "inline tables",
        true,
        "name = { first = \"Tom\", last = \"Preston-Werner\" }\npoint = { x = 1, y = 2 }\nanimal = { type.name = \"pug\" }\nempty = {}\nmulti = {\n  a = 1, # TOML 1.1\n  b.c = [1,\n    2],\n  d = { e = 1, },\n}\n",
    ),
    (
        "arrays of tables",
        true,
        "[[products]]\nname = \"Hammer\"\nsku = 738594937\n\n[[products]]  # empty table within the array\n\n[[products]]\nname = \"Nail\"\nsku = 284758393\ncolor = \"gray\"\n\n[[fruits]]\nname = \"apple\"\n\n[fruits.physical]  # subtable\ncolor = \"red\"\nshape = \"round\"\n\n[[fruits.varieties]]  # nested array of tables\nname = \"red delicious\"\n\n[[fruits.varieties]]\nname = \"granny smith\"\n\n[[fruits]]\nname = \"banana\"\n\n[[fruits.varieties]]\nname = \"plantain\"\n",
    ),
    ("implicit then dotted", true, "[a.b.c]\nz = 9\n\n[a]\nb.d = 1\n"),
    ("dotted root then subtable", true, "a.b = 1\n[a.c]\nd = 2\n"),
    ("strata.toml", true, "# The standard library.\nname = \"std\"\nexports = [\"io\", \"fs\"]\n\n[deps]\nhttp = { git = \"https://example.com/http.git\", rev = \"v1\" }\n\n[verify]\nproven = [\"main\"]\n\n[json]\nunknown_keys = \"error\"\n"),
    // invalid
    ("no value", false, "a =\n"),
    ("no equals", false, "a 1\n"),
    ("two on a line", false, "a = 1 b = 2\n"),
    ("bare newline in key", false, "a\n= 1\n"),
    ("value on next line", false, "a =\n1\n"),
    ("duplicate key", false, "a = 1\na = 2\n"),
    ("duplicate quoted key", false, "spelling = \"favorite\"\n\"spelling\" = \"favourite\"\n"),
    ("key then table", false, "a = 1\n[a]\n"),
    ("table twice", false, "[a]\n[a]\n"),
    ("table twice nested", false, "[a.b]\n[a]\n[a.b]\n"),
    ("dotted then header", false, "[fruit]\napple.color = \"red\"\n[fruit.apple]\n"),
    ("root dotted then header", false, "a.b = 1\n[a]\n"),
    ("dotted into header table", false, "[a.b.c]\nz = 9\n[a]\nb.c.t = 1\n"),
    ("dotted into value", false, "a = 1\na.b = 2\n"),
    ("extend inline table", false, "a = { b = 1 }\na.c = 2\n"),
    ("header into inline table", false, "a = { b = 1 }\n[a.c]\n"),
    ("inline duplicate", false, "a = { b = 1, b = 2 }\n"),
    ("inline dotted clash", false, "a = { b = 1, b.c = 2 }\n"),
    ("inline double comma", false, "a = { b = 1,, c = 2 }\n"),
    ("inline only comma", false, "a = { , }\n"),
    ("inline unclosed", false, "a = { b = 1\n"),
    ("array of tables then table", false, "[[a]]\n[a]\n"),
    ("table then array of tables", false, "[a]\n[[a]]\n"),
    ("static array then array of tables", false, "a = [1]\n[[a]]\n"),
    ("dotted into array of tables", false, "[[a]]\n[x]\n[a]\n"),
    ("array double comma", false, "a = [1,,2]\n"),
    ("array only comma", false, "a = [,]\n"),
    ("array unclosed", false, "a = [1, 2\n"),
    ("header unclosed", false, "[a\n"),
    ("array header spaced", false, "[[a] ]\n"),
    ("empty header", false, "[]\n"),
    ("empty bare key", false, "= 1\n"),
    ("multi-line key", false, "\"\"\"a\"\"\" = 1\n"),
    ("bad bare key", false, "a$ = 1\n"),
    ("string unclosed", false, "a = \"abc\n"),
    ("string newline", false, "a = \"a\nb\"\n"),
    ("literal unclosed", false, "a = 'abc\n"),
    ("multi unclosed", false, "a = \"\"\"abc\n"),
    ("six quotes", false, "a = \"\"\"a\"\"\"\"\"\"\n"),
    ("bad escape", false, "a = \"\\q\"\n"),
    ("short unicode escape", false, "a = \"\\u12\"\n"),
    ("surrogate escape", false, "a = \"\\uD800\"\n"),
    ("too big escape", false, "a = \"\\U00110000\"\n"),
    ("control in string", false, "a = \"\u{1}\"\n"),
    ("delete in string", false, "a = \"\u{7f}\"\n"),
    ("control in comment", false, "# \u{0} \na = 1\n"),
    ("bare carriage return", false, "a = 1\rb = 2\n"),
    ("carriage return in multi", false, "a = \"\"\"x\ry\"\"\"\n"),
    ("space before escape newline", false, "a = \"\"\"x\\ y\"\"\"\n"),
    ("leading zero", false, "a = 012\n"),
    ("leading zero negative", false, "a = -01\n"),
    ("leading zero float", false, "a = 00.5\n"),
    ("trailing underscore", false, "a = 1_\n"),
    ("double underscore", false, "a = 1__2\n"),
    ("leading underscore", false, "a = _1\n"),
    ("underscore before exponent", false, "a = 1_e2\n"),
    ("signed hex", false, "a = +0x10\n"),
    ("bad hex digit", false, "a = 0xG\n"),
    ("bad octal digit", false, "a = 0o8\n"),
    ("empty hex", false, "a = 0x\n"),
    ("capital hex prefix", false, "a = 0X10\n"),
    ("too big", false, "a = 9223372036854775808\n"),
    ("too small", false, "a = -9223372036854775809\n"),
    ("hex too big", false, "a = 0x8000000000000000\n"),
    ("no integer part", false, "a = .5\n"),
    ("no fraction", false, "a = 5.\n"),
    ("dot exponent", false, "a = 5.e3\n"),
    ("empty exponent", false, "a = 5e\n"),
    ("bad boolean", false, "a = True\n"),
    ("boolean junk", false, "a = truex\n"),
    ("bad month", false, "a = 1979-13-01\n"),
    ("bad day", false, "a = 1979-04-31\n"),
    ("not a leap year", false, "a = 2023-02-29\n"),
    ("century not leap", false, "a = 1900-02-29\n"),
    ("bad hour", false, "a = 24:00:00\n"),
    ("bad minute", false, "a = 12:60:00\n"),
    ("bad second", false, "a = 12:00:61\n"),
    ("short year", false, "a = 979-05-27\n"),
    ("one-digit hour", false, "a = 7:32:00\n"),
    ("fraction without seconds", false, "a = 07:32.5\n"),
    ("empty fraction", false, "a = 07:32:00.\n"),
    ("offset on a date", false, "a = 1979-05-27Z\n"),
    ("offset on a time", false, "a = 07:32:00Z\n"),
    ("bad offset", false, "a = 1979-05-27T07:32:00+7:00\n"),
    ("offset hour", false, "a = 1979-05-27T07:32:00+24:00\n"),
    ("two spaces in date-time", false, "a = 1979-05-27  07:32:00\n"),
    ("junk after table", false, "[a] b = 1\n"),
    ("junk after value", false, "a = \"x\" y\n"),
];

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-toml-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A string as its code points: `104.105`.
fn code_points(s: &str) -> String {
    s.chars().map(|c| (c as u32).to_string()).collect::<Vec<_>>().join(".")
}

/// A value in the checker's canonical form.
fn canon(v: &toml::Value) -> String {
    use toml::Value::*;
    match v {
        String(s) => format!("s:{}", code_points(s)),
        Integer(n) => format!("i:{n}"),
        Float(x) => format!("f:{}", x.to_bits()),
        Boolean(b) => format!("b:{b}"),
        Datetime(d) => {
            let date = d.date.map_or("-".to_string(), |x| format!("{}-{}-{}", x.year, x.month, x.day));
            let time = d.time.map_or("-".to_string(), |t| format!("{}:{}:{}.{}", t.hour, t.minute, t.second.unwrap_or(0), t.nanosecond.unwrap_or(0)));
            let offset = match d.offset {
                Some(toml::value::Offset::Z) => "Z".to_string(),
                Some(toml::value::Offset::Custom { minutes }) => minutes.to_string(),
                None => "-".to_string(),
            };
            format!("d:{date}T{time}O{offset}")
        }
        Array(xs) => format!("[{}]", xs.iter().map(canon).collect::<Vec<_>>().join(",")),
        Table(t) => canon_table(t),
    }
}

fn canon_table(t: &toml::Table) -> String {
    let mut es: Vec<String> = t.iter().map(|(k, v)| format!("{}={}", code_points(k), canon(v))).collect();
    es.sort();
    format!("{{{}}}", es.join(","))
}

/// A float Strata has no value for, anywhere in `v`.
fn not_finite(v: &toml::Value) -> bool {
    match v {
        toml::Value::Float(x) => !x.is_finite(),
        toml::Value::Array(xs) => xs.iter().any(not_finite),
        toml::Value::Table(t) => t.values().any(not_finite),
        _ => false,
    }
}

/// What the `toml` crate makes of a file: its canonical value, `None` for an
/// error; `Some(None)` for a value Strata can't hold.
fn reference(bytes: &[u8]) -> Option<Option<String>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let t: toml::Table = toml::from_str(text).ok()?;
    let v = toml::Value::Table(t);
    Some(if not_finite(&v) { None } else { Some(canon(&v)) })
}

/// The files of a toml-test checkout that TOML 1.1 has.
fn corpus(root: &Path) -> Vec<(String, PathBuf)> {
    let list = std::fs::read_to_string(root.join("tests/files-toml-1.1.0")).unwrap();
    // (`parse` takes a `String`: text that isn't UTF-8 never reaches it)
    list.lines()
        .filter(|l| l.ends_with(".toml"))
        .map(|l| (l.to_string(), root.join("tests").join(l)))
        .filter(|(_, p)| std::str::from_utf8(&std::fs::read(p).unwrap()).is_ok())
        .collect()
}

#[test]
fn reads_toml_as_the_toml_crate_does() {
    let dir = scratch("all");
    let checker = dir.join("check.strata");
    let mut c = Command::new(STRATA).args(["--file", &checker.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(CHECKER.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    let mut files = vec![];
    let mut wrong = vec![];
    for (i, (name, valid, text)) in CASES.iter().enumerate() {
        if reference(text.as_bytes()).is_some() != *valid {
            wrong.push(format!("{name}: the toml crate says it is {}valid", if *valid { "in" } else { "" }));
        }
        let p = dir.join(format!("case{i}.toml"));
        std::fs::write(&p, text).unwrap();
        files.push((name.to_string(), p));
    }
    if let Ok(root) = std::env::var("STRATA_TOML_CORPUS") {
        files.extend(corpus(Path::new(&root)));
    }

    let paths: Vec<String> = files.iter().map(|f| f.1.to_string_lossy().into_owned()).collect();
    let mut args = vec!["--file", checker.to_str().unwrap(), "run", "--"];
    args.extend(paths.iter().map(|s| s.as_str()));
    let o = Command::new(STRATA).args(&args).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let got = String::from_utf8(o.stdout).unwrap();

    let mut valid = 0;
    for (i, (name, p)) in files.iter().enumerate() {
        let start = format!("=== {}\n", p.display());
        let from = got.find(&start).unwrap_or_else(|| panic!("nothing for {name}")) + start.len();
        let until = paths.get(i + 1).and_then(|n| got[from..].find(&format!("=== {n}\n")).map(|k| from + k)).unwrap_or(got.len());
        let lines: Vec<&str> = got[from..until].lines().collect();
        let strata_error = lines.first().is_some_and(|l| l.starts_with("error: "));
        match reference(&std::fs::read(p).unwrap()) {
            None if !strata_error => wrong.push(format!("{name}: invalid, but std.toml read it")),
            None => {}
            Some(None) if !strata_error => wrong.push(format!("{name}: a float that isn't finite, but std.toml read it")),
            Some(None) => {}
            Some(Some(_)) if strata_error => wrong.push(format!("{name}: valid, but std.toml says {}", lines[0])),
            Some(Some(want)) => {
                valid += 1;
                if lines.first() != Some(&want.as_str()) {
                    wrong.push(format!("{name}: read differently:\n  std.toml: {}\n  toml:     {want}", lines.first().unwrap_or(&"(none)")));
                } else if lines.get(1) != Some(&want.as_str()) {
                    wrong.push(format!("{name}: written by stringify, it reads back as:\n  {}", lines.get(1).unwrap_or(&"(none)")));
                }
            }
        }
    }
    assert!(valid >= 15, "only {valid} valid documents");
    assert!(wrong.is_empty(), "{} of {} documents:\n{}", wrong.len(), files.len(), wrong.join("\n"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Messages say where: line and column, in characters.
#[test]
fn errors_say_where() {
    let dir = scratch("errors");
    let prog = dir.join("p.strata");
    let src = r#"use std.toml.{parse, lookup, Text, Integer}

fn main() {
  for t in ["a = 1\nb = 2\nb = 3\n", "[x]\ny = \"é\" z\n", "a = 1979-02-30\n", "n = inf\n"] {
    match parse(t) {
      Ok(_) => print("ok"),
      Err(e) => print(e),
    }
  }
  doc := parse("[server]\nport = 8080\nhost.name = \"x\"\n") except { [] }
  print(lookup(doc, ["server", "port"]))
  print(lookup(doc, ["server", "host", "name"]))
  print(lookup(doc, ["server", "port", "x"]))
  print(lookup(doc, ["nothing"]))
}
"#;
    let mut c = Command::new(STRATA).args(["--file", &prog.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(src.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = Command::new(STRATA).args(["--file", &prog.to_string_lossy(), "run"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "3:1: `b` is defined twice\n\
         2:9: expected the end of the line\n\
         1:5: `1979-02-30`: that month has no day 30\n\
         1:5: `inf`: a Strata Float is finite, so it can't be inf or nan\n\
         Some(Integer(8080))\n\
         Some(Text(\"x\"))\n\
         None\n\
         None\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Edit `src` into a script and run it: its output, or the edit's error.
fn edit_and_run(dir: &Path, name: &str, src: &str) -> Result<String, String> {
    let prog = dir.join(format!("{name}.strata"));
    let mut c = Command::new(STRATA).args(["--file", &prog.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(src.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    if !o.status.success() {
        return Err(format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)));
    }
    let o = Command::new(STRATA).args(["--file", &prog.to_string_lossy(), "run"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

const DERIVED: &str = r#"use std.toml.{Toml, Date, DateTime}
use std.json.Json

type Level =
  | Quiet
  | Loud

type Shape =
  | Circle { r: Float }
  | Square { side: Int }
  | Dot

type Server = { host: String, port: Int where 0 < it && it < 65536 = 8080 }

type Config = {
  name: String where it != "",
  level: Level = Quiet,
  owner: Option[String],
  tags: List[String] = [],
  servers: List[Server] = [],
  shapes: List[Shape] = [],
  limits: Dict[String, Int] = Dict {},
  pair: (Int, String) = (1, "a"),
  since: Option[Date],
  stamp: Option[DateTime],
}

derive Toml for Level

derive Toml for Shape

derive Toml for Server

derive Toml for Config

derive Json for Server

fn main() {
  text := "name = \"app\"\nlevel = \"Loud\"\ntags = [\"a\", \"b\"]\nsince = 2024-02-29\nstamp = 1979-05-27T07:32:00Z\npair = [2, \"b\"]\nshapes = [{ type = \"Circle\", r = 1 }, \"Dot\", { type = \"Square\", side = 3 }]\n\n[limits]\ncpu = 4\n\n[[servers]]\nhost = \"a.example\"\n\n[[servers]]\nhost = \"b.example\"\nport = 9000\n"
  c := Config.read_toml(text)
  print(c)
  match c {
    Ok(cfg) => {
      out := cfg.write_toml()
      print(out)
      match out {
        Ok(t) => print(Config.read_toml(t) == Ok(cfg)),
        Err(_) => (),
      }
      // a type may derive both: the methods are named for each format
      print(Server { host: "x", port: 1 }.to_json())
      print(Server { host: "x", port: 1 }.to_toml())
    },
    Err(_) => (),
  }
  print(Config.read_toml("level = \"Loud\""))
  print(Config.read_toml("name = \"\""))
  print(Config.read_toml("name = \"a\"\n[[servers]]\nhost = \"h\"\nport = 70000"))
  print(Config.read_toml("name = \"a\"\nlevel = \"Medium\""))
  print(Config.read_toml("name = \"a\"\nsince = 07:32"))
  print(Config.read_toml("name = \"a\"\nshapes = [{ type = \"Circle\" }]"))
  print(Config.read_toml("name = \"a\"\nnope = 1"))
  print(Config.from_toml_with(std.toml.Table([("name", std.toml.Text("a")), ("nope", std.toml.Integer(1))]), std.json.Reject))
  print(Loud.write_toml())
}
"#;

#[test]
fn derives_toml() {
    let dir = scratch("derive");
    let out = edit_and_run(&dir, "config", DERIVED).unwrap();
    let want = r#"Ok(Config { name: "app", level: Loud, owner: None, tags: ["a", "b"], servers: [Server { host: "a.example", port: 8080 }, Server { host: "b.example", port: 9000 }], shapes: [Circle { r: 1.0 }, Dot, Square { side: 3 }], limits: Dict { "cpu": 4 }, pair: (2, "b"), since: Some(Date { year: 2024, month: 2, day: 29 }), stamp: Some(DateTime { date: Some(Date { year: 1979, month: 5, day: 27 }), time: Some(Time { hour: 7, minute: 32, second: 0, nanosecond: 0 }), offset: Some(Utc) }) })
Ok("name = \"app\"\nlevel = \"Loud\"\ntags = [\"a\", \"b\"]\npair = [2, \"b\"]\nsince = 2024-02-29\nstamp = 1979-05-27T07:32:00Z\n\n[[servers]]\nhost = \"a.example\"\nport = 8080\n\n[[servers]]\nhost = \"b.example\"\nport = 9000\n\n[[shapes]]\ntype = \"Circle\"\nr = 1.0\n\n[[shapes]]\ntype = \"Dot\"\n\n[[shapes]]\ntype = \"Square\"\nside = 3\n\n[limits]\ncpu = 4\n")
true
Ok(Object([("type", Text("Server")), ("host", Text("x")), ("port", Integer(1))]))
Ok(Table([("host", Text("x")), ("port", Integer(1))]))
Err("missing key \"name\"")
Err("name: \"\" does not satisfy `String where it != \"\"`")
Err("servers[0].port: 70000 does not satisfy `Int where 0 < it && it < 65536`")
Err("level: `Medium` is not one of Quiet, Loud")
Err("since: expected a date (1979-05-27), got 07:32:00")
Err("shapes[0]: missing key \"r\"")
Ok(Config { name: "a", level: Quiet, owner: None, tags: [], servers: [], shapes: [], limits: Dict {}, pair: (1, "a"), since: None, stamp: None })
Err("unknown key \"nope\"")
Err("a TOML document is a table, not \"Loud\"")
"#;
    assert_eq!(out, want);

    // what has no TOML form is an error at the `derive`
    for (name, src, message) in [
        ("optional_element", "type A = { xs: List[Option[Int]] }", "TOML has no null"),
        ("double_option", "type A = { x: Option[Option[Int]] }", "`None` and `Some(None)` would both be left out"),
        ("type_field", "type A = { type: String }", "a field named `type`"),
        ("overlap", "type A = { x: Int | Float }", "two members are written as a TOML number"),
    ] {
        let err = edit_and_run(&dir, name, &format!("use std.toml.Toml\n\n{src}\n\nderive Toml for A\n")).unwrap_err();
        assert!(err.contains(message), "{name}: {err}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
