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
];

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

/// The builtin types (`builtin type List[T]`): their names and parameters.
pub fn builtin_types() -> &'static [(Symbol, Vec<Symbol>)] {
    static T: OnceLock<Vec<(Symbol, Vec<Symbol>)>> = OnceLock::new();
    T.get_or_init(|| {
        module("types.strata")
            .iter()
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
/// written (overloads are tried in that order).
pub fn builtin_fns() -> impl Iterator<Item = &'static crate::ast::FnDecl> {
    module("builtins.strata").iter().filter_map(|d| match &d.kind {
        DeclKind::Extern(e) if e.lang == Lang::Builtin => match &e.item {
            ExternItem::Fn(f) => Some(f),
            ExternItem::Type(..) => None,
        },
        _ => None,
    })
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
