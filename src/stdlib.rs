//! The standard library: the `std/` package, built into the binary so any
//! program can `use std.io.*` without a dependency. Its modules are named
//! `std.<path>` wherever they are used, including when `std/` itself is the
//! package being checked.
//!
//! The standard library is written in Strata. Only the thin layer that talks
//! to the operating system is native: the `host_*` builtins below, which only
//! code inside `std` can call. Programs reach the system through capabilities
//! (`Console`, `Files`, `Environment`), and the only real implementations are
//! private types of `std` that the runtime hands to `main`.

use crate::types::Builtin;
use crate::ast::Program;
use std::sync::OnceLock;

/// The package name whose modules are prefixed with `std.`.
pub const NAME: &str = "std";

const FILES: &[(&str, &str)] = &[
    ("io.strata", include_str!("../std/io.strata")),
    ("fs.strata", include_str!("../std/fs.strata")),
    ("process.strata", include_str!("../std/process.strata")),
    ("cli.strata", include_str!("../std/cli.strata")),
    ("web.strata", include_str!("../std/web.strata")),
    ("http.strata", include_str!("../std/http.strata")),
];

/// Native operations, callable only from `std`.
/// Failures are `(kind, message)`; `std` turns them into its own error types.
pub const HOST: &[Builtin] = &[
    Builtin::new("host_write", "", "(Int, String) -> ()"),
    Builtin::new("host_read_line", "", "() -> Option[String]"),
    Builtin::new("host_read_file", "", "(String) -> Result[String, (String, String)]"),
    Builtin::new("host_write_file", "", "(String, String, Bool) -> Result[(), (String, String)]"),
    Builtin::new("host_exists", "", "(String) -> Bool"),
    Builtin::new("host_list_dir", "", "(String) -> Result[List[String], (String, String)]"),
    Builtin::new("host_make_dir", "", "(String) -> Result[(), (String, String)]"),
    Builtin::new("host_remove", "", "(String) -> Result[(), (String, String)]"),
    Builtin::new("host_args", "", "() -> List[String]"),
    Builtin::new("host_env_var", "", "(String) -> Option[String]"),
];

/// The private types that reach the real system. Only `strata run` makes
/// them (for `main`), `verify` never runs them, and fuzzing never picks them
/// for a type parameter.
pub const HOST_TYPES: &[&str] = &["std.io.Terminal", "std.fs.Disk", "std.process.Process"];

pub fn is_host(name: &str) -> bool {
    HOST.iter().any(|h| h.name == name)
}

/// Whether a module belongs to the standard library.
pub fn is_std(module: &str) -> bool {
    module == NAME || module.starts_with("std.")
}

/// The standard library's modules, parsed, named `std.<path>`.
pub fn program() -> &'static Program {
    static P: OnceLock<Program> = OnceLock::new();
    P.get_or_init(|| {
        let mut decls = Vec::new();
        for (file, src) in FILES {
            let module = format!("{NAME}.{}", file.trim_end_matches(".strata").replace('/', "."));
            let p = crate::parser::parse_module_file(src, &module).unwrap_or_else(|e| panic!("std/{file}: {e}"));
            decls.extend(p.decls);
        }
        Program { decls }
    })
}

/// `p` with the standard library added after its own definitions, if it uses
/// it and does not already contain it (when `std/` itself is being checked).
pub fn with_std(p: &Program) -> Option<Program> {
    use crate::ast::DeclKind;
    let uses = p.decls.iter().any(|d| matches!(&d.kind, DeclKind::Use(u) if is_std(&u.path)));
    // a resolved program has it already, under qualified names in the root module
    let has_std = p.decls.iter().any(|d| {
        is_std(&d.module)
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
