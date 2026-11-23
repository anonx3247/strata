//! `compiler/eval` (the compiler package's `eval` command) runs programs
//! from their stored trees, in Strata, as the compiled programs run them: for each program of tests/build.rs below, it
//! prints what the compiled program printed, ends with the same fault or
//! error, and gives the same exit code (the outputs recorded in
//! tests/data/build/expected).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const STRATA: &str = env!("CARGO_BIN_EXE_strata");

/// std's module files (what a program may `use`).
fn std_files() -> Vec<PathBuf> {
    let mut out = vec![];
    let mut todo = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("std")];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                // (not `std/.strata`, the log's)
                if !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                    todo.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "strata") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-eval-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn have_clang() -> bool {
    let cc = std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into());
    Command::new(cc).arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Store a program written as text at `file` (a module file).
fn save(file: &Path, text: &str) {
    let mut c = Command::new(STRATA).args(["--file", &file.to_string_lossy(), "edit"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "{}: {}", file.display(), String::from_utf8_lossy(&o.stderr));
}

fn strata(args: &[&str]) {
    let o = Command::new(STRATA).args(args).output().unwrap();
    assert!(o.status.success(), "strata {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

#[path = "common/compiler.rs"]
mod compiler;

#[path = "common/programs.rs"]
mod programs;

#[path = "common/json.rs"]
mod json_case;

/// What a program must print and exit with: the output recorded under its
/// key in tests/data/build/expected, or an output given here (exit code 0).
enum Want {
    Recorded,
    Text(&'static str),
}

/// The programs of tests/build.rs that the evaluator runs: the expected
/// output's key, the program's text, and what it must print.
fn cases() -> Vec<(String, String, Want)> {
    let mut out = vec![];
    let groups: [(&str, &[(&str, &str, i32)]); 4] = [("faults", programs::FAULTS), ("data", programs::DATA), ("traits", programs::TRAITS), ("dyn", programs::DYN)];
    for (g, cases) in groups {
        for (name, text, _) in cases {
            out.push((format!("{g}-{name}"), text.to_string(), Want::Recorded));
        }
    }
    for (g, cases) in [("collections", programs::COLLECTION_FAULTS), ("capabilities", programs::CAPABILITIES)] {
        for (name, text) in cases {
            out.push((format!("{g}-{name}"), text.to_string(), Want::Recorded));
        }
    }
    for (key, text) in [
        ("capabilities-args", programs::ARGS.to_string()),
        ("collections-strings", programs::STRINGS.to_string()),
        ("collections-dicts_and_sets", programs::COLLECTIONS.to_string()),
        ("collections-builtins_example", programs::tests_as_main("examples/builtins.strata", "")),
        ("traits-traits_example", programs::tests_as_main("examples/traits.strata", "")),
        ("lists-math", programs::math()),
        ("small-gaps-fields", programs::FIELDS.to_string()),
        ("small-gaps-trait_equality", programs::TRAIT_EQUALITY.to_string()),
        ("declared_unions-declared", programs::DECLARED_UNIONS.to_string()),
        ("unions-unions", programs::UNIONS.to_string()),
        ("unions-union_methods", programs::UNION_METHODS.to_string()),
        ("trait-methods-trait_methods", programs::TRAIT_METHODS.to_string()),
        ("memory-features", programs::MEMORY.to_string()),
        ("memory-nested_assign", programs::NESTED_ASSIGN.to_string()),
        ("bigint-bigint", programs::BIG_INTEGERS.to_string()),
        ("floats-checked", programs::CHECKED_FLOATS.to_string()),
    ] {
        out.push((key.to_string(), text, Want::Recorded));
    }
    for (key, text, want) in [
        ("traits-traits_example", "`T.empty()` in a generic function: the type the caller asks for"),
    ("impl-statics-statics", json_case::STATICS, json_case::STATICS_OUTPUT),
        ("impl-statics-text", json_case::TEXT, json_case::TEXT_OUTPUT),
        ("impl-statics-self_methods", json_case::SELF_METHODS, json_case::SELF_METHODS_OUTPUT),
        ("impl-statics-root_builtin_name", json_case::ROOT_BUILTIN_NAME, json_case::ROOT_BUILTIN_NAME_OUTPUT),
        ("impl-statics-builtins", json_case::BUILTINS, json_case::BUILTINS_OUTPUT),
        ("impl-statics-option_option", json_case::OPTION_OPTION, json_case::OPTION_OPTION_OUTPUT),
        ("json-program", json_case::PROGRAM, json_case::OUTPUT),
        ("impl-statics-generic_derive", json_case::GENERIC_DERIVE, json_case::GENERIC_DERIVE_OUTPUT),
        ("impl-statics-refined", json_case::REFINED, json_case::REFINED_OUTPUT),
        ("iterators-iterators", json_case::ITERATORS, json_case::ITERATORS_OUTPUT),
        ("iterators-lazy", json_case::LAZY_ITERATORS, json_case::LAZY_ITERATORS_OUTPUT),
        ("iterators-comps", json_case::ITERATOR_COMPREHENSIONS, json_case::ITERATOR_COMPREHENSIONS_OUTPUT),
        ("iterators-collections", json_case::COLLECTION_ITERATORS, json_case::COLLECTION_ITERATORS_OUTPUT),
        ("iterators-string_units", json_case::STRING_UNITS, json_case::STRING_UNITS_OUTPUT),
        ("iterators-indexing", json_case::ITERATOR_INDEXING, json_case::ITERATOR_INDEXING_OUTPUT),
    ] {
        out.push((key.to_string(), text.to_string(), Want::Text(want)));
    }
    out.retain(|(k, _, _)| !NOT_YET.iter().any(|(n, _)| n == k));
    out
}

/// Programs the evaluator doesn't run yet, and why.
const NOT_YET: &[(&str, &str)] = &[
    ("capabilities-files_and_environment", "files: the evaluator gives a program no `Disk` yet"),
    // a generic function's type parameter known only from what the call
    // must return (`read[T]` in `fn ints() -> Result[List[Int], String] { read(..) }`):
    // needs the checker's types
    ("impl-statics-builtins", "`T.from_json(..)` in `read[T]`: the type the caller asks for"),
    ("impl-statics-refined", "`T.from_json(..)` in `read[T]`: the type the caller asks for"),
    ("impl-statics-self_methods", "`T.zero()` where no value shows `T` (`Cell { v: None }`)"),
    ("impl-statics-statics", "`T.zero()` in `zero_of[T]()`: the type the caller asks for"),
];

/// The stored programs of tests/data/build.
const FILES: &[&str] = &["numbers", "lists", "data"];

#[test]
fn programs_run_as_compiled() {
    if !have_clang() {
        eprintln!("skipped: no clang");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let d = scratch("programs");
    let prelude = d.join("prelude.strata");
    strata(&["pack", "--prelude", &prelude.to_string_lossy()]);
    let mut programs: Vec<(String, PathBuf, Want)> = FILES.iter().map(|f| (f.to_string(), root.join("tests/data/build").join(format!("{f}.strata")), Want::Recorded)).collect();
    for (key, text, want) in cases() {
        let f = d.join(format!("{key}.strata"));
        save(&f, &text);
        programs.push((key, f, want));
    }
    let mut c = Command::new(compiler::exe());
    c.arg("eval").arg(&prelude).args(std_files()).arg("--").args(programs.iter().map(|p| &p.1));
    let o = c.output().unwrap();
    assert!(o.status.success(), "the checker failed: {}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8(o.stdout).unwrap();
    let runs: Vec<&str> = out.split("===\n").collect();
    let mut wrong = vec![];
    for ((key, _, want), got) in programs.iter().zip(&runs) {
        // the recorded form: the output, a blank line, then the exit code
        let (text, exit) = got.rsplit_once("--- exit ").unwrap_or((got, "?\n"));
        let got = format!("{text}\n--- exit {exit}");
        let want = match want {
            Want::Recorded => std::fs::read_to_string(root.join("tests/data/build/expected").join(format!("{key}.out"))).unwrap(),
            Want::Text(t) => format!("{t}\n--- exit 0\n"),
        };
        if got != want {
            wrong.push(format!("{key}:\n--- compiler/eval\n{got}--- compiled\n{want}"));
        }
    }
    assert_eq!(runs.len(), programs.len() + 1, "{out}");
    assert!(wrong.is_empty(), "{} of {} programs differ:\n{}", wrong.len(), programs.len(), wrong.join("\n"));
}
