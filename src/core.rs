//! `core`: what every program has without a `use`, below the standard
//! library (docs/internals.md, "core"). The `core/` package, built into the
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
    ("mem.strata", include_bytes!("../core/mem.strata")),
    ("iter.strata", include_bytes!("../core/iter.strata")),
    ("channel.strata", include_bytes!("../core/channel.strata")),
    ("text.strata", include_bytes!("../core/text.strata")),
    ("task.strata", include_bytes!("../core/task.strata")),
    ("rt.strata", include_bytes!("../core/rt.strata")),
];

/// The modules of code a program gets when it uses them (`needed`):
/// iterators, channels, a string's units, supervised tasks' `Panic`.
pub const CODE: &[&str] = &["core.iter", "core.channel", "core.text", "core.task"];

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

/// The code modules (`CODE`), as written.
pub fn code() -> Program {
    Program { decls: CODE.iter().flat_map(|m| module(&format!("{}.strata", m.trim_start_matches("core.")))).filter(|d| !is_builtin(d)).cloned().collect() }
}

fn is_builtin(d: &Decl) -> bool {
    matches!(&d.kind, DeclKind::Extern(e) if e.lang == Lang::Builtin)
}

/// The names in a definition that can pick a code module: variables and
/// functions, methods, constructors, types, traits (each as written, short).
fn names_in(k: &DeclKind, out: &mut std::collections::HashSet<String>) {
    use crate::ast::{Expr, TypeExpr, Visit};
    struct Names<'o>(&'o mut std::collections::HashSet<String>);
    impl Visit for Names<'_> {
        fn expr(&mut self, e: &Expr) {
            match e {
                Expr::Var(x) => {
                    self.0.insert(crate::names::short(x).to_string());
                }
                Expr::Method(_, m, _) => {
                    self.0.insert(crate::names::short(m).to_string());
                }
                Expr::Ctor(c, _) => {
                    self.0.insert(crate::names::short(c).to_string());
                }
                // a supervised task's result is `Result[T, Panic]`
                Expr::SupervisedSpawn(_) => {
                    self.0.insert("Panic".into());
                }
                _ => {}
            }
            crate::ast::walk_expr(self, e)
        }
        fn ty(&mut self, t: &TypeExpr) {
            if let TypeExpr::Named(n, _) = t {
                self.0.insert(crate::names::short(n).to_string());
            }
            crate::ast::walk_ty(self, t)
        }
    }
    match k {
        DeclKind::Impl(i) => {
            out.insert(crate::names::short(&i.trait_name).to_string());
        }
        // a type or trait named like core's: its module comes in, so the
        // clash is said (`ListIter` is already defined in the prelude)
        DeclKind::Type(t) => {
            out.insert(crate::names::short(&t.name).to_string());
        }
        DeclKind::Trait(t) => {
            out.insert(crate::names::short(&t.name).to_string());
        }
        _ => {}
    }
    crate::ast::walk_decl(&mut Names(out), k);
}

/// What picks a code module: the names it defines (types, traits and their
/// methods, functions, builtins), but those of a builtin defined elsewhere
/// (`map`, `len`: a list's `xs.map(f)` needs no iterator).
fn triggers(m: &str) -> std::collections::HashSet<String> {
    let elsewhere: std::collections::HashSet<&str> = builtin_fns().filter(|(bm, _)| bm.as_str() != m).map(|(_, f)| f.name.as_str()).collect();
    let mut out = std::collections::HashSet::default();
    for d in module(&file_of(m)) {
        match &d.kind {
            DeclKind::Type(t) => {
                out.insert(t.name.to_string());
            }
            DeclKind::Trait(t) => {
                out.insert(t.name.to_string());
                out.extend(t.methods.iter().map(|x| x.decl.name.to_string()));
            }
            DeclKind::Fn(f) => {
                out.insert(f.name.to_string());
            }
            DeclKind::Extern(e) => match &e.item {
                ExternItem::Fn(f) => {
                    out.insert(f.name.to_string());
                }
                ExternItem::Type(t, _) | ExternItem::Static(t, _) => {
                    out.insert(t.to_string());
                }
            },
            _ => {}
        }
    }
    out.retain(|n| !elsewhere.contains(n.as_str()));
    out
}

pub fn file_of(m: &str) -> String {
    format!("{}.strata", m.trim_start_matches("core."))
}

/// The code modules `p` uses: those it names something of (`xs.iter()`,
/// `channel()`, `s.bytes()`, `supervised spawn`), and those these use in turn.
pub fn needed(p: &Program) -> std::collections::HashSet<&'static str> {
    static T: OnceLock<Vec<(&'static str, std::collections::HashSet<String>)>> = OnceLock::new();
    let triggers = T.get_or_init(|| CODE.iter().map(|m| (*m, triggers(m))).collect());
    let mut names = std::collections::HashSet::default();
    for d in &p.decls {
        names_in(&d.kind, &mut names);
    }
    let mut out = std::collections::HashSet::default();
    loop {
        let more: Vec<&'static str> = triggers.iter().filter(|(m, t)| !out.contains(m) && t.iter().any(|n| names.contains(n))).map(|(m, _)| *m).collect();
        if more.is_empty() {
            return out;
        }
        for m in more {
            out.insert(m);
            for d in module(&file_of(m)) {
                names_in(&d.kind, &mut names);
            }
        }
    }
}

/// The builtin types (`builtin type List[T]`, and core.mem's `Ptr`): their
/// names and parameters.
pub fn builtin_types() -> &'static [(Symbol, Vec<Symbol>)] {
    static T: OnceLock<Vec<(Symbol, Vec<Symbol>)>> = OnceLock::new();
    T.get_or_init(|| {
        FILES
            .iter()
            .flat_map(|(f, _)| module(f))
            .filter_map(|d| match &d.kind {
                DeclKind::Extern(e) if e.lang == Lang::Builtin => match &e.item {
                    ExternItem::Type(t, ps) => Some((*t, ps.clone())),
                    ExternItem::Fn(_) | ExternItem::Static(..) => None,
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
    FILES.iter().flat_map(|(f, _)| module(f)).filter_map(|d| match &d.kind {
        DeclKind::Extern(e) if e.lang == Lang::Builtin => match &e.item {
            ExternItem::Fn(f) => Some((d.module, f)),
            ExternItem::Type(..) | ExternItem::Static(..) => None,
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

/// A core module's definitions (by its file, `rt.strata`), as written.
pub fn module_decls(file: &str) -> Vec<Decl> {
    module(file).to_vec()
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
    let path = dir.join(file_of(&m));
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

/// core's own tests (`test` blocks in its modules), as a program, with
/// the functions of the modules programs don't get by themselves (core.rt's).
pub fn tests() -> Program {
    let decls = FILES
        .iter()
        .flat_map(|(f, _)| module(f).iter().filter(move |d| matches!(d.kind, DeclKind::Test(_)) || (*f == "rt.strata" && (!is_builtin(d) || matches!(&d.kind, DeclKind::Extern(e) if matches!(e.item, ExternItem::Static(..)))))).cloned())
        .collect();
    Program { decls }
}

#[cfg(test)]
mod tests {
    fn needed(src: &str) -> Vec<&'static str> {
        let p = crate::parser::parse_program(src).expect("parses");
        let mut m: Vec<&str> = super::needed(&p).into_iter().collect();
        m.sort();
        m
    }

    /// A program gets only the code of core it uses.
    #[test]
    fn core_code_is_included_when_used() {
        // a list's `map` and `len` are builtins: no iterators
        assert!(needed("fn main() {\n  print(len([1, 2].map(x -> x + 1)))\n}\n").is_empty());
        assert_eq!(needed("fn main() {\n  for b in \"hi\".bytes() {\n    print(b)\n  }\n}\n"), ["core.iter", "core.text"]);
        assert_eq!(needed("fn main() {\n  xs := [1, 2].iter().take(1).to_list()\n}\n"), ["core.iter"]);
        assert_eq!(needed("fn main() {\n  (tx, rx) := channel()\n  tx.close()\n}\n"), ["core.channel", "core.iter"]);
        assert_eq!(needed("fn main() {\n  scope {\n    h := supervised spawn 1\n    await h\n  }\n}\n"), ["core.task"]);
        // implementing a trait of core uses its module
        assert_eq!(needed("type Count = { n: Int }\n\nimpl Iterator[Int] for Count {\n  fn next(mut self) -> Option[Int] {\n    None\n  }\n}\n"), ["core.iter"]);
    }
}
