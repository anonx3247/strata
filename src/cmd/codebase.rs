//! Commands that change the package: init, add and edit, rename, move, and pending changes.

use crate::*;

/// `strata init`.
pub fn init() -> Result<bool, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let toml = Codebase::init(&cwd)?;
    println!("wrote {}: {}", package::FILE, toml.trim());
    Ok(true)
}

/// The text an `add` or a piped `edit` brings in, as definitions: `add`'s
/// file (text, or a module file), `edit`'s standard input; unqualified
/// definitions go into the module named (`strata edit shop < defs`).
fn edits_in(args: &Args, cmd: &str, p: &Program, default_module: &str) -> Result<Program, String> {
    let module = match args.pos.get(if cmd == "add" { 2 } else { 1 }) {
        None => default_module.to_string(),
        Some(arg) => edit_module(p, arg).unwrap_or_else(|| arg.clone()),
    };
    let text = if cmd == "add" {
        let f = args.pos.get(1).ok_or("usage: strata add <FILE> [MODULE]  (a text file, or a .strata file or folder)")?;
        let path = std::path::Path::new(f);
        let bytes = if path.is_dir() { vec![] } else { std::fs::read(path).map_err(|e| format!("{f}: {e}"))? };
        if path.is_dir() || binary::is_binary(&bytes) {
            return read_sources(f);
        }
        String::from_utf8(bytes).map_err(|_| format!("{f}: neither text nor a Strata file"))?
    } else {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
        s
    };
    parser::parse_module_file(&text, &module).map_err(|e| format!("parse error at {e}"))
}

/// `strata add | edit`.
pub fn edit(args: &Args, cmd: &str, json_out: bool) -> Result<bool, String> {
    if let Some(f) = args.val("--file") {
        return edit_file(args, cmd, f);
    }
    let Source::Db(mut cb) = load(&args)? else { return Err("`add`/`edit` need a codebase".into()) };
    // `strata edit` at a terminal, with nothing piped in: edit the code in an editor
    if cmd == "edit" && (args.flag("--editor") || std::io::IsTerminal::is_terminal(&std::io::stdin())) {
        return edit_in_editor(&mut cb, &args, json_out);
    }
    let edits = edits_in(args, cmd, &cb.program, "")?;
    let deletes: Vec<String> = args.flags.get("--rm").cloned().unwrap_or_default();
    let msg = args.val("-m").unwrap_or(if cmd == "add" { "import" } else { "edit" });
    Ok(apply_edit(&mut cb, cmd, edits, deletes, msg, json_out)?.is_ok())
}

/// `strata --file F edit | add`: change a file outside a package's log (a
/// script, or a folder of module files), created if it doesn't exist. Only
/// what doesn't parse is refused: the result is stored as it is, and checked
/// like any program afterwards.
fn edit_file(args: &Args, cmd: &str, f: &str) -> Result<bool, String> {
    let path = std::path::Path::new(f);
    let dir = path.is_dir() || f.ends_with('/');
    if !dir {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
    }
    // a module file in a package is the module its path names
    let in_package = if dir { None } else { store::module_at(path)? };
    let old = if path.exists() { read_sources(f)? } else { Program::default() };
    // a script keeps its module; a folder's definitions go into its root module by default
    let module = match &in_package {
        Some((_, m)) => m.clone(),
        None if dir => String::new(),
        None => old.decls.first().map(|d| d.module.to_string()).unwrap_or_default(),
    };
    let edits = edits_in(args, cmd, &old, &module)?;
    let deletes: Vec<String> = args.flags.get("--rm").cloned().unwrap_or_default();
    for q in &deletes {
        if !old.decls.iter().any(|d| d.qname() == *q) {
            return Err(format!("no definition `{q}` to remove"));
        }
    }
    let new = store::merge(&old, edits, &deletes);
    if dir {
        store::write_modules(path, &old, &new)?;
    } else if in_package.is_some() {
        store::write_file(path, &store::encode_at(path, new.decls.clone())?)?;
    } else {
        store::write_file(path, &store::encode_script(&new))?;
    }
    let changes = store::changes(&old, &new);
    println!("{f}: {} definition{} changed, {} in all", changes.len(), if changes.len() == 1 { "" } else { "s" }, new.decls.len());
    Ok(true)
}

/// Each definition's name as stored → as written (`impl std.cli.CommandLine
/// for Options` is `impl CommandLine for Options`), for messages.
fn shown_names(p: &Program) -> HashMap<String, String> {
    p.decls.iter().zip(resolve::written_form(p).decls).map(|(s, w)| (s.qname(), w.qname())).collect()
}

/// `strata rename`.
pub fn rename(args: &Args, json_out: bool) -> Result<bool, String> {
    let Source::Db(mut cb) = load(&args)? else { return Err("`rename` needs a codebase (it commits the change)".into()) };
    let words = &args.pos[1..];
    if words.is_empty() || words.len() % 2 != 0 {
        return Err("usage: strata rename <old> <new> [<old> <new>]...  (several renames are one step)".into());
    }
    // each `old` is a qualified name, or one that only one module defines: a
    // definition, a variant, a trait method, or a field as `Owner.field`
    let mut mods: Vec<String> = cb.program.decls.iter().map(|d| d.module.to_string()).collect();
    mods.sort();
    mods.dedup();
    let mut pairs: Vec<(String, String, &str)> = Vec::new();
    for w in words.chunks(2) {
        let (old, new) = (&w[0], &w[1]);
        let (old_q, kind) = match resolve::member(&cb.program, old) {
            Some((kind, _)) => (old.clone(), kind),
            None => {
                let found: Vec<(String, &str)> = mods
                    .iter()
                    .map(|m| if m.is_empty() { old.clone() } else { format!("{m}.{old}") })
                    .filter_map(|q| resolve::member(&cb.program, &q).map(|(k, _)| (q, k)))
                    .collect();
                match found.as_slice() {
                    [one] => one.clone(),
                    [] => return Err(format!("no definition, variant, trait method or field `{old}`")),
                    many => return Err(format!("`{old}` is ambiguous: {}", many.iter().map(|(q, _)| q.as_str()).collect::<Vec<_>>().join(", "))),
                }
            }
        };
        pairs.push((old_q, new.clone(), kind));
    }
    let renamed = resolve::rename_all(&cb.program, &pairs.iter().map(|(o, n, _)| (o.clone(), n.clone())).collect::<Vec<_>>())?;
    let rep = full_check(&renamed);
    if !rep.ok() {
        print_errors(&rep);
        eprintln!("\nrejected: the codebase would not check; nothing was changed");
        return Ok(false);
    }
    let new_q = |old_q: &str, new: &str| match old_q.rsplit_once('.') {
        Some((m, _)) => format!("{m}.{new}"),
        None => new.to_string(),
    };
    let news: Vec<String> = pairs.iter().map(|(o, n, _)| new_q(o, n)).collect();
    // what declares them (themselves, or the types / traits members belong to) isn't a reference
    let owners: Vec<String> = pairs.iter().filter_map(|(o, _, _)| resolve::owner(&cb.program, o)).collect();
    let before: HashMap<String, String> = cb.program.decls.iter().map(|d| (d.qname(), store::self_hash(d))).collect();
    let shown = shown_names(&renamed);
    let updated: Vec<String> = renamed
        .decls
        .iter()
        .filter(|d| !matches!(d.kind, DeclKind::Use(_)) && !news.contains(&d.qname()) && !owners.contains(&d.qname()) && before.get(&d.qname()).is_some_and(|h| *h != store::self_hash(d)))
        .map(|d| shown[&d.qname()].clone())
        .collect();
    // prose is not rewritten: point at docs and comments that still say an old name
    let shorts: Vec<String> = pairs.iter().map(|(o, _, _)| crate::names::short(&o).to_string()).collect();
    let word = |t: &str| t.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| shorts.iter().any(|s| s == w));
    let mentions: Vec<String> = renamed
        .decls
        .iter()
        .filter(|d| d.doc.iter().chain(&d.comments).any(|l| word(l)))
        .map(|d| shown[&d.qname()].clone())
        .collect();
    let arrows: Vec<String> = pairs.iter().zip(&news).map(|((o, _, _), n)| format!("{o} → {n}")).collect();
    let default_msg = format!("rename {}", arrows.join(", "));
    let msg = args.val("-m").unwrap_or(&default_msg);
    let entries: Vec<serde_json::Value> = pairs.iter().zip(&news).map(|((o, _, k), n)| json!({"from": o, "to": n, "kind": k})).collect();
    record(&mut cb, renamed, msg, json!({"renames": entries, "references": updated}))?;
    if json_out {
        println!("{}", json!({"renames": entries, "references": updated, "mentions": mentions}));
        return Ok(true);
    }
    println!("renamed {}", arrows.join(", "));
    match updated.len() {
        0 => println!("no other definition refers to {}", if pairs.len() == 1 { "it" } else { "them" }),
        n => println!("updated {n} definition{} that refer{} to {}: {}", if n > 1 { "s" } else { "" }, if n > 1 { "" } else { "s" }, if pairs.len() == 1 { "it" } else { "them" }, updated.join(", ")),
    }
    if !mentions.is_empty() {
        println!("docs or comments still mention {} in: {}", shorts.iter().map(|s| format!("`{s}`")).collect::<Vec<_>>().join(" or "), mentions.join(", "));
    }
    Ok(true)
}

/// `strata move`.
pub fn move_def(args: &Args, json_out: bool) -> Result<bool, String> {
    let Source::Db(mut cb) = load(&args)? else { return Err("`move` needs a package (it commits the change)".into()) };
    let (Some(old), Some(target)) = (args.pos.get(1), args.pos.get(2)) else { return Err("usage: strata move <name> <module>  (`main` is the root module)".into()) };
    let target = if target == "main" { String::new() } else { target.clone() };
    // `old` is a qualified name, or a short name that only one module defines
    let old_q = if cb.program.decls.iter().any(|d| &d.qname() == old) {
        old.clone()
    } else {
        let found: Vec<String> = cb.program.decls.iter().filter(|d| &d.name() == old).map(|d| d.qname()).collect();
        match found.as_slice() {
            [one] => one.clone(),
            [] => return Err(format!("no definition `{old}`")),
            many => return Err(format!("`{old}` is ambiguous: {}", many.join(", "))),
        }
    };
    let moved = resolve::move_def(&cb.program, &old_q, &target)?;
    let rep = full_check(&moved);
    if !rep.ok() {
        print_errors(&rep);
        eprintln!("\nrejected: the codebase would not check; nothing was changed");
        return Ok(false);
    }
    let short = crate::names::short(&old_q).to_string();
    let new_q = if target.is_empty() { short.clone() } else { format!("{target}.{short}") };
    // what refers to it: definitions whose stored form changed (their text may
    // not: `charge(o)` where `charge` is imported stays `charge(o)`)
    let stored = |p: &Program| -> HashMap<String, String> { p.decls.iter().zip(resolve::stored_form(p).decls).map(|(d, s)| (d.qname(), store::self_hash(&s))).collect() };
    let (before, after) = (stored(&cb.program), stored(&moved));
    let is_use = |d: &Decl| matches!(d.kind, DeclKind::Use(_));
    let shown_q = shown_names(&moved);
    let updated: Vec<String> = moved.decls.iter().filter(|d| !is_use(d) && d.qname() != new_q && before.get(&d.qname()).is_some_and(|h| Some(h) != after.get(&d.qname()))).map(|d| shown_q[&d.qname()].clone()).collect();
    let shown = |m: &str| if m.is_empty() { "main".to_string() } else { m.to_string() };
    // imports the move added or changed, by module
    let old_uses: crate::fx::HashSet<(String, String)> = cb.program.decls.iter().filter(|d| is_use(d)).map(|d| (d.module.to_string(), printer::decl(d))).collect();
    let imports: Vec<String> = moved.decls.iter().filter(|d| is_use(d) && !old_uses.contains(&(d.module.to_string(), printer::decl(d)))).map(|d| format!("{}: {}", shown(&d.module), printer::decl(d))).collect();
    let from_m = old_q.rsplit_once('.').map(|(m, _)| m.to_string()).unwrap_or_default();
    let default_msg = format!("move {old_q} → {}", shown(&target));
    let msg = args.val("-m").unwrap_or(&default_msg);
    record(&mut cb, moved, msg, json!({"renames": [{"from": old_q, "to": new_q, "kind": "move"}], "references": updated}))?;
    if json_out {
        println!("{}", json!({"from": old_q, "to": new_q, "references": updated}));
        return Ok(true);
    }
    println!("moved {old_q} from {} to {} ({})", shown(&from_m), shown(&target), store::module_path(&target).display());
    match updated.len() {
        0 => println!("no other definition refers to it"),
        n => println!("updated {n} definition{} that refer{} to it: {}", if n > 1 { "s" } else { "" }, if n > 1 { "" } else { "s" }, updated.join(", ")),
    }
    if !imports.is_empty() {
        println!("imports: {}", imports.join("; "));
    }
    Ok(true)
}

/// `strata status`.
pub fn status(args: &Args) -> Result<bool, String> {
    let Source::Db(cb) = load(&args)? else { return Err("`status` needs a package".into()) };
    let Some(p) = pending::load(&cb) else {
        println!("no pending change");
        return Ok(true);
    };
    let open: crate::fx::HashSet<String> = types::check(&cb.program).holes.into_iter().map(|h| h.name).collect();
    let (left, filled): (Vec<&pending::Hole>, Vec<&pending::Hole>) = p.holes.iter().partition(|h| open.contains(&h.name));
    println!("pending change: {}  ({} hole{} left, {} filled)", p.message, left.len(), if left.len() == 1 { "" } else { "s" }, filled.len());
    for h in &left {
        println!("  ?{}  in {}\n      was: {}", h.name, h.decl, h.was);
    }
    println!("{}", if left.is_empty() { "`strata done` records it as one change; `strata abort` undoes it" } else { "fill the holes with `strata edit`; `strata done --keep-holes` records it as is; `strata abort` undoes it" });
    Ok(true)
}

/// `strata done`.
pub fn done(args: &Args) -> Result<bool, String> {
    let Source::Db(mut cb) = load(&args)? else { return Err("`done` needs a package".into()) };
    let p = pending::load(&cb).ok_or("no pending change")?;
    let open: crate::fx::HashSet<String> = types::check(&cb.program).holes.into_iter().map(|h| h.name).collect();
    let left: Vec<&pending::Hole> = p.holes.iter().filter(|h| open.contains(&h.name)).collect();
    if !left.is_empty() && !args.flag("--keep-holes") {
        eprintln!("{} hole{} of this change {} still open:", left.len(), if left.len() == 1 { "" } else { "s" }, if left.len() == 1 { "is" } else { "are" });
        for h in &left {
            eprintln!("  ?{}  in {}  (was: {})", h.name, h.decl, h.was);
        }
        eprintln!("fill them with `strata edit`, keep them with `strata done --keep-holes`, or undo the change with `strata abort`");
        return Ok(false);
    }
    if !left.is_empty() {
        let kept = pending::keep_holes(&cb.program);
        cb.write(kept)?;
    }
    let before = pending::before_program(&p)?;
    let msg = args.val("-m").unwrap_or(&p.message).to_string();
    let extra = if p.renames.is_empty() { json!({}) } else { json!({"renames": p.renames}) };
    let changes = cb.log_entry(&before, &msg, extra)?;
    pending::clear(&cb);
    git_commit(&cb, &msg);
    println!("recorded \"{msg}\" as one change: {} definition{}", changes.len(), if changes.len() == 1 { "" } else { "s" });
    if !left.is_empty() {
        println!("{} hole{} kept as ?todo_…, see `strata holes`", left.len(), if left.len() == 1 { "" } else { "s" });
    }
    Ok(true)
}

/// `strata abort`.
pub fn abort(args: &Args) -> Result<bool, String> {
    let Source::Db(cb) = load(&args)? else { return Err("`abort` needs a package".into()) };
    let p = pending::load(&cb).ok_or("no pending change")?;
    pending::restore(&cb, &p)?;
    pending::clear(&cb);
    println!("undid the pending change \"{}\": every file is as it was before it", p.message);
    Ok(true)
}

/// Apply edited definitions (and deletions) to the package: committed if the
/// whole package checks; if they check but break code elsewhere, the broken
/// uses become holes in a pending change. `Err` holds the errors of a rejected edit.
pub fn apply_edit(cb: &mut Codebase, cmd: &str, edits: Program, deletes: Vec<String>, msg: &str, json_out: bool) -> Result<Result<(), Vec<String>>, String> {
    let mut edited: Vec<String> = edits.decls.iter().map(|d| d.qname()).collect();
    edited.extend(deletes.iter().cloned());
    let new = store::merge(&cb.program, edits, &deletes);
    let rep = full_check(&new);
    if !rep.ok() {
        // an edit that breaks code elsewhere: the broken uses become holes, in a pending change
        let open = pending::load(cb);
        let first = open.as_ref().map_or(1, |p| p.holes.len() + 1);
        match (cmd, pending::holeify(&cb.program, &new, &edited, &rep.errors, first)) {
            ("edit", Ok((with_holes, holes, notes))) => {
                let mut p = match open {
                    Some(p) => p,
                    None => pending::start(cb, msg)?,
                };
                cb.write(with_holes)?;
                p.holes.extend(holes.iter().cloned());
                pending::save(cb, &p)?;
                println!("the edit breaks {} use{} elsewhere; {}, in a pending change:", holes.len(), if holes.len() == 1 { "" } else { "s" }, if holes.len() == 1 { "it is now a hole" } else { "they are now holes" });
                for h in &holes {
                    if h.was.is_empty() {
                        println!("  ?{}  in {}\n      why: {}", h.name, h.decl, h.why.join("; "));
                    } else {
                        println!("  ?{}  in {}\n      was: {}\n      why: {}", h.name, h.decl, h.was, h.why.join("; "));
                    }
                }
                for n in &notes {
                    println!("  {n}");
                }
                println!("fill them with `strata edit`, then `strata done` (or `strata abort` to undo the whole change)");
                return Ok(Ok(()));
            }
            _ => {
                print_errors(&rep);
                eprintln!("\nrejected: the codebase would not check; nothing was changed");
                return Ok(Err(rep.errors.iter().map(|e| format!("error in {}: {}", e.decl, e.msg)).collect()));
            }
        }
    }
    let changes = record(cb, new, msg, json!({}))?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&changes).unwrap());
        return Ok(Ok(()));
    }
    if changes.is_empty() {
        println!("no changes");
    }
    for c in &changes {
        let what = match (&c.old, &c.new) {
            (None, Some(h)) => format!("added    #{h}"),
            (Some(h), None) => format!("removed  #{h}"),
            (Some(a), Some(b)) if c.interface_changed => format!("changed  #{a} → #{b}  (interface changed)"),
            (Some(a), Some(b)) => format!("changed  #{a} → #{b}  (implementation only — dependents unaffected)"),
            _ => unreachable!(),
        };
        println!("{:<6} {:<24} {what}", c.kind, c.name);
        if c.affected.len() > 6 {
            // summarise by module
            let mut per: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
            for a in &c.affected {
                let m = cb.program.decls.iter().find(|d| &d.qname() == a).map(|d| d.module.clone()).unwrap_or_default();
                *per.entry(if m.is_empty() { "(root)".into() } else { m.to_string() }).or_default() += 1;
            }
            let parts: Vec<String> = per.iter().map(|(m, n)| format!("{m} ({n})")).collect();
            println!("         re-check {} dependents in: {}", c.affected.len(), parts.join(", "));
        } else if !c.affected.is_empty() {
            println!("         re-check dependents: {}", c.affected.join(", "));
        }
    }
    // a module's contract is its public definitions
    let mut contracts: Vec<String> = Vec::new();
    for c in changes.iter().filter(|c| c.interface_changed && c.old.is_some()) {
        if let Some(d) = cb.program.decls.iter().find(|d| d.qname() == c.name) {
            if d.public && !d.module.is_empty() && !contracts.contains(&d.module.to_string()) {
                contracts.push(d.module.to_string());
            }
        }
    }
    for m in contracts {
        println!("module {m}: its public contract changed (see `strata interface {m}`)");
    }
    if !rep.holes.is_empty() {
        println!("{} open hole(s) — see `strata holes`", rep.holes.len());
    }
    Ok(Ok(()))
}

/// Commit a change, or, during a pending change, only write it: the change
/// is logged as one entry when it is done.
pub fn record(cb: &mut Codebase, new: Program, msg: &str, extra: serde_json::Value) -> Result<Vec<store::Change>, String> {
    match pending::load(cb) {
        None => {
            let changes = cb.commit_with(new, msg, extra)?;
            if !changes.is_empty() {
                git_commit(cb, msg);
            }
            Ok(changes)
        }
        Some(mut p) => {
            let changes = store::changes(&cb.program, &new);
            cb.write(new)?;
            p.renames.extend(extra["renames"].as_array().cloned().unwrap_or_default());
            // the holes' definitions keep their names
            for (from, to) in store::renames_of(&extra) {
                for h in p.holes.iter_mut().filter(|h| h.decl == from) {
                    h.decl = to.clone();
                }
            }
            pending::save(cb, &p)?;
            Ok(changes)
        }
    }
}

/// Lines the editor buffer starts with when an edit didn't go through; they
/// are dropped when the buffer is read back.
pub const EDIT_MARK: &str = "// ✗ ";

/// The user's editor: `$VISUAL`, `$EDITOR`, git's, or `vi`.
pub fn editor_command() -> String {
    for var in ["VISUAL", "EDITOR"] {
        if let Ok(e) = std::env::var(var) {
            if !e.trim().is_empty() {
                return e;
            }
        }
    }
    crate::git::run(None, &["var", "GIT_EDITOR"]).map(|e| e.trim().to_string()).ok().filter(|e| !e.is_empty()).unwrap_or_else(|| "vi".into())
}

/// What `strata edit X` names: a module (`shop`, `main` for the root, or its
/// file `shop/orders.strata`, which may not exist yet), else the definitions
/// a selector picks. `None` when `X` is a selector.
pub fn edit_module(p: &Program, arg: &str) -> Option<String> {
    if let Some(path) = arg.strip_suffix(".strata") {
        let m = path.trim_start_matches("./").replace(['/', '\\'], ".");
        return Some(if m == "main" { String::new() } else { m });
    }
    if arg == "main" {
        return Some(String::new());
    }
    p.decls.iter().any(|d| d.module == arg).then(|| arg.to_string())
}

/// One file of an editing session: a module, the definitions shown from it
/// (those missing when it is saved are removed), and its text.
pub struct EditFile {
    module: String,
    shown: Vec<String>,
    text: String,
    path: std::path::PathBuf,
}

/// `strata edit [MODULE | SELECTOR]` with nothing piped in: open the code in
/// the user's editor, and apply what is saved as one edit. With nothing
/// named, every module of the package, one file each; a module (`shop`, or
/// its file `shop.strata`) as its file; otherwise the definitions a selector
/// picks. Definitions deleted from a file are removed. If the edit doesn't
/// parse or check, the editor opens again with the errors at the top; saving
/// it unchanged gives up.
pub fn edit_in_editor(cb: &mut Codebase, args: &Args, json_out: bool) -> Result<bool, String> {
    // the code as written; a definition deleted from it is removed by its stored name
    let p = resolve::written_form(&cb.program);
    let stored_name: HashMap<String, String> = p.decls.iter().zip(&cb.program.decls).map(|(w, s)| (w.qname(), s.qname())).collect();
    let mut mods: Vec<String> = p.decls.iter().map(|d| d.module.to_string()).collect();
    mods.sort();
    mods.dedup();
    let whole = |m: &str| -> (Vec<String>, String) {
        let shown = p.decls.iter().filter(|d| d.module == m).map(|d| d.qname()).collect();
        let text = if p.decls.iter().any(|d| d.module == m) { printer::module_file(&p, m) } else { String::new() };
        (shown, text)
    };
    let dir = cb.cache_dir().join("edit");
    let _ = std::fs::remove_dir_all(&dir);
    let file = |module: String, shown: Vec<String>, text: String| EditFile { path: dir.join(store::module_path(&module)).with_extension(store::TEXT_EXT), module, shown, text };
    let mut files: Vec<EditFile> = match args.pos.get(1) {
        None if mods.is_empty() => vec![file(String::new(), vec![], String::new())],
        None => mods.iter().map(|m| {
            let (shown, text) = whole(m);
            file(m.clone(), shown, text)
        }).collect(),
        Some(arg) => match edit_module(&p, arg) {
            Some(m) => {
                let (shown, text) = whole(&m);
                vec![file(m, shown, text)]
            }
            None => {
                let names = view::select(&p, Some(arg));
                let picked: Vec<&Decl> = p.decls.iter().filter(|d| names.contains(&d.qname())).collect();
                if picked.is_empty() {
                    return Err(format!("`{arg}` is no module and picks no definition (a new module is opened by its file: `strata edit {arg}.strata`)"));
                }
                let mut ms: Vec<&str> = picked.iter().map(|d| d.module.as_str()).collect();
                ms.sort();
                ms.dedup();
                ms.into_iter()
                    .map(|m| {
                        let here: Vec<&&Decl> = picked.iter().filter(|d| d.module == m).collect();
                        let text = here.iter().map(|d| printer::decl(d)).collect::<Vec<_>>().join("\n\n") + "\n";
                        file(m.to_string(), here.iter().map(|d| d.qname()).collect(), text)
                    })
                    .collect()
            }
        },
    };
    let editor = editor_command();
    let msg = args.val("-m").unwrap_or("edit").to_string();
    let first = files.iter().map(|f| f.text.clone()).collect::<Vec<_>>();
    let mut last = first.clone();
    let mut errors_at: Vec<Vec<String>> = vec![vec![]; files.len()];
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&dir);
    };
    loop {
        for (f, errors) in files.iter().zip(&errors_at) {
            let mut buffer = f.text.clone();
            if !errors.is_empty() {
                let mut head: Vec<String> = errors.iter().flat_map(|e| e.lines().map(|l| format!("{EDIT_MARK}{l}")).collect::<Vec<_>>()).collect();
                head.push(format!("{EDIT_MARK}not applied: fix it and save, or save it unchanged to give up"));
                buffer = head.join("\n") + "\n\n" + &f.text;
            }
            if let Some(parent) = f.path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&f.path, &buffer).map_err(|e| format!("{}: {e}", f.path.display()))?;
        }
        let status = Command::new("sh")
            .arg("-c")
            .arg(format!("{editor} \"$@\""))
            .arg("strata-edit")
            .args(files.iter().map(|f| &f.path))
            .status()
            .map_err(|e| format!("could not start the editor `{editor}`: {e}"))?;
        if !status.success() {
            cleanup();
            return Err(format!("the editor `{editor}` failed; nothing was changed"));
        }
        for f in &mut files {
            let raw = std::fs::read_to_string(&f.path).map_err(|e| e.to_string())?;
            f.text = raw.lines().filter(|l| !l.starts_with(EDIT_MARK)).collect::<Vec<_>>().join("\n").trim_start_matches('\n').to_string() + "\n";
        }
        let now: Vec<String> = files.iter().map(|f| f.text.clone()).collect();
        if now.iter().zip(&last).all(|(a, b)| a.trim() == b.trim()) {
            cleanup();
            let untouched = last.iter().zip(&first).all(|(a, b)| a.trim() == b.trim());
            println!("{}", if untouched { "no changes" } else { "gave up: nothing was changed" });
            return Ok(untouched);
        }
        last = now;
        errors_at = vec![vec![]; files.len()];
        let mut edits = Program::default();
        for (i, f) in files.iter().enumerate() {
            match parser::parse_module_file(&f.text, &f.module) {
                Ok(p) => edits.decls.extend(p.decls),
                Err(e) => errors_at[i].push(format!("parse error at {e}")),
            }
        }
        if errors_at.iter().all(Vec::is_empty) {
            let kept: crate::fx::HashSet<String> = edits.decls.iter().map(|d| d.qname()).collect();
            let deletes: Vec<String> = files.iter().flat_map(|f| f.shown.iter()).filter(|q| !kept.contains(*q)).map(|q| stored_name.get(q).cloned().unwrap_or_else(|| q.clone())).collect();
            match apply_edit(cb, "edit", edits, deletes, &msg, json_out)? {
                Ok(()) => {
                    cleanup();
                    return Ok(true);
                }
                Err(errors) => {
                    // each error goes on top of the file of its definition (`error in shop.total: ...`)
                    for e in errors {
                        let decl = e.strip_prefix("error in ").and_then(|r| r.split_once(": ")).map_or("", |(d, _)| d);
                        let i = files.iter().enumerate().filter(|(_, f)| f.module.is_empty() || decl.starts_with(&format!("{}.", f.module))).max_by_key(|(_, f)| f.module.len()).map_or(0, |(i, _)| i);
                        errors_at[i].push(e);
                    }
                }
            }
        }
    }
}

/// After a change is logged: commit the package's files to git, with the
/// change's message, unless the package says `auto_commit = false`, the
/// command says `--no-commit`, or this isn't a git repository. Only the
/// package's files are committed; anything else staged is left alone. A
/// failed commit doesn't undo the change: it says why.
pub fn git_commit(cb: &Codebase, msg: &str) {
    if std::env::args().any(|a| a == "--no-commit") {
        return;
    }
    if package::load(cb.root()).map(|p| !p.auto_commit).unwrap_or(false) {
        return;
    }
    let dir = Some(cb.root());
    if !crate::git::ok(dir, &["rev-parse", "--show-toplevel"]) {
        return; // not in a git repository
    }
    if crate::git::ok(dir, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]) {
        println!("not committed to git: a merge is in progress (commit it, then strata commits again)");
        return;
    }
    let _ = crate::git::run(dir, &["add", "-A", "--", "."]);
    let staged = !crate::git::ok(dir, &["diff", "--cached", "--quiet", "--", "."]);
    if !staged {
        return;
    }
    match crate::git::run(dir, &["commit", "-q", "-m", msg, "--", "."]) {
        Ok(_) => {
            let sha = crate::git::run(dir, &["rev-parse", "--short", "HEAD"]).map(|o| o.trim().to_string()).unwrap_or_default();
            println!("committed to git: {sha} {}", msg.lines().next().unwrap_or(""));
        }
        Err(e) => eprintln!("the change is made, but {e}"),
    }
}
