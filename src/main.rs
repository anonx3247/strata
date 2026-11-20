mod ast;
mod cli;
mod diff;
mod history;
mod generate;
mod git;
mod interp;
mod lexer;
mod merge;
mod names;
mod package;
mod pending;
mod parser;
mod printer;
mod resolve;
mod smt;
mod stdlib;
mod termination;
mod store;
mod types;
mod ui;
mod verify;
mod view;

use ast::*;
use serde_json::json;
use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, ExitCode};
use store::Codebase;

const USAGE: &str = "strata — an experimental language for working with agents

USAGE: strata [--file F.strata] <command> [args]

Codebase (the package around the current folder: its files, and strata.log, the log of
every change made through strata, committed with them):
  init                         make the current folder a package (a strata.toml named
                               after the folder)
  add <file.strata> [-m MSG]   import definitions from a file (checked, atomic)
  edit [MODULE] [-m MSG] [--rm NAME]..
                               read definitions from stdin; replace/add them by name (into
                               MODULE, default the root); commit only if the whole codebase
                               still checks
  edit [MODULE | SELECTOR]     with nothing piped in (or --editor): open the code in $VISUAL /
                               $EDITOR and apply what you save: the whole package (one file
                               per module), a module (`shop`, `main`, or its file
                               `shop/orders.strata`, new or not), or the definitions a
                               selector picks
  rename <old> <new> [<old> <new>]... [-m MSG]
                               rename functions, types, traits, variants, trait methods or
                               fields (`Owner.field`) and every reference, in one step
                               (`old` may be qualified, `new` is the short name)
  move <name> <module>         move a function, type or trait to another module (its file),
                               updating references and imports (`main` is the root module)
  status                       the pending change, if any: its holes, filled or not
  done [--keep-holes] [-m MSG] record the pending change as one log entry
  (in a git repository each recorded change is also a git commit, unless the package says
   `auto_commit = false` or the command `--no-commit`)
  abort                        undo the pending change: every file as it was before it
  (an `edit` that breaks code elsewhere turns each broken use into a hole, ?pending_N, and
   opens a pending change; further edits, renames and moves join it)
  log                          show the patch history
  diff [--from N] [--to N] [--step K] [--json]
                               what changed, structurally: renames, new variants, stronger
                               contracts, bodies with changes marked in place. In a codebase
                               the range is patches (default: all after the first import);
                               with --file, git: `diff [BASE]` compares BASE (default HEAD)
                               with the working tree, one step per commit
  interface <module>           a module's public contract: what other modules can use
  render [MODULE] [--split DIR]
                               print a module as source (L3): the file that holds it (the
                               root module is `main`); --split writes one file per module
  (`--file` and `add` accept a folder: each .strata file is a module named after its path;
   `--module m` reads a single file as module m)

Packages: a folder with a strata.toml (`name = \"...\"`, optional `exports = [...]`);
its .strata files are modules named after their path, main.strata is the entry point.
A .strata file outside every package is a script.

Git:
  merge-file O A B [P]         git merge driver: merge each definition separately, then
                               type-check; writes the result to A, fails on a conflict
  setup-git                    use merge-file for `git merge` / `git pull` in this clone
  sync [BRANCH]                merge BRANCH (default main; origin's if there is a remote)
                               into this branch with the driver, then check; commits the
                               merge only if `ci` passes
  ci [--fix]                   what CI requires: every package and script in the repository
                               checks, exports exist, and every .strata file is in canonical
                               format (--fix rewrites them)

Views:
  ui [--port 7777]                live viewer in the browser (map, levels, holes, activity)
  view [SELECTOR] [--level 0-3]   SELECTOR: name | prefix* | slice:Name | a,b
  ast <name>                      the stored syntax tree as JSON

Checking:
  check [--json]               types, exhaustiveness, literal refinements
  holes [--json]               typed holes with goal type, context and obligations
  verify [SELECTOR] [--cases N] [--checks] [--smt] [--require R,..] [--json]
                               prove every contract with an SMT solver (z3), confirm
                               its counterexamples by running them, fuzz the rest;
                               results cached by content hash. --checks lists the
                               proven checks, --smt prints the solver queries.
                               Each function also says whether it terminates.
                               --require (proven, terminates, no-recursion) fails
                               the run when a checked function falls short (as do
                               strata.toml's [verify] rules). Proven functions that
                               terminate are still run on --cross-check N cases
                               (default 20, 0: off), to catch a wrong proof

Running:
  run [PATH] [-- ARGS..]       run the program: `main` gets the capabilities it asks for
                               (Console, Files, Environment from std) and ARGS; it
                               returns (), an ExitCode, or a Result (Err: exit code 1).
                               PATH is a .strata file or a package folder (or --file)
  run <fn> [JSON-ARGS] [--trace]
                               call one function: args are a JSON array validated
                               against the types; --trace prints each call and return
                               to stderr
  test [FILTER]                run test blocks
  builtins [--json]            list built-in functions and their signatures

With --file, commands operate on a source file instead of a codebase.";

struct Args {
    pos: Vec<String>,
    flags: HashMap<String, Vec<String>>,
}

impl Args {
    fn parse() -> Args {
        let raw: Vec<String> = std::env::args().skip(1).collect();
        let valued = ["-m", "--level", "--cases", "--file", "--rm", "--seed", "--port", "--module", "--split", "--from", "--to", "--step", "--base", "--require", "--cross-check"];
        let (mut pos, mut flags) = (Vec::new(), HashMap::<String, Vec<String>>::new());
        let mut i = 0;
        while i < raw.len() {
            let a = &raw[i];
            if a == "--" {
                // the rest goes to the program (`strata run -- a b`)
                flags.insert("--".into(), raw[i + 1..].to_vec());
                break;
            }
            if valued.contains(&a.as_str()) && i + 1 < raw.len() {
                flags.entry(a.clone()).or_default().push(raw[i + 1].clone());
                i += 2;
            } else if a.starts_with("--") {
                flags.entry(a.clone()).or_default();
                i += 1;
            } else {
                pos.push(a.clone());
                i += 1;
            }
        }
        Args { pos, flags }
    }
    fn flag(&self, f: &str) -> bool {
        self.flags.contains_key(f)
    }
    fn val(&self, f: &str) -> Option<&str> {
        self.flags.get(f).and_then(|v| v.last()).map(|s| s.as_str())
    }
}

enum Source {
    File(Program),
    Db(Codebase),
}

impl Source {
    fn program(&self) -> &Program {
        match self {
            Source::File(p) => p,
            Source::Db(c) => &c.program,
        }
    }
}

/// Read source: a single file, or a folder where each `.strata` file is a
/// module named after its path (`shop/orders.strata` -> `shop.orders`,
/// `main.strata` -> the root module).
fn read_sources(path: &str, module: Option<&str>) -> Result<Program, String> {
    let root = std::path::Path::new(path);
    if !root.is_dir() {
        let src = std::fs::read_to_string(root).map_err(|e| format!("{path}: {e}"))?;
        return parser::parse_module_file(&src, module.unwrap_or("")).map_err(|e| format!("{path}:{e}"));
    }
    // packages nested in this folder are separate programs
    package::program(root)
}

/// Parse a folder's `.strata` files, given as (path relative to the folder,
/// source): each file is a module named after its path (`main` is the root).
fn parse_files(mut files: Vec<(std::path::PathBuf, String)>) -> Result<Program, String> {
    files.sort();
    let mut decls = Vec::new();
    for (rel, src) in files {
        let mut m = rel.with_extension("").to_string_lossy().replace(std::path::MAIN_SEPARATOR, ".");
        if m == "main" {
            m.clear();
        }
        let p = parser::parse_module_file(&src, &m).map_err(|e| format!("{}:{e}", rel.display()))?;
        decls.extend(p.decls);
    }
    Ok(Program { decls })
}

fn load(args: &Args) -> Result<Source, String> {
    if let Some(f) = args.val("--file") {
        return Ok(Source::File(read_sources(f, args.val("--module"))?));
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = Codebase::find(&cwd).ok_or("no package here (no strata.toml in this folder or above: run `strata init`, or pass --file)")?;
    Ok(Source::Db(Codebase::open(&root)?))
}

/// Full static check: types, exhaustiveness, then literal refinements.
fn full_check(p: &Program) -> types::CheckReport {
    let mut rep = types::check(p);
    let (env, _) = types::Env::new(p);
    for d in verify::static_refinements(p, &env) {
        if !rep.errors.iter().any(|e| e.decl == d.decl && e.msg == d.msg) {
            rep.errors.push(d);
        }
    }
    // a `CommandLine` record must be readable from arguments
    for i in env.impls.iter().filter(|i| i.trait_name == cli::TRAIT) {
        if let Err(msg) = cli::validate(&env, &types::texpr_head(&i.for_type)) {
            rep.errors.push(types::Diag { decl: i.name(), msg });
        }
    }
    rep
}

fn print_errors(rep: &types::CheckReport) {
    for e in &rep.errors {
        eprintln!("error in {}: {}", e.decl, e.msg);
    }
}

fn verdicts_for(src: &Source) -> HashMap<String, String> {
    let Source::Db(cb) = src else { return HashMap::new() };
    let cache = cb.load_verdicts();
    cb.program
        .decls
        .iter()
        .filter_map(|d| cache.get(&store::closure_hash(&cb.program, &d.qname())).map(|v| (d.qname(), v.tag())))
        .collect()
}

fn run() -> Result<bool, String> {
    let args = Args::parse();
    let Some(cmd) = args.pos.first().map(|s| s.as_str()) else {
        println!("{USAGE}");
        return Ok(true);
    };
    let json_out = args.flag("--json");
    match cmd {
        "help" | "--help" => println!("{USAGE}"),
        "init" => {
            let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
            let toml = Codebase::init(&cwd)?;
            println!("wrote {}: {}", package::FILE, toml.trim());
        }
        "add" | "edit" => {
            let Source::Db(mut cb) = load(&args)? else { return Err("`add`/`edit` need a codebase".into()) };
            // `strata edit` at a terminal, with nothing piped in: edit the code in an editor
            if cmd == "edit" && (args.flag("--editor") || std::io::IsTerminal::is_terminal(&std::io::stdin())) {
                return edit_in_editor(&mut cb, &args, json_out);
            }
            let edits = if cmd == "add" {
                let f = args.pos.get(1).ok_or("usage: strata add <file.strata | folder>")?;
                read_sources(f, args.val("--module"))?
            } else {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
                // `strata edit shop < defs`: unqualified definitions go into the module `shop`
                let module = match args.pos.get(1) {
                    None => String::new(),
                    Some(arg) => edit_module(&cb.program, arg).unwrap_or_else(|| arg.clone()),
                };
                parser::parse_module_file(&s, &module).map_err(|e| format!("parse error at {e}"))?
            };
            let deletes: Vec<String> = args.flags.get("--rm").cloned().unwrap_or_default();
            let msg = args.val("-m").unwrap_or(if cmd == "add" { "import" } else { "edit" });
            return Ok(apply_edit(&mut cb, cmd, edits, deletes, msg, json_out)?.is_ok());
        }
        "rename" => {
            let Source::Db(mut cb) = load(&args)? else { return Err("`rename` needs a codebase (it commits the change)".into()) };
            let words = &args.pos[1..];
            if words.is_empty() || words.len() % 2 != 0 {
                return Err("usage: strata rename <old> <new> [<old> <new>]...  (several renames are one step)".into());
            }
            // each `old` is a qualified name, or one that only one module defines: a
            // definition, a variant, a trait method, or a field as `Owner.field`
            let mut mods: Vec<String> = cb.program.decls.iter().map(|d| d.module.clone()).collect();
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
            let updated: Vec<String> = renamed
                .decls
                .iter()
                .filter(|d| !news.contains(&d.qname()) && !owners.contains(&d.qname()) && before.get(&d.qname()).is_some_and(|h| *h != store::self_hash(d)))
                .map(|d| d.qname())
                .collect();
            // prose is not rewritten: point at docs and comments that still say an old name
            let shorts: Vec<String> = pairs.iter().map(|(o, _, _)| crate::names::short(&o).to_string()).collect();
            let word = |t: &str| t.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| shorts.iter().any(|s| s == w));
            let mentions: Vec<String> = renamed
                .decls
                .iter()
                .filter(|d| d.doc.iter().chain(&d.comments).any(|l| word(l)))
                .map(|d| d.qname())
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
        }
        "move" => {
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
            let before: HashMap<String, String> = cb.program.decls.iter().map(|d| (d.qname(), store::self_hash(d))).collect();
            let is_use = |d: &Decl| matches!(d.kind, DeclKind::Use(_));
            let updated: Vec<String> = moved.decls.iter().filter(|d| !is_use(d) && d.qname() != new_q && before.get(&d.qname()).is_some_and(|h| *h != store::self_hash(d))).map(|d| d.qname()).collect();
            let shown = |m: &str| if m.is_empty() { "main".to_string() } else { m.to_string() };
            // imports the move added or changed, by module
            let old_uses: std::collections::HashSet<(String, String)> = cb.program.decls.iter().filter(|d| is_use(d)).map(|d| (d.module.clone(), printer::decl(d))).collect();
            let imports: Vec<String> = moved.decls.iter().filter(|d| is_use(d) && !old_uses.contains(&(d.module.clone(), printer::decl(d)))).map(|d| format!("{}: {}", shown(&d.module), printer::decl(d))).collect();
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
        }
        "status" => {
            let Source::Db(cb) = load(&args)? else { return Err("`status` needs a package".into()) };
            let Some(p) = pending::load(&cb) else {
                println!("no pending change");
                return Ok(true);
            };
            let open: std::collections::HashSet<String> = types::check(&cb.program).holes.into_iter().map(|h| h.name).collect();
            let (left, filled): (Vec<&pending::Hole>, Vec<&pending::Hole>) = p.holes.iter().partition(|h| open.contains(&h.name));
            println!("pending change: {}  ({} hole{} left, {} filled)", p.message, left.len(), if left.len() == 1 { "" } else { "s" }, filled.len());
            for h in &left {
                println!("  ?{}  in {}\n      was: {}", h.name, h.decl, h.was);
            }
            println!("{}", if left.is_empty() { "`strata done` records it as one change; `strata abort` undoes it" } else { "fill the holes with `strata edit`; `strata done --keep-holes` records it as is; `strata abort` undoes it" });
        }
        "done" => {
            let Source::Db(mut cb) = load(&args)? else { return Err("`done` needs a package".into()) };
            let p = pending::load(&cb).ok_or("no pending change")?;
            let open: std::collections::HashSet<String> = types::check(&cb.program).holes.into_iter().map(|h| h.name).collect();
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
        }
        "abort" => {
            let Source::Db(cb) = load(&args)? else { return Err("`abort` needs a package".into()) };
            let p = pending::load(&cb).ok_or("no pending change")?;
            pending::restore(&cb, &p)?;
            pending::clear(&cb);
            println!("undid the pending change \"{}\": every file is as it was before it", p.message);
        }
        "diff" => {
            let num = |k: &str| args.val(k).map(|v| v.parse::<usize>().map_err(|_| format!("{k} needs a number"))).transpose();
            let range = match args.val("--file") {
                Some(f) => history::git_range(std::path::Path::new(f), args.pos.get(1).map(|s| s.as_str()).or(args.val("--base")))?,
                None => {
                    let Source::Db(cb) = load(&args)? else { unreachable!() };
                    history::log_range(&cb, num("--from")?, num("--to")?)?
                }
            };
            let j = range.diff(num("--step")?)?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&j).unwrap());
                return Ok(true);
            }
            println!("{}", j["range"]["label"].as_str().unwrap_or(""));
            for st in j["steps"].as_array().into_iter().flatten() {
                let g = st["group"].as_str().filter(|g| !g.is_empty()).map(|g| format!("{g}  ")).unwrap_or_default();
                let n = st["names"].as_array().map_or(0, |a| a.len());
                println!("  step {:<3} {g}{}  ({n} definition{})", st["id"], st["title"].as_str().unwrap_or(""), if n == 1 { "" } else { "s" });
            }
            println!();
            print!("{}", diff::render_text(&j));
        }
        "merge-file" => {
            let (Some(o), Some(a), Some(b)) = (args.pos.get(1), args.pos.get(2), args.pos.get(3)) else {
                return Err("usage: strata merge-file BASE OURS THEIRS [PATH]  (git: `strata merge-file %O %A %B %P`)".into());
            };
            let path = args.pos.get(4).unwrap_or(a);
            let read = |f: &str| std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"));
            let (base, ours, theirs) = (read(o)?, read(a)?, read(b)?);
            match merge::merge(&base, &ours, &theirs) {
                Ok(m) => {
                    std::fs::write(a, &m.text).map_err(|e| format!("{a}: {e}"))?;
                    let mut ok = m.conflicts.is_empty();
                    for n in &m.notes {
                        eprintln!("strata: {path}: {n}");
                    }
                    for c in &m.conflicts {
                        eprintln!("strata: {path}: conflict in {c}");
                    }
                    // under `strata sync`, the check waits until every file is merged and the
                    // logged renames are followed: while git merges, the other files are still ours
                    let in_sync = std::env::var_os("STRATA_MERGE_BASE").is_some();
                    if ok && !in_sync {
                        match merge::new_errors(path, &m.text, &ours, &theirs) {
                            Ok(errs) if !errs.is_empty() => {
                                ok = false;
                                eprintln!("strata: {path}: merged definition by definition, but the result doesn't check:");
                                for e in errs {
                                    eprintln!("    {e}");
                                }
                            }
                            Ok(_) => {}
                            Err(e) => eprintln!("strata: {path}: not type-checked: {e}"),
                        }
                    }
                    return Ok(ok);
                }
                Err(e) => {
                    eprintln!("strata: {path}: {e}; merging it as text");
                    let (text, clean) = merge::text_merge(&base, &ours, &theirs)?;
                    std::fs::write(a, text).map_err(|e| format!("{a}: {e}"))?;
                    return Ok(clean);
                }
            }
        }
        "setup-git" => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let driver = format!("'{}' merge-file %O %A %B %P", exe.display());
            crate::git::run(None, &["config", "merge.strata.name", "Strata: merge each definition separately, then type-check"])?;
            crate::git::run(None, &["config", "merge.strata.driver", &driver])?;
            println!("git will merge .strata files with: {driver}");
            let top = crate::git::run(None, &["rev-parse", "--show-toplevel"])?;
            let attrs = std::fs::read_to_string(std::path::Path::new(top.trim()).join(".gitattributes")).unwrap_or_default();
            if !attrs.lines().any(|l| l.starts_with("*.strata") && l.split_whitespace().any(|w| w == "merge=strata")) {
                println!("add `*.strata merge=strata` to .gitattributes (and commit it) so git uses it");
            }
        }
        "sync" => {
            let branch = args.pos.get(1).map(|s| s.as_str()).unwrap_or("main");
            if !crate::git::run(None, &["status", "--porcelain", "--untracked-files=no"])?.trim().is_empty() {
                return Err("there are uncommitted changes; commit them first".into());
            }
            let target = if crate::git::run(None, &["remote", "get-url", "origin"]).is_ok() {
                crate::git::run(None, &["fetch", "origin", branch])?;
                format!("origin/{branch}")
            } else {
                branch.to_string()
            };
            crate::git::run(None, &["rev-parse", "--verify", &format!("{target}^{{commit}}")]).map_err(|_| format!("no branch `{target}`"))?;
            let is_ancestor = |a: &str, b: &str| crate::git::ok(None, &["merge-base", "--is-ancestor", a, b]);
            if is_ancestor(&target, "HEAD") {
                println!("already up to date with {target}");
                return Ok(true);
            }
            if is_ancestor("HEAD", &target) {
                crate::git::run(None, &["merge", "--ff-only", &target])?;
                println!("fast-forwarded to {target}");
                return ci(false);
            }
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let attrs = crate::git::run(None, &["rev-parse", "--git-path", "strata-sync-attributes"])?.trim().to_string();
            std::fs::write(&attrs, "*.strata merge=strata\nstrata.log merge=union\n").map_err(|e| format!("{attrs}: {e}"))?;
            let base = crate::git::run(None, &["merge-base", "HEAD", &target])?.trim().to_string();
            let status = Command::new("git")
                .args(["-c", "merge.strata.name=strata", "-c"])
                .arg(format!("merge.strata.driver='{}' merge-file %O %A %B %P", exe.display()))
                .args(["-c", &format!("core.attributesFile={attrs}"), "merge", "--no-ff", "--no-commit", &target])
                // the driver reads the renames both sides logged since the merge base
                .env("STRATA_MERGE_BASE", &base)
                .env("STRATA_MERGE_THEIRS", &target)
                .status()
                .map_err(|e| format!("git: {e}"));
            let _ = std::fs::remove_file(&attrs);
            status?;
            let conflicted = crate::git::run(None, &["diff", "--name-only", "--diff-filter=U"])?;
            if !conflicted.trim().is_empty() {
                eprintln!("\nconflicts with {target} in:");
                for f in conflicted.lines() {
                    eprintln!("  {f}");
                }
                eprintln!("resolve them, run `strata ci`, then `git commit` (or `git merge --abort`)");
                return Ok(false);
            }
            // a rename on one side reaches the other side's new calls, in any file
            let top = std::path::PathBuf::from(crate::git::run(None, &["rev-parse", "--show-toplevel"])?.trim());
            for note in merge::follow_renames(&top, &base, &target)? {
                println!("{note}");
            }
            if !ci(false)? {
                eprintln!("\nthe merge with {target} doesn't check, so it is staged but not committed:");
                eprintln!("fix it, run `strata ci`, then `git commit` (or `git merge --abort`)");
                return Ok(false);
            }
            crate::git::run(None, &["commit", "--no-edit"])?;
            println!("merged {target}");
        }
        "ci" => return ci(args.flag("--fix")),
        "log" => {
            let Source::Db(cb) = load(&args)? else { return Err("`log` needs a codebase".into()) };
            for e in cb.log() {
                if json_out {
                    println!("{e}");
                    continue;
                }
                println!("● {}", e["message"].as_str().unwrap_or(""));
                for c in e["changes"].as_array().into_iter().flatten() {
                    let tag = match (c["old"].is_null(), c["new"].is_null()) {
                        (true, _) => "+",
                        (_, true) => "-",
                        _ if c["interface_changed"] == true => "~ interface",
                        _ => "~ impl",
                    };
                    println!("    {:<12} {}", tag, c["name"].as_str().unwrap_or(""));
                }
            }
        }
        "ui" => {
            let port: u16 = args.val("--port").unwrap_or("7777").parse().map_err(|_| "bad --port")?;
            let target = match args.val("--file") {
                Some(f) => ui::Target::File(f.into()),
                None => {
                    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
                    ui::Target::Db(Codebase::find(&cwd).ok_or("no package here (no strata.toml in this folder or above: run `strata init`, or pass --file)")?)
                }
            };
            ui::serve(target, port)?;
        }
        "builtins" => {
            if json_out {
                let out: Vec<_> = types::BUILTINS.iter().map(|b| json!({"name": b.name, "type_params": b.params, "signature": b.sig, "total": b.total})).collect();
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
            } else {
                println!("// prelude types\n");
                for d in &types::prelude().decls {
                    println!("{}\n", printer::decl(&d));
                }
                println!("// builtin functions\n");
                for b in types::BUILTINS {
                    let tp = if b.params.is_empty() { String::new() } else { format!("[{}]", b.params) };
                    println!("{}{tp}{}", b.name, b.sig);
                }
            }
        }
        "render" => {
            let src = load(&args)?;
            let p = src.program();
            match args.val("--split") {
                None => {
                    // a module is a file: print one
                    let mut mods: Vec<String> = p.decls.iter().map(|d| d.module.clone()).collect();
                    mods.sort();
                    mods.dedup();
                    let m = match (args.pos.get(1), mods.as_slice()) {
                        (Some(m), _) if m == "main" => String::new(),
                        (Some(m), _) if mods.contains(m) => m.clone(),
                        (Some(m), _) => return Err(format!("no module `{m}` (modules: {})", mods.iter().map(|m| if m.is_empty() { "main" } else { m }).collect::<Vec<_>>().join(", "))),
                        (None, [] | [_]) => mods.first().cloned().unwrap_or_default(),
                        (None, _) => {
                            return Err(format!(
                                "{} modules, one file each: `render MODULE` prints one ({}), `render --split DIR` writes them all",
                                mods.len(),
                                mods.iter().map(|m| if m.is_empty() { "main" } else { m }).collect::<Vec<_>>().join(", ")
                            ))
                        }
                    };
                    print!("{}", printer::module_file(p, &m));
                }
                Some(dir) => {
                    // one file per module; the root module goes to main.strata
                    let mut mods: Vec<String> = p.decls.iter().map(|d| d.module.clone()).collect();
                    mods.sort();
                    mods.dedup();
                    for m in mods {
                        let rel = if m.is_empty() { "main".to_string() } else { m.replace('.', "/") };
                        let path = std::path::Path::new(dir).join(format!("{rel}.strata"));
                        if let Some(parent) = path.parent() {
                            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                        }
                        std::fs::write(&path, printer::module_file(p, &m)).map_err(|e| e.to_string())?;
                        println!("wrote {}", path.display());
                    }
                }
            }
        }
        "interface" => {
            // what other modules can see: public definitions at interface level
            let src = load(&args)?;
            let p = src.program();
            let m = args.pos.get(1).ok_or("usage: strata interface <module>")?;
            let names: std::collections::BTreeSet<String> = p.decls.iter().filter(|d| &d.module == m && d.public).map(|d| d.qname()).collect();
            if names.is_empty() {
                return Err(format!("module `{m}` has no public definitions (or does not exist)"));
            }
            let hashes: Vec<String> = p.decls.iter().filter(|d| names.contains(&d.qname())).map(store::interface_hash).collect();
            println!("// contract of module {m}  #{}\n", store::sha(&hashes.join(",")));
            let st = view::Status { holes: HashMap::new(), verdicts: verdicts_for(&src) };
            print!("{}", view::render(p, &names, 1, &st));
        }
        "ast" => {
            let src = load(&args)?;
            let name = args.pos.get(1).ok_or("usage: strata ast <name>")?;
            let d = src.program().decls.iter().find(|d| &d.qname() == name).ok_or(format!("no definition `{name}`"))?;
            println!("{}", serde_json::to_string_pretty(&json!({"name": name, "hash": store::self_hash(d), "interface_hash": store::interface_hash(d), "node": d})).unwrap());
        }
        "view" => {
            let src = load(&args)?;
            let p = src.program();
            let level: u8 = args.val("--level").unwrap_or("1").parse().map_err(|_| "--level must be 0-3")?;
            let names = view::select(p, args.pos.get(1).map(|s| s.as_str()));
            let rep = types::check(p);
            let mut holes: HashMap<String, Vec<String>> = HashMap::new();
            for h in rep.holes {
                holes.entry(h.decl).or_default().push(h.name);
            }
            let st = view::Status { holes, verdicts: verdicts_for(&src) };
            print!("{}", view::render(p, &names, level, &st));
        }
        "check" => {
            let src = load(&args)?;
            let rep = full_check(src.program());
            if json_out {
                println!("{}", serde_json::to_string_pretty(&rep).unwrap());
            } else {
                print_errors(&rep);
                let n = src.program().decls.len();
                if rep.ok() {
                    println!("ok: {n} definitions check ({} open holes)", rep.holes.len());
                }
            }
            return Ok(rep.ok());
        }
        "holes" => {
            let src = load(&args)?;
            let rep = types::check(src.program());
            if json_out {
                println!("{}", serde_json::to_string_pretty(&rep.holes).unwrap());
                return Ok(true);
            }
            if rep.holes.is_empty() {
                println!("no holes");
            }
            let open = match &src {
                Source::Db(cb) => pending::load(cb),
                _ => None,
            };
            for h in &rep.holes {
                println!("?{}  in {}\n  goal:    {}", h.name, h.decl, h.goal);
                if let Some(ph) = open.as_ref().and_then(|p| p.holes.iter().find(|x| x.name == h.name)) {
                    println!("  was:     {}\n  why:     {}", ph.was, ph.why.join("; "));
                }
                if !h.context.is_empty() {
                    println!("  context: {}", h.context.iter().map(|(n, t)| format!("{n}: {t}")).collect::<Vec<_>>().join(", "));
                }
                for o in &h.obligations {
                    println!("  must:    {o}");
                }
                println!();
            }
        }
        "verify" => {
            let src = load(&args)?;
            let p = src.program();
            let rep = full_check(p);
            if !rep.ok() {
                print_errors(&rep);
                return Ok(false);
            }
            let (env, _) = types::Env::new(p);
            let cases: usize = args.val("--cases").unwrap_or("200").parse().map_err(|_| "bad --cases")?;
            let seed: u64 = args.val("--seed").unwrap_or("1").parse().map_err(|_| "bad --seed")?;
            let names = view::select(p, args.pos.get(1).map(|s| s.as_str()));
            let mut cache = match &src {
                Source::Db(cb) => cb.load_verdicts(),
                _ => HashMap::new(),
            };
            let mut all_ok = true;
            let mut results = Vec::new();
            let resolved = types::elaborate(p).program;
            let solver = smt::solver();
            let solver_bin = solver.as_ref();
            let analysis = termination::analyze(&env, solver_bin);
            let transparent = &analysis.transparent;
            let cross_check: usize = args.val("--cross-check").unwrap_or("20").parse().map_err(|_| "bad --cross-check")?;
            let require: Vec<String> = args.val("--require").map(|r| r.split(',').map(|s| s.trim().to_string()).collect()).unwrap_or_default();
            // the package's `[verify]` rules apply too
            let policy = package::find(std::path::Path::new(args.val("--file").unwrap_or("."))).ok().flatten().map(|p| p.verify).unwrap_or_default();
            let mut module_of: HashMap<String, String> = HashMap::new();
            for r in &require {
                if !["proven", "terminates", "no-recursion"].contains(&r.as_str()) {
                    return Err(format!("unknown requirement `{r}` (use proven, terminates, no-recursion)"));
                }
            }
            if args.flag("--smt") {
                // show the queries instead of running them
                for d in &resolved.decls {
                    if let (true, DeclKind::Fn(f)) = (names.contains(&d.qname()), &d.kind) {
                        for (what, q) in verify::smt_queries(&env, f, transparent).into_iter().chain(termination::queries(&env, f, solver_bin)) {
                            println!("; {} — {what}\n{q}", f.name);
                        }
                    }
                }
                return Ok(true);
            }
            // results depend on the solver: it is part of the cache key
            let skey = solver.as_ref().map(|s| s.version.clone()).unwrap_or("no-solver".into());
            for (orig, d) in p.decls.iter().zip(&resolved.decls) {
                if !names.contains(&orig.qname()) {
                    continue;
                }
                let env = &env;
                // only code shown to terminate is cross-checked: running anything else may never end
                let analysis = &analysis;
                let cc = move |label: &str| if matches!(analysis.status.get(label), Some(termination::Termination::Proven { .. })) { cross_check } else { 0 };
                // (label, cache key, how to check)
                let mut jobs: Vec<(String, String, Box<dyn Fn() -> verify::Verdict + '_>)> = Vec::new();
                let base = store::closure_hash(p, &orig.qname());
                match &d.kind {
                    DeclKind::Fn(f) => {
                        let n = cc(&f.name);
                        jobs.push((f.name.clone(), format!("{base}:{cases}:{n}:{skey}"), Box::new(move || verify::check_fn(env, f, None, solver_bin, transparent, cases, n, seed))))
                    }
                    DeclKind::Impl(imp) if imp.is_inherent() => {
                        // a type's own methods are functions (`Item.new`, `Item.describe`), checked as such
                        for m in &imp.methods {
                            let short = crate::names::short(&m.name).to_string();
                            let f = types::static_fn(imp, m);
                            let n = cc(&f.name);
                            let key = format!("{base}:{short}:{cases}:{n}:{skey}");
                            jobs.push((f.name.clone(), key, Box::new(move || verify::check_fn(env, &f, None, solver_bin, transparent, cases, n, seed))));
                        }
                    }
                    DeclKind::Impl(imp) => {
                        let Some(tr) = env.traits.get(&imp.trait_name) else { continue };
                        let ty = printer::ty(&imp.for_type);
                        for m in &tr.methods {
                            let mname = m.decl.name.clone();
                            let decl = imp.methods.iter().find(|x| x.name == mname).or(if m.default { Some(&m.decl) } else { None });
                            match decl {
                                Some(decl) => {
                                    let label = termination::impl_label(env, imp, &mname);
                                    let n = cc(&label);
                                    jobs.push((label, format!("{base}:{mname}:{cases}:{n}:{skey}"), Box::new(move || verify::check_fn(env, decl, Some(imp), solver_bin, transparent, cases, n, seed))))
                                }
                                None => jobs.push((termination::impl_label(env, imp, &mname), format!("{base}:{mname}:{cases}"), Box::new(move || verify::fuzz_impl_method(env, imp, &mname, cases, seed)))),
                            }
                        }
                        for law in &tr.laws {
                            jobs.push((format!("law {} for {ty}", law.name), format!("{base}:law:{}:{cases}", law.name), Box::new(move || verify::check_law(env, imp, law, cases, seed))));
                        }
                    }
                    _ => {}
                }
                let mut worst: Option<verify::Verdict> = None;
                for (label, key, job) in jobs {
                    let (verdict, cached) = match cache.get(&key) {
                        Some(v) if !args.flag("--force") => (v.clone(), true),
                        _ => (job(), false),
                    };
                    let verdict = verdict.with_termination(analysis.status.get(&label).cloned());
                    cache.insert(key.clone(), verdict.clone());
                    if matches!(verdict, verify::Verdict::Counterexample { .. }) {
                        all_ok = false;
                    }
                    if worst.as_ref().map_or(true, |w| verdict.rank() > w.rank()) {
                        worst = Some(verdict.clone());
                    }
                    module_of.insert(label.clone(), orig.module.clone());
                    results.push((label, verdict, cached));
                }
                // the viewer shows one status per definition
                if let Some(w) = worst {
                    cache.insert(base, w);
                }
            }
            if let Source::Db(cb) = &src {
                cb.save_verdicts(&cache);
            }
            // the project's policy
            let mut violations = vec![];
            // what `proven` accepted on trust, listed after the results
            let mut trusted = vec![];
            for (n, v, _) in &results {
                if n.starts_with("law ") {
                    continue;
                }
                let module = module_of.get(n).map(|m| m.as_str()).unwrap_or("");
                // a rule applies from the command line (everywhere) or from strata.toml (its modules)
                let rule = |flag: &str, listed: &[String]| -> Option<String> {
                    if require.iter().any(|r| r == flag) {
                        Some(String::new())
                    } else if package::Policy::covers(listed, module) {
                        Some(format!(" ({} [verify])", package::FILE))
                    } else {
                        None
                    }
                };
                if let Some(src) = rule("proven", &policy.proven) {
                    match v {
                        verify::Verdict::Proven { .. } => {}
                        // trusted code meets `proven`, and is listed as such
                        verify::Verdict::Assumed { .. } => trusted.push(format!("`{n}`")),
                        _ => violations.push(format!("`{n}` is not proven{src}")),
                    }
                }
                if let Some(src) = rule("terminates", &policy.terminates) {
                    if let Some(termination::Termination::Unknown { .. }) = v.termination() {
                        violations.push(format!("`{n}` may not terminate{src}"));
                    }
                }
                if let Some(src) = rule("no-recursion", &policy.no_recursion) {
                    if analysis.recursive.contains(n) {
                        violations.push(format!("`{n}` is recursive{src}"));
                    }
                }
            }
            if !violations.is_empty() {
                all_ok = false;
            }
            if json_out {
                let out: Vec<_> = results.iter().map(|(n, v, c)| json!({"fn": n, "cached": c, "verdict": v})).collect();
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
                return Ok(all_ok);
            }
            for (n, v, cached) in &results {
                let c = if *cached { " (cached)" } else { "" };
                match v {
                    verify::Verdict::Proven { checks, .. } => {
                        match checks.len() {
                            0 => println!("✓ {n:<24} proven (nothing to check){c}"),
                            k => println!("✓ {n:<24} proven ({k} check{}){c}", if k == 1 { "" } else { "s" }),
                        }
                        if args.flag("--checks") {
                            for ch in checks {
                                println!("    proven:  {ch}");
                            }
                        }
                    }
                    verify::Verdict::Fuzzed { cases, discarded, proven, unproven, .. } => {
                        let total = proven + unproven.len();
                        let p = if total > 0 { format!("; {proven} of {total} checks proven") } else { String::new() };
                        println!("~ {n:<24} fuzzed: {cases} cases, {discarded} discarded by `requires`{p}{c}");
                        for u in unproven {
                            println!("    not proven: {u}");
                        }
                    }
                    verify::Verdict::Counterexample { args, kind, message, found_by, .. } => {
                        let by = if found_by.is_empty() { String::new() } else { format!(" (found by the {found_by})") };
                        println!("✗ {n:<24} {kind} violation{by}{c}\n    input:   {args}");
                        println!("    {message}");
                    }
                    verify::Verdict::Incomplete { message, .. } => println!("○ {n:<24} incomplete: {message}{c}"),
                    verify::Verdict::Assumed { why } => println!("≈ {n:<24} assumed: {why}"),
                    verify::Verdict::Skipped { reason } => println!("- {n:<24} skipped: {reason}"),
                }
                match v.termination() {
                    Some(termination::Termination::Unknown { reason }) => println!("    may not terminate: {reason}"),
                    Some(termination::Termination::Assumed { by }) => {
                        println!("    terminates if {} does (marked `terminates`, not proven)", by.iter().map(|b| format!("`{b}`")).collect::<Vec<_>>().join(", "))
                    }
                    Some(termination::Termination::Proven { how }) if args.flag("--checks") => println!("    terminates: {how}"),
                    _ => {}
                }
            }
            if !trusted.is_empty() {
                println!("policy: `proven` accepts {} as assumed (trusted, not checked)", trusted.join(", "));
            }
            for v in &violations {
                println!("policy: {v}");
            }
            return Ok(all_ok);
        }
        "run" => {
            // `strata run hello.strata`: the program's path in place of --file
            let (src, rest, path) = match args.pos.get(1) {
                Some(f) if f.ends_with(".strata") || std::path::Path::new(f).join(package::FILE).is_file() => (Source::File(read_sources(f, None)?), &args.pos[2..], f.clone()),
                _ => (load(&args)?, &args.pos[1..], args.val("--file").unwrap_or(".").to_string()),
            };
            // the program's name, for `--help`: its package's, or the file's
            let path = std::path::Path::new(&path);
            let prog = match package::find(path) {
                Ok(Some(pkg)) => pkg.name,
                _ => path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "main".into()),
            };
            let p = src.program();
            let rep = full_check(p);
            if !rep.ok() {
                print_errors(&rep);
                return Ok(false);
            }
            let (env, _) = types::Env::new(p);
            let Some(name) = rest.first() else {
                let program_args = args.flags.get("--").cloned().unwrap_or_default();
                std::process::exit(run_main(p, &env, &prog, program_args, args.flag("--trace"))?);
            };
            let f = env.fns.get(name).ok_or(format!("no function `{name}`"))?.clone();
            let raw: serde_json::Value = serde_json::from_str(rest.get(1).map(|s| s.as_str()).unwrap_or("[]")).map_err(|e| format!("args: {e}"))?;
            let raw = raw.as_array().cloned().ok_or("args must be a JSON array")?;
            if raw.len() != f.params.len() {
                return Err(format!("`{name}` takes {} arguments", f.params.len()));
            }
            let seed: u64 = args.val("--seed").unwrap_or("1").parse().map_err(|_| "bad --seed")?;
            let mut it = interp::Interp::new(&env, seed);
            it.tracing = args.flag("--trace");
            // what it prints comes before its result
            it.output = interp::Output::Terminal;
            let mut vals = Vec::new();
            for (pm, j) in f.params.iter().zip(&raw) {
                vals.push(it.from_json(&pm.ty, j, &pm.name).map_err(|e| format!("invalid input: {e}"))?);
            }
            let res = it.call_fn(name, vals);
            if it.tracing {
                for t in &it.trace {
                    eprintln!("{t}");
                }
            }
            match res {
                Ok(v) => println!("{}", serde_json::to_string_pretty(&interp::to_json(&v)).unwrap()),
                Err(fault) => {
                    println!("{}", serde_json::to_string_pretty(&json!({"fault": fault})).unwrap());
                    return Ok(false);
                }
            }
        }
        "test" => {
            let src = load(&args)?;
            let p = src.program();
            let rep = full_check(p);
            if !rep.ok() {
                print_errors(&rep);
                return Ok(false);
            }
            let (env, _) = types::Env::new(p);
            let filter = args.pos.get(1).cloned().unwrap_or_default();
            let (mut pass, mut fail) = (0, 0);
            let resolved = types::elaborate(p).program;
            for d in &resolved.decls {
                let DeclKind::Test(t) = &d.kind else { continue };
                if !t.name.contains(&filter) {
                    continue;
                }
                let mut it = interp::Interp::new(&env, 7);
                // each test prints into its own buffer, shown only if it fails
                it.output = interp::Output::Buffer(String::new());
                match it.eval(&t.body, &None) {
                    Ok(_) => {
                        pass += 1;
                        println!("✓ {}", t.name);
                    }
                    Err(f) => {
                        fail += 1;
                        println!("✗ {}\n    {:?}: {}", t.name, f.kind, f.msg);
                        if !f.stack.is_empty() {
                            println!("    stack: {}", f.stack.join(" -> "));
                        }
                        if let interp::Output::Buffer(out) = &it.output {
                            if !out.is_empty() {
                                println!("    printed:");
                                for l in out.lines() {
                                    println!("      {l}");
                                }
                            }
                        }
                    }
                }
            }
            println!("\n{pass} passed, {fail} failed");
            return Ok(fail == 0);
        }
        other => return Err(format!("unknown command `{other}` (try `strata help`)")),
    }
    Ok(true)
}

/// Apply edited definitions (and deletions) to the package: committed if the
/// whole package checks; if they check but break code elsewhere, the broken
/// uses become holes in a pending change. `Err` holds the errors of a rejected edit.
fn apply_edit(cb: &mut Codebase, cmd: &str, edits: Program, deletes: Vec<String>, msg: &str, json_out: bool) -> Result<Result<(), Vec<String>>, String> {
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
                *per.entry(if m.is_empty() { "(root)".into() } else { m }).or_default() += 1;
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
            if d.public && !d.module.is_empty() && !contracts.contains(&d.module) {
                contracts.push(d.module.clone());
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
fn record(cb: &mut Codebase, new: Program, msg: &str, extra: serde_json::Value) -> Result<Vec<store::Change>, String> {
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
const EDIT_MARK: &str = "// ✗ ";

/// The user's editor: `$VISUAL`, `$EDITOR`, git's, or `vi`.
fn editor_command() -> String {
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
fn edit_module(p: &Program, arg: &str) -> Option<String> {
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
struct EditFile {
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
fn edit_in_editor(cb: &mut Codebase, args: &Args, json_out: bool) -> Result<bool, String> {
    let p = cb.program.clone();
    let mut mods: Vec<String> = p.decls.iter().map(|d| d.module.clone()).collect();
    mods.sort();
    mods.dedup();
    let whole = |m: &str| -> (Vec<String>, String) {
        let shown = p.decls.iter().filter(|d| d.module == m).map(|d| d.qname()).collect();
        let text = if p.decls.iter().any(|d| d.module == m) { printer::module_file(&p, m) } else { String::new() };
        (shown, text)
    };
    let dir = cb.cache_dir().join("edit");
    let _ = std::fs::remove_dir_all(&dir);
    let file = |module: String, shown: Vec<String>, text: String| EditFile { path: dir.join(store::module_path(&module)), module, shown, text };
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
            let kept: std::collections::HashSet<String> = edits.decls.iter().map(|d| d.qname()).collect();
            let deletes: Vec<String> = files.iter().flat_map(|f| f.shown.iter()).filter(|q| !kept.contains(*q)).cloned().collect();
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
fn git_commit(cb: &Codebase, msg: &str) {
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

/// What CI requires, over the whole repository (or the current folder outside
/// git): every package and every script checks, every package's exports name
/// something, and every `.strata` file is in canonical format (what `render`
/// prints). `fix` rewrites the files that aren't.
fn ci(fix: bool) -> Result<bool, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let top = crate::git::run(None, &["rev-parse", "--show-toplevel"]).map(|t| std::path::PathBuf::from(t.trim())).unwrap_or(cwd);
    let top = std::fs::canonicalize(&top).unwrap_or(top);
    let (packages, scripts) = package::discover(&top)?;
    if packages.is_empty() && scripts.is_empty() {
        return Err(format!("no .strata files under {}", top.display()));
    }
    let mut ok = true;
    for path in packages.iter().chain(&scripts) {
        let shown = path.strip_prefix(&top).unwrap_or(path).display().to_string();
        let pkg = if path.is_dir() { Some(package::load(path)?) } else { None };
        let files = package::files(path)?;
        let mut parsed = true;
        for (rel, src) in &files {
            let file = if rel.as_os_str().is_empty() { shown.clone() } else { format!("{shown}/{}", rel.display()) };
            match parser::parse_module_file(src, "") {
                Err(e) => {
                    ok = false;
                    parsed = false;
                    println!("✗ {file}: doesn't parse: {e}");
                }
                Ok(p) if printer::program(&p) != *src => {
                    if fix {
                        std::fs::write(top.join(&file), printer::program(&p)).map_err(|e| format!("{file}: {e}"))?;
                        println!("  {file}: rewritten in canonical format");
                    } else {
                        ok = false;
                        println!("✗ {file}: not in canonical format (`strata ci --fix` rewrites it)");
                    }
                }
                Ok(_) => {}
            }
        }
        if !parsed {
            continue;
        }
        let p = package::program(path)?;
        let rep = full_check(&p);
        let mut errors: Vec<String> = rep.errors.iter().map(|e| format!("error in {}: {}", e.decl, e.msg)).collect();
        // holes of an unfinished change are not to be merged
        errors.extend(rep.holes.iter().filter(|h| h.name.starts_with(pending::PREFIX)).map(|h| {
            format!("?{} in {} belongs to an unfinished change: fill it and run `strata done`, or `strata abort`", h.name, h.decl)
        }));
        if let Some(pkg) = &pkg {
            errors.extend(package::bad_exports(pkg, &p).into_iter().chain(package::bad_policy(pkg, &p)).map(|e| format!("{}: {e}", package::FILE)));
        }
        let label = match &pkg {
            Some(pkg) => format!("package {} ({shown})", pkg.name),
            None => shown.clone(),
        };
        if errors.is_empty() {
            println!("✓ {label}: {} definitions check ({} open holes)", p.decls.len(), rep.holes.len());
        } else {
            ok = false;
            println!("✗ {label}:");
            for e in errors {
                println!("    {e}");
            }
        }
    }
    Ok(ok)
}

/// Run the program's `main` with the real system, and give its exit code.
/// `main` asks for the capabilities it needs by type (`Console`, `Files`,
/// `Environment`, in any order) and may return `()` (0), an exit code, or a
/// `Result` of either, whose `Err` is printed to standard error (exit code 1).
fn run_main(p: &Program, env: &types::Env, prog: &str, program_args: Vec<String>, trace: bool) -> Result<i32, String> {
    use interp::Value;
    let f = env.fns.get("main").ok_or("no `main` function to run (the entry point is `fn main` in main.strata, or in the file you run)")?.clone();
    let mut it = interp::Interp::new(env, 1);
    it.tracing = trace;
    it.host = Some(interp::Host { args: program_args.clone() });
    it.output = interp::Output::Terminal;
    let mut vals = Vec::new();
    for pm in &f.params {
        let ty = printer::ty(&pm.ty);
        let rec = |name: &str, fields: Vec<(&str, Value)>| Value::Record(name.into(), fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
        vals.push(match ty.as_str() {
            "std.io.Console" => rec("std.io.Terminal", vec![("stdout", Value::Int(1)), ("stderr", Value::Int(2))]),
            "std.fs.Files" => rec("std.fs.Disk", vec![("root", Value::Str(".".into()))]),
            "std.process.Environment" => rec("std.process.Process", vec![("pid", Value::Int(std::process::id() as i64))]),
            // options read from the command line
            _ if cli::is_options(env, &pm.ty) => {
                let doc = p.decls.iter().find(|d| matches!(d.kind, DeclKind::Type(_)) && d.qname() == ty).map(|d| d.doc.clone()).unwrap_or_default();
                match cli::parse(&mut it, env, &ty, &doc, prog, &program_args) {
                    cli::Parsed::Options(v) => v,
                    cli::Parsed::Help(text) => {
                        print!("{text}");
                        return Ok(0);
                    }
                    cli::Parsed::Error(e) => {
                        eprintln!("error: {e}");
                        return Ok(2);
                    }
                }
            }
            _ => {
                return Err(format!(
                    "`main` can ask for `Console` (std.io), `Files` (std.fs), `Environment` (std.process), and options: a record that implements `CommandLine` (std.cli). Its parameter `{}` is a `{ty}`",
                    pm.name
                ))
            }
        });
    }
    let res = it.call_fn("main", vals);
    if it.tracing {
        for t in &it.trace {
            eprintln!("{t}");
        }
    }
    let code = |v: &Value| match v {
        Value::Unit => Ok(0),
        Value::Int(n) => i32::try_from(*n).map_err(|_| format!("`main` returned {n}, which is not an exit code")),
        other => Err(format!("`main` must return `()`, an `ExitCode` or a `Result` of either, not {}", interp::show(other))),
    };
    match res {
        Ok(Value::Variant(c, interp::Fields::Pos(mut xs))) if c == "Ok" && xs.len() == 1 => code(&xs.pop().unwrap()),
        Ok(Value::Variant(c, interp::Fields::Pos(xs))) if c == "Err" && xs.len() == 1 => {
            let shown = match &xs[0] {
                Value::Str(s) => s.clone(),
                e => interp::show(e),
            };
            eprintln!("error: {shown}");
            Ok(1)
        }
        Ok(v) => code(&v),
        Err(fault) => {
            eprintln!("fault ({:?}): {}", fault.kind, fault.msg);
            if !fault.stack.is_empty() {
                eprintln!("    stack: {}", fault.stack.join(" -> "));
            }
            Ok(101)
        }
    }
}

fn main() -> ExitCode {
    // a deep (but legal) recursion in the interpreter needs more than the
    // default stack; its own depth limit should be what stops it
    let result = std::thread::Builder::new().stack_size(1 << 30).spawn(run).expect("start the main thread").join().unwrap_or_else(|_| Err("internal error".into()));
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
