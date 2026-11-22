//! `core`: what every program has without a `use`, below the standard
//! library (docs/language.md, "core"). The `core/` package, built into the
//! binary as std is: the builtin types and functions the compiler provides
//! (`builtin type Int`, `builtin fn len[A](xs: List[A]) -> Int`, with
//! `requires` where they can fail), the types and traits every program
//! names (`Option`, `Result`, `Debug`, the refinements), and the code every
//! program gets (iterators, channels' methods, a string's units).
//!
//! `core` depends on nothing; std depends on it. Only `core` may declare
//! `builtin` definitions.

/// The package's name, and its modules' prefix (`core.types`).
pub const NAME: &str = "core";

/// Whether a module belongs to `core` (`core.types`, ...: core has no root
/// module, so a package's own module named `core` isn't one).
pub fn is_core(module: &str) -> bool {
    module.starts_with("core.")
}

use crate::ast::{Decl, DeclKind, ExternItem, Lang, Program, Symbol};
use std::sync::OnceLock;

/// Its module files (binary, docs/binary-format.md), named `core.<path>`.
pub const FILES: &[(&str, &[u8])] = &[
    ("types.strata", include_bytes!("../core/types.strata")),
    ("builtins.strata", include_bytes!("../core/builtins.strata")),
    ("prelude.strata", include_bytes!("../core/prelude.strata")),
    ("mem.strata", include_bytes!("../core/mem.strata")),
];

/// The module of raw memory (`Ptr`, `raw_alloc`, `load_i64`, ...): only
/// core's own modules can use it.
pub const MEM: &str = "core.mem";

/// The definitions of module `core.<file>`, decoded once.
fn module(file: &str) -> &'static [Decl] {
    static M: OnceLock<Vec<(String, Vec<Decl>)>> = OnceLock::new();
    let all = M.get_or_init(|| {
        FILES.iter().map(|(f, bytes)| (f.to_string(), crate::binary::decode_module(bytes).unwrap_or_else(|e| panic!("core/{f}: {e}")))).collect()
    });
    all.iter().find(|(f, _)| f == file).map(|(_, ds)| ds.as_slice()).unwrap_or(&[])
}

/// `core.types`: the builtin types, and the types and traits every program
/// names (`Option`, `Debug`, `Natural`, ...), as written: their names are
/// global, nobody's module's.
pub fn types() -> Program {
    let mut decls = module("types.strata").to_vec();
    for d in &mut decls {
        d.module = Symbol::EMPTY;
    }
    Program { decls }
}

/// `core.prelude`: the code every program gets (iterators, channels'
/// methods, a string's units), as written.
pub fn prelude() -> Program {
    Program { decls: module("prelude.strata").to_vec() }
}

/// The builtin types (`builtin type List[T]`, and core.mem's `Ptr`): their
/// names and parameters.
pub fn builtin_types() -> &'static [(Symbol, Vec<Symbol>)] {
    static T: OnceLock<Vec<(Symbol, Vec<Symbol>)>> = OnceLock::new();
    T.get_or_init(|| {
        module("types.strata")
            .iter()
            .chain(module("mem.strata"))
            .filter_map(|d| match &d.kind {
                DeclKind::Extern(e) if e.lang == Lang::Builtin => match &e.item {
                    ExternItem::Type(t, ps) => Some((*t, ps.clone())),
                    ExternItem::Fn(_) => None,
                },
                _ => None,
            })
            .collect()
    })
}

/// The builtin functions (`builtin fn`), each signature in the order
/// written (overloads are tried in that order), with their module
/// (`core.builtins`, `core.mem`).
pub fn builtin_fns() -> impl Iterator<Item = (Symbol, &'static crate::ast::FnDecl)> {
    module("builtins.strata").iter().chain(module("mem.strata")).filter_map(|d| match &d.kind {
        DeclKind::Extern(e) if e.lang == Lang::Builtin => match &e.item {
            ExternItem::Fn(f) => Some((d.module, f)),
            ExternItem::Type(..) => None,
        },
        _ => None,
    })
}

/// Whether the definition named `name` in `module` is core's own (one of
/// its files' definitions): what may use core.mem. Once resolved, a
/// definition's module is in its name (`core.mem.f`, `test "core.mem: ..."`).
/// A package's module can't pass for one by its name alone: the definition
/// must be one core has.
pub fn owns(module: &str, name: &str) -> bool {
    let (module, short) = if is_core(module) {
        (module.to_string(), crate::names::short(name).to_string())
    } else if let Some(label) = name.strip_prefix("test \"") {
        match label.split_once(": ") {
            Some((m, rest)) => (m.to_string(), format!("test \"{rest}")),
            None => return false,
        }
    } else {
        match name.rsplit_once('.') {
            Some((m, n)) => (m.to_string(), n.to_string()),
            None => return false,
        }
    };
    is_core(&module) && FILES.iter().any(|(f, _)| module_of(f) == module && crate::core::module(f).iter().any(|d| d.name() == short))
}

/// `core.mem` for `mem.strata`.
fn module_of(file: &str) -> String {
    format!("{NAME}.{}", file.trim_end_matches(".strata"))
}

/// The builtins that can fail in ways not yet written as contracts: a
/// pattern that isn't a regular expression, a channel used after it ended.
/// They aren't total, though they have no `requires`.
pub const NOT_YET_CONTRACTS: &[&str] = &["matches", "channel_send", "channel_recv", "channel_close"];

/// The `core/` folder named by `--file`, if that is what it names.
pub fn dir_of(file: Option<&str>) -> Option<std::path::PathBuf> {
    let f = std::path::Path::new(file?);
    let pkg = crate::package::find(f).ok().flatten().filter(|p| p.name == NAME)?;
    f.is_dir().then_some(pkg.dir)
}

/// `strata --file core/ render [MODULE]`: a module as stored (core's names
/// are global, so there is nothing to shorten).
pub fn render(dir: &std::path::Path, module: Option<&str>) -> Result<String, String> {
    let m = crate::stdlib::package_module(NAME, module.unwrap_or("types"));
    let path = dir.join(crate::store::module_path(&m));
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let decls = crate::binary::decode_module(&bytes)?;
    Ok(crate::printer::module_file(&Program { decls }, &m))
}

/// `strata --file core/ check`: core as this strata has it built in (its
/// files on disk must be those: rebuild after an edit) checks: its code, in
/// a program of its own, and its builtins' contracts.
pub fn check(dir: &std::path::Path) -> crate::types::CheckReport {
    let mut rep = crate::types::check(&Program { decls: vec![] });
    for (f, bytes) in FILES {
        if std::fs::read(dir.join(f)).ok().as_deref() != Some(*bytes) {
            rep.errors.push(crate::types::Diag { decl: format!("core/{f}"), msg: "differs from the core this strata was built with: rebuild strata, then check".into() });
        }
    }
    let builtins = Program { decls: module("builtins.strata").to_vec() };
    // (as written: resolving would name them `core.builtins.len`)
    rep.errors.extend(crate::types::check_report_resolved(&builtins).errors);
    rep
}

/// core's own tests (`test` blocks in its modules), as a program.
pub fn tests() -> Program {
    let decls = FILES.iter().flat_map(|(f, _)| module(f).iter().filter(|d| matches!(d.kind, DeclKind::Test(_))).cloned()).collect();
    Program { decls }
}
