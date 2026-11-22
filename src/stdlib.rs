//! The standard library: the `std/` package, built into the binary so any
//! program can `use std.io.*` without a dependency. Its modules are named
//! `std.<path>` wherever they are used, including when `std/` itself is the
//! package being checked.
//!
//! The standard library is written in Strata. Only the thin layer that talks
//! to the operating system is native: the `host_*` builtins below, which only
//! code inside `std` can call. Programs reach the system through capabilities
//! (`Console`, `Files`, `Environment`, `Code`), and the only real implementations are
//! private types of `std` that the runtime hands to `main`.

use crate::types::Builtin;
use crate::ast::Program;
use std::sync::OnceLock;

/// The package name whose modules are prefixed with `std.`.
pub const NAME: &str = "std";

/// Its module files (binary, docs/binary-format.md); their modules are named `std.<path>`.
pub const FILES: &[(&str, &[u8])] = &[
    ("io.strata", include_bytes!("../std/io.strata")),
    ("fs.strata", include_bytes!("../std/fs.strata")),
    ("process.strata", include_bytes!("../std/process.strata")),
    ("time.strata", include_bytes!("../std/time.strata")),
    ("cli.strata", include_bytes!("../std/cli.strata")),
    ("web.strata", include_bytes!("../std/web.strata")),
    ("http.strata", include_bytes!("../std/http.strata")),
    ("typing.strata", include_bytes!("../std/typing.strata")),
    ("json.strata", include_bytes!("../std/json.strata")),
    ("toml.strata", include_bytes!("../std/toml.strata")),
    ("code.strata", include_bytes!("../std/code.strata")),
    ("unicode_tables.strata", include_bytes!("../std/unicode_tables.strata")),
    ("unicode.strata", include_bytes!("../std/unicode.strata")),
    ("regex.strata", include_bytes!("../std/regex.strata")),
    ("utf8.strata", include_bytes!("../std/utf8.strata")),
];

/// Native operations, callable only from `std`.
/// Failures are `(kind, message)`; `std` turns them into its own error types.
pub const HOST: &[Builtin] = &[
    Builtin::new("host_write", "", "(Int, String) -> ()"),
    Builtin::new("host_read_line", "", "() -> Option[String]"),
    Builtin::new("host_read_file", "", "(String) -> Result[String, (String, String)]"),
    // the file's bytes, each an `Int` from 0 to 255
    Builtin::new("host_read_bytes", "", "(String) -> Result[List[Int], (String, String)]"),
    Builtin::new("host_write_file", "", "(String, String, Bool) -> Result[(), (String, String)]"),
    Builtin::new("host_exists", "", "(String) -> Bool"),
    Builtin::new("host_list_dir", "", "(String) -> Result[List[String], (String, String)]"),
    Builtin::new("host_make_dir", "", "(String) -> Result[(), (String, String)]"),
    Builtin::new("host_remove", "", "(String) -> Result[(), (String, String)]"),
    Builtin::new("host_args", "", "() -> List[String]"),
    Builtin::new("host_env_var", "", "(String) -> Option[String]"),
    // the monotonic clock, in nanoseconds; waiting (a task that is cancelled stops at once)
    Builtin::new("host_now", "", "() -> Int"),
    Builtin::new("host_sleep", "", "(Int) -> ()"),
    Builtin::new("host_code_version", "", "() -> String"),
    Builtin::new("host_code_snapshot", "", "() -> Result[std.code.Snapshot, String]"),
];

/// The private types that reach the real system. Only `strata run` makes
/// them (for `main`), `verify` never runs them, and fuzzing never picks them
/// for a type parameter.

pub fn is_host(name: &str) -> bool {
    HOST.iter().any(|h| h.name == name)
}

/// Whether a module belongs to the standard library.
pub fn is_std(module: &str) -> bool {
    module == NAME || module.starts_with("std.")
}

/// The standard library's modules, decoded, named `std.<path>`.
pub fn program() -> &'static Program {
    static P: OnceLock<Program> = OnceLock::new();
    P.get_or_init(|| {
        let mut decls = Vec::new();
        for (file, bytes) in FILES {
            decls.extend(crate::binary::decode_module(bytes).unwrap_or_else(|e| panic!("std/{file}: {e}")));
        }
        Program { decls }
    })
}

/// Whether a definition calls `matches` (compiled code matches with
/// `std.regex`: a literal pattern's matcher is made while compiling) or
/// `s.graphemes()` (`std.unicode`'s): std is loaded for them.
fn run_time_pattern(d: &crate::ast::DeclKind) -> bool {
    use crate::ast::Expr;
    let mut found = false;
    let mut k = d.clone();
    crate::ast::decl_all_exprs_mut(&mut k, |e| {
        found = found
            || match e {
                Expr::Call(f, args) => matches!(&**f, Expr::Var(n) if n.as_str() == "matches") && args.len() == 2,
                Expr::Method(_, m, args) => (m.as_str() == "matches" && args.len() == 1) || (m.as_str() == "graphemes" && args.is_empty()),
                _ => false,
            }
    });
    found
}

/// `p` with the standard library added after its own definitions, if it uses
/// it and does not already contain it (when `std/` itself is being checked).
pub fn with_std(p: &Program) -> Option<Program> {
    use crate::ast::DeclKind;
    // (`matches` is std.regex's, compiled; `s.graphemes()` std.unicode's)
    let uses = p.decls.iter().any(|d| matches!(&d.kind, DeclKind::Use(u) if is_std(&u.path))) || p.decls.iter().any(|d| run_time_pattern(&d.kind));
    // a resolved program has it already, under qualified names in the root module
    let has_std = p.decls.iter().any(|d| {
        // (the prelude's code is added to every program: it says nothing about std)
        (is_std(&d.module) && d.module != crate::types::PRELUDE_MODULE)
            || match &d.kind {
                DeclKind::Type(t) => is_std(&t.name),
                DeclKind::Trait(t) => is_std(&t.name),
                DeclKind::Fn(f) => is_std(&f.name),
                _ => false,
            }
    });
    if !uses || has_std {
        return None;
    }
    let mut out = p.clone();
    out.decls.extend(program().decls.iter().cloned());
    Some(out)
}

/// The value of `std.regex.<name>(args)` (literals), computed as
/// compile-time code is (on the JIT, cached between runs, and here per call):
/// the compiler's own uses of `std.regex` (the prover's relations between
/// patterns, the generator's samples), written as a literal.
pub fn regex_value(name: &str, args: Vec<crate::ast::Expr>) -> Result<crate::ast::Expr, String> {
    use crate::ast::{Expr, Symbol};
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static STD: Option<Program> = crate::parser::parse_program("use std.regex.*\n").ok().map(|p| crate::resolve::resolve(&p).program);
        static DONE: RefCell<HashMap<String, Result<Expr, String>>> = Default::default();
    }
    let call = Expr::Call(Box::new(Expr::Var(Symbol::new(&format!("std.regex.{name}")))), args);
    let key = crate::printer::expr(&call, 0);
    if let Some(v) = DONE.with(|d| d.borrow().get(&key).cloned()) {
        return v;
    }
    let v = STD.with(|p| match p {
        Some(p) => crate::derive::comptime_literals(p, std::slice::from_ref(&call)).pop().unwrap_or_else(|| Err("no value".into())),
        None => Err("std.regex could not be read".into()),
    });
    DONE.with(|d| d.borrow_mut().insert(key, v.clone()));
    v
}
