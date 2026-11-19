mod ast;
mod generate;
mod interp;
mod lexer;
mod parser;
mod printer;
mod store;
mod types;
mod ui;
mod verify;
mod view;

use ast::*;
use serde_json::json;
use std::collections::HashMap;
use std::io::Read;
use std::process::ExitCode;
use store::Codebase;

const USAGE: &str = "strata — an experimental language for working with agents

USAGE: strata [--file F.strata] <command> [args]

Codebase (stored as a database of syntax trees in .strata/):
  init                         create a codebase in the current directory
  add <file.strata> [-m MSG]   import definitions from a file (checked, atomic)
  edit [-m MSG] [--rm NAME]..  read definitions from stdin; replace/add them by name;
                               commit only if the whole codebase still checks
  log                          show the patch history
  render                       print the full codebase as source (L3)

Views:
  ui [--port 7777]                live viewer in the browser (map, levels, holes, activity)
  view [SELECTOR] [--level 0-3]   SELECTOR: name | prefix* | slice:Name | a,b
  ast <name>                      the stored syntax tree as JSON

Checking:
  check [--json]               types, effects, exhaustiveness, literal refinements
  holes [--json]               typed holes with goal type, context and obligations
  verify [SELECTOR] [--cases N] [--json]
                               fuzz every contract; results cached by content hash

Running:
  run <fn> [JSON-ARGS] [--mock] [--trace]
                               args are a JSON array validated against the types;
                               --mock handles the function's effects with generated values
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
        let valued = ["-m", "--level", "--cases", "--file", "--rm", "--seed", "--port"];
        let (mut pos, mut flags) = (Vec::new(), HashMap::<String, Vec<String>>::new());
        let mut i = 0;
        while i < raw.len() {
            let a = &raw[i];
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

fn load(args: &Args) -> Result<Source, String> {
    if let Some(f) = args.val("--file") {
        let src = std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?;
        let p = parser::parse_program(&src).map_err(|e| format!("{f}:{e}"))?;
        return Ok(Source::File(p));
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = Codebase::find(&cwd).ok_or("no .strata codebase here (run `strata init`, or pass --file)")?;
    Ok(Source::Db(Codebase::open(&root)?))
}

/// Full static check: types, effects, exhaustiveness, then literal refinements.
fn full_check(p: &Program) -> types::CheckReport {
    let mut rep = types::check(p);
    let (env, _) = types::Env::new(p);
    for d in verify::static_refinements(p, &env) {
        if !rep.errors.iter().any(|e| e.decl == d.decl && e.msg == d.msg) {
            rep.errors.push(d);
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
        .filter_map(|d| cache.get(&store::closure_hash(&cb.program, &d.name())).map(|v| (d.name(), v.tag())))
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
            Codebase::init(&std::env::current_dir().map_err(|e| e.to_string())?)?;
            println!("initialized empty Strata codebase in {}/", store::DIR);
        }
        "add" | "edit" => {
            let Source::Db(mut cb) = load(&args)? else { return Err("`add`/`edit` need a codebase".into()) };
            let text = if cmd == "add" {
                let f = args.pos.get(1).ok_or("usage: strata add <file.strata>")?;
                std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?
            } else {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
                s
            };
            let edits = parser::parse_program(&text).map_err(|e| format!("parse error at {e}"))?;
            let deletes: Vec<String> = args.flags.get("--rm").cloned().unwrap_or_default();
            let new = store::merge(&cb.program, edits, &deletes);
            let rep = full_check(&new);
            if !rep.ok() {
                print_errors(&rep);
                eprintln!("\nrejected: the codebase would not check; nothing was changed");
                return Ok(false);
            }
            let msg = args.val("-m").unwrap_or(if cmd == "add" { "import" } else { "edit" });
            let changes = cb.commit(new, msg)?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&changes).unwrap());
                return Ok(true);
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
                if !c.affected.is_empty() {
                    println!("         re-check dependents: {}", c.affected.join(", "));
                }
            }
            if !rep.holes.is_empty() {
                println!("{} open hole(s) — see `strata holes`", rep.holes.len());
            }
        }
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
                    ui::Target::Db(Codebase::find(&cwd).ok_or("no .strata codebase here (run `strata init`, or pass --file)")?)
                }
            };
            ui::serve(target, port)?;
        }
        "builtins" => {
            if json_out {
                let out: Vec<_> = types::BUILTINS.iter().map(|(n, ps, s)| json!({"name": n, "type_params": ps, "signature": s})).collect();
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
            } else {
                for (n, ps, s) in types::BUILTINS {
                    let tp = if ps.is_empty() { String::new() } else { format!("[{ps}]") };
                    println!("{n}{tp}{s}");
                }
            }
        }
        "render" => print!("{}", printer::program(load(&args)?.program())),
        "ast" => {
            let src = load(&args)?;
            let name = args.pos.get(1).ok_or("usage: strata ast <name>")?;
            let d = src.program().decls.iter().find(|d| &d.name() == name).ok_or(format!("no definition `{name}`"))?;
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
            for h in &rep.holes {
                println!("?{}  in {}\n  goal:    {}", h.name, h.decl, h.goal);
                if !h.context.is_empty() {
                    println!("  context: {}", h.context.iter().map(|(n, t)| format!("{n}: {t}")).collect::<Vec<_>>().join(", "));
                }
                if !h.effects.is_empty() {
                    println!("  effects: {{{}}}", h.effects.join(", "));
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
            for d in &p.decls {
                let DeclKind::Fn(f) = &d.kind else { continue };
                if !names.contains(&f.name) {
                    continue;
                }
                let key = format!("{}:{cases}", store::closure_hash(p, &f.name));
                let (verdict, cached) = match cache.get(&key) {
                    Some(v) if !args.flag("--force") => (v.clone(), true),
                    _ => {
                        let v = verify::fuzz_fn(&env, f, cases, seed);
                        cache.insert(key.clone(), v.clone());
                        cache.insert(store::closure_hash(p, &f.name), v.clone());
                        (v, false)
                    }
                };
                if matches!(verdict, verify::Verdict::Counterexample { .. }) {
                    all_ok = false;
                }
                results.push((f.name.clone(), verdict, cached));
            }
            if let Source::Db(cb) = &src {
                cb.save_verdicts(&cache);
            }
            if json_out {
                let out: Vec<_> = results.iter().map(|(n, v, c)| json!({"fn": n, "cached": c, "verdict": v})).collect();
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
                return Ok(all_ok);
            }
            for (n, v, cached) in &results {
                let c = if *cached { " (cached)" } else { "" };
                match v {
                    verify::Verdict::Fuzzed { cases, discarded } => {
                        println!("✓ {n:<24} {cases} cases, {discarded} discarded by `requires`{c}")
                    }
                    verify::Verdict::Counterexample { args, effects, kind, message } => {
                        println!("✗ {n:<24} {kind} violation{c}\n    input:   {args}");
                        if effects.as_array().is_some_and(|a| !a.is_empty()) {
                            println!("    effects: {effects}");
                        }
                        println!("    {message}");
                    }
                    verify::Verdict::Incomplete { message } => println!("○ {n:<24} incomplete: {message}{c}"),
                    verify::Verdict::Skipped { reason } => println!("- {n:<24} skipped: {reason}"),
                }
            }
            return Ok(all_ok);
        }
        "run" => {
            let src = load(&args)?;
            let p = src.program();
            let rep = full_check(p);
            if !rep.ok() {
                print_errors(&rep);
                return Ok(false);
            }
            let (env, _) = types::Env::new(p);
            let name = args.pos.get(1).ok_or("usage: strata run <fn> [JSON-ARGS]")?;
            let f = env.fns.get(name).ok_or(format!("no function `{name}`"))?.clone();
            let raw: serde_json::Value = serde_json::from_str(args.pos.get(2).map(|s| s.as_str()).unwrap_or("[]")).map_err(|e| format!("args: {e}"))?;
            let raw = raw.as_array().cloned().ok_or("args must be a JSON array")?;
            if raw.len() != f.params.len() {
                return Err(format!("`{name}` takes {} arguments", f.params.len()));
            }
            let seed: u64 = args.val("--seed").unwrap_or("1").parse().map_err(|_| "bad --seed")?;
            let mut it = interp::Interp::new(&env, seed);
            it.tracing = args.flag("--trace");
            let mut vals = Vec::new();
            for (pm, j) in f.params.iter().zip(&raw) {
                vals.push(it.from_json(&pm.ty, j, &pm.name).map_err(|e| format!("invalid input: {e}"))?);
            }
            if !f.effects.is_empty() {
                if !args.flag("--mock") {
                    return Err(format!("`{name}` performs {{{}}}; pass --mock to handle them with generated values", f.effect_names().join(", ")));
                }
                it.mock_effects(&f.effect_names());
            }
            let res = it.call_fn(name, vals);
            if it.tracing {
                for t in &it.trace {
                    eprintln!("{t}");
                }
            } else if args.flag("--mock") && !it.effect_log.is_empty() {
                for t in &it.effect_log {
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
            for d in &p.decls {
                let DeclKind::Test(t) = &d.kind else { continue };
                if !t.name.contains(&filter) {
                    continue;
                }
                let mut it = interp::Interp::new(&env, 7);
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

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
