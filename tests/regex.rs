//! `std.regex` reads patterns as the `regex` crate does: `parse` gives the
//! tree regex-syntax builds (shown the same way on both sides), or an error
//! where regex-syntax gives one. Curated patterns, then random ones: 5000 by
//! default; `REGEX_CASES=30000 REGEX_SEED=3 cargo test --release
//! --test regex` tries more.

use std::io::Write;
use std::process::{Command, Stdio};

use rand::{RngExt, SeedableRng};
use regex_syntax::hir::{Class, Hir, HirKind};

/// The pattern as `regex::Regex::new` reads it (regex-automata's defaults).
fn reference(pattern: &str) -> String {
    let parsed = regex_syntax::ParserBuilder::new()
        .case_insensitive(false)
        .multi_line(false)
        .dot_matches_new_line(false)
        .crlf(false)
        .line_terminator(b'\n')
        .swap_greed(false)
        .ignore_whitespace(false)
        .unicode(true)
        .utf8(true)
        .nest_limit(250)
        .octal(false)
        .build()
        .parse(pattern);
    match parsed {
        Ok(h) => format!("ok {}", show(&h)),
        Err(_) => "err".into(),
    }
}

/// As `std.regex.show`.
fn show(h: &Hir) -> String {
    let all = |xs: &[Hir]| xs.iter().map(show).collect::<Vec<_>>().join(",");
    match h.kind() {
        HirKind::Empty => "empty".into(),
        HirKind::Literal(l) => {
            let s = std::str::from_utf8(&l.0).expect("a UTF-8 literal");
            format!("lit({})", s.chars().map(|c| format!("{:x}", c as u32)).collect::<Vec<_>>().join(" "))
        }
        HirKind::Class(Class::Unicode(c)) => {
            format!("cls({})", c.ranges().iter().map(|r| format!("{:x}-{:x}", r.start() as u32, r.end() as u32)).collect::<Vec<_>>().join(" "))
        }
        HirKind::Class(Class::Bytes(c)) => {
            format!("cls({})", c.ranges().iter().map(|r| format!("{:x}-{:x}", r.start(), r.end())).collect::<Vec<_>>().join(" "))
        }
        HirKind::Look(l) => format!("look({l:?})"),
        HirKind::Repetition(r) => format!(
            "rep({},{},{},{})",
            r.min,
            r.max.map_or("*".to_string(), |m| m.to_string()),
            if r.greedy { "greedy" } else { "lazy" },
            show(&r.sub)
        ),
        HirKind::Capture(c) => format!("cap({},{},{})", c.index, c.name.as_deref().unwrap_or("-"), show(&c.sub)),
        HirKind::Concat(xs) => format!("cat({})", all(xs)),
        HirKind::Alternation(xs) => format!("alt({})", all(xs)),
    }
}

/// A Strata string literal: only `\n`, `\t`, `\"` and `\\` are escapes.
fn literal(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What `std.regex` makes of each pattern, one line each.
fn strata(name: &str, patterns: &[String]) -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("strata-regex-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("parse.strata");
    let list = patterns.iter().map(|p| literal(p)).collect::<Vec<_>>().join(",\n    ");
    let src = format!(
        "use std.regex.{{parse, show}}\n\nfn main() {{\n  for p in [\n    {list},\n  ] {{\n    print(match parse(p) {{\n      Ok(r) => \"ok \" + show(r),\n      Err(_) => \"err\",\n    }})\n  }}\n}}\n"
    );
    let strata = env!("CARGO_BIN_EXE_strata");
    let mut c = Command::new(strata)
        .args(["--file", &file.to_string_lossy(), "edit"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(src.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = Command::new(strata).args(["--file", &file.to_string_lossy(), "run"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8(o.stdout).unwrap().lines().map(String::from).collect()
}

fn same_as_regex_syntax(name: &str, patterns: Vec<String>) {
    let got = strata(name, &patterns);
    assert_eq!(got.len(), patterns.len(), "one line per pattern");
    let mut wrong = vec![];
    for (p, g) in patterns.iter().zip(&got) {
        let want = reference(p);
        if *g != want {
            wrong.push(format!("pattern {p:?}\n  std.regex:    {g}\n  regex-syntax: {want}"));
        }
    }
    assert!(wrong.is_empty(), "{} of {} patterns differ:\n{}", wrong.len(), patterns.len(), wrong.join("\n"));
}

const CURATED: &[&str] = &[
    // literals, escapes, flags
    "", "a", "abc", "λx", "\\.", "\\%", "\\e", "\\x41", "\\x{1F600}", "\\u00e9", "\\U0001F600", "\\x{110000}", "\\x{D800}",
    "\\x{}", "\\x4", "\\xff", "(?-u)\\xff", "(?-u)\\x41", "(?-u)é", "\\a\\f\\t\\n\\r\\v", "\\0", "\\1", "\\8", "\\", "\\<", "\\>",
    "\\b", "\\B", "\\A", "\\z", "\\b{start}", "\\b{end}", "\\b{start-half}", "\\b{end-half}", "\\b{bogus}", "\\b{2}", "\\b{",
    "(?-u)\\b", "(?-u)\\B\\b{start}", "^", "$", "(?m)^$", "(?mR)^$", "(?R)$", ".", "(?s).", "(?R).", "(?-u).", "(?sR).",
    "(?i)a", "(?i)k", "(?i)ß", "(?i)1", "(?i)abc", "(?i-u)k", "(?i-u)é", "a(?i)b", "a(?i)1", "(?i:a)b", "a|(?i)b|c", "((?i)a)b",
    "(?x) a b # comment\n c", "(?x)a\\ b", "(?x)[ a - c ]", "(?x)a{ 2 , 3 }", "a{ 2 }", "(?x)(?-x) a", "(?x:a b)c d",
    "(?U)a*", "(?U)a*?", "(?)", "(?i", "(?i:", "(?-)", "(?i-)", "(?--i)", "(?ii)", "(?z)", "(?u)\\pL", "(?-u)\\pL", "(?-u:\\d)",
    "(?-u)\\D", "(?-u)\\w\\s",
    // groups and repetition
    "()", "(a)", "(?:a)", "(?P<n>a)", "(?<n>a)", "(?P<n>a)(?P<n>b)", "(?P<>a)", "(?P<1>a)", "(?P<a.b[c]>x)", "(?P<n", "(?P<λ>a)",
    "(?=a)", "(?!a)", "(?<=a)", "(?<!a)", "(", ")", "a)", "(a", "((a)|b)", "a|", "|a", "|", "a||b",
    "a*", "a+", "a?", "a*?", "a+?", "a??", "a**", "*", "a|*", "(?i)*", "a{2}", "a{2,}", "a{2,3}", "a{,3}", "a{3,2}", "a{0}",
    "a{1}", "a{0,1}", "a{2}{3}", "a{", "a{2", "a{2,", "a{x}", "{2}", "a{4294967295}", "a{4294967296}", "\\b*", "^*", "(?:)*",
    "(?:^)+", "(?:a|)", "(?:a|b)*", "(?:$|a)*", "a{2,3}?",
    // alternation and its simplifications
    "a|b", "a|b|c", "a|a", "ab|ac", "ab|ac|ad", "abc|abd", "a[b]|a[c]", "[a]|[b]", "a|[bc]", "\\d|\\s", "^a|^b", "a|ab", "ab|a",
    "x(?:ab|ac)y", "(?:ab|ac)|ad", "a*b|a*c",
    // classes
    "[a]", "[abc]", "[a-z]", "[z-a]", "[^a]", "[^\\x00-\\x{10FFFF}]", "[\\x00-\\x{D7FF}\\x{E000}-\\x{10FFFF}]", "[^\\x00-\\x{D7FF}\\x{E000}-\\x{10FFFF}]",
    "[]a]", "[^]a]", "[-a]", "[a-]", "[--a]", "[a-\\d]", "[\\d-a]", "[[:alpha:]]", "[[:^alpha:]]", "[[:bogus:]]", "[[:alpha:]b]",
    "[[a-c][x-z]]", "[a-z&&[^aeiou]]", "[\\w--\\d]", "[a-c~~b-d]", "[a&&b&&c]", "[a--b--c]", "[&&a]", "[a&&]", "[\\pL&&\\p{Greek}]",
    "[", "[a", "[^", "[]", "[^]", "[a-", "[\\b]", "[.]", "[\\A]", "[(?i)a]", "(?i)[a-z]", "(?i)[^a]", "(?i)[k]", "(?i)[a&&A]",
    "(?i)[[:upper:]]", "(?-u)[a-z]", "(?-u)[^a]", "(?-u)[é]", "(?-u)[\\xff]", "(?i-u)[k]", "(?-u)[[:^alpha:]]", "(?-u)[\\w]",
    "\\d", "\\D", "\\s", "\\S", "\\w", "\\W", "[\\d\\s]", "[^\\w]",
    // Unicode classes
    "\\pL", "\\pN", "\\PL", "\\p{L}", "\\p{Letter}", "\\p{Lu}", "\\p{gc=Lu}", "\\p{General_Category=Uppercase_Letter}",
    "\\p{Greek}", "\\p{sc=Greek}", "\\p{scx=Greek}", "\\p{Script_Extensions=Grek}", "\\p{Is_Greek}", "\\p{isc}", "\\p{c}",
    "\\p{cf}", "\\p{sc}", "\\p{lc}", "\\p{Any}", "\\p{ASCII}", "\\p{Assigned}", "\\p{Alphabetic}", "\\p{White_Space}", "\\p{Emoji}",
    "\\p{age=6.0}", "\\p{age:1.1}", "\\p{age=V16_0}", "\\p{age=99}", "\\p{gcb=Extend}", "\\p{wb=ALetter}", "\\p{sb=Upper}",
    "\\p{sc!=Latin}", "\\P{sc!=Latin}", "\\p{Bogus}", "\\p{gc=Bogus}", "\\p{bc=L}", "\\p{Script}", "\\p{Decimal_Number}",
    "\\p{Nd}", "\\p{ Greek }", "\\p{greek}", "\\p{GREEK}", "\\p", "\\p{", "\\p{L", "\\p\\", "\\pZ", "\\p{Zs}", "(?i)\\p{Lu}",
    "(?i)\\P{Ll}", "\\p{Han}", "\\p{Cased_Letter}", "\\p{LC}", "\\p{L&}",
    // nesting
    "((((((((((a))))))))))",
];

#[test]
fn curated_patterns_read_as_regex_syntax_reads_them() {
    let mut patterns: Vec<String> = CURATED.iter().map(|s| s.to_string()).collect();
    patterns.push("(".repeat(249) + "a" + &")".repeat(249));
    patterns.push("(".repeat(251) + "a" + &")".repeat(251));
    patterns.push("[".repeat(126) + "a" + &"]".repeat(126));
    patterns.push("[".repeat(260) + "a" + &"]".repeat(260));
    patterns.push("a*".repeat(3) + &"*".repeat(260));
    same_as_regex_syntax("curated", patterns);
}

/// Pieces random patterns are made of: valid and broken syntax both.
const PIECES: &[&str] = &[
    "a", "b", "k", "K", "s", "ß", "λ", "Σ", "1", "_", " ", "\n", "#", "-", "]", "[", "{", "}", ",", ":", "^", "$", ".", "|", "*", "+",
    "?", "??", "*?", "{2}", "{1,3}", "{2,}", "{0}", "{,2}", "{3,1}", "(", ")", "(?:", "(?i)", "(?-i)", "(?x)", "(?m)", "(?s)", "(?U)",
    "(?-u)", "(?R)", "(?i:", "(?x:", "(?P<n>", "(?<m>", "\\d", "\\w", "\\s", "\\D", "\\W", "\\S", "\\b", "\\B", "\\A", "\\z",
    "\\b{start}", "\\<", "\\>", "\\x41", "\\xff", "\\x{1F600}", "\\u00e9", "\\.", "\\", "\\pL", "\\p{Greek}", "\\P{Lu}", "\\p{gc=Nd}",
    "\\p{sc!=Latin}", "\\p{age:6.0}", "[a-z]", "[^a]", "[[:alpha:]]", "[[:^digit:]]", "[a-z&&[^aeiou]]", "[\\w--\\d]", "[a~~b]",
    "[k]", "[^\\s]", "[-a]", "[]]", "&&", "--", "~~", "a-z", "[", "[^",
];

fn random_pattern(rng: &mut impl RngExt) -> String {
    let n = rng.random_range(1..=8);
    (0..n).map(|_| PIECES[rng.random_range(0..PIECES.len())]).collect()
}

#[test]
fn random_patterns_read_as_regex_syntax_reads_them() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(std::env::var("REGEX_SEED").ok().and_then(|n| n.parse().ok()).unwrap_or(7));
    let patterns: Vec<String> = (0..std::env::var("REGEX_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(5000)).map(|_| random_pattern(&mut rng)).collect();
    same_as_regex_syntax("random", patterns);
}
