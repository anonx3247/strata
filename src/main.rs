mod ast;
mod binary;
mod cli;
mod cmd;
mod diff;
mod history;
mod generate;
mod git;
mod interp;
mod lexer;
mod llvm;
mod merge;
mod names;
mod package;
mod pending;
mod parser;
mod printer;
mod resolve;
mod serve;
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
  pack <OUT>                   write the whole program (and its strata.toml) to OUT, one
                               binary file (docs/binary-format.md); --file, run, ui and
                               every view read it like source
  unpack <FILE> <DIR>          write a binary file back out as source: DIR/strata.toml and
                               one file per module, in canonical form
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
  run --jit [PATH] [-- ARGS..] the same, compiled to machine code in memory (LLVM) and run:
                               much faster, when the program only uses what compiles
                               (see build); otherwise interpreted, with a note saying why.
                               Needs strata built with `--features jit`
  serve [PATH] [--port N]      serve the web: each request is passed to the program's
                               `fn handle(req: Request) -> Response` (std.http), which may
                               also ask for Console, Files, Environment (default port 8000);
                               or run it as an app: `init`, `update` and `view` (std.web)
  run <fn> [JSON-ARGS] [--trace]
                               call one function: args are a JSON array validated
                               against the types; --trace prints each call and return
                               to stderr
  test [FILTER]                run test blocks
  build [PATH] [-o OUT] [--emit-llvm]
                               compile the program to a native executable, through LLVM
                               (needs clang; STRATA_CC chooses another): `main` takes no
                               parameters yet, and only part of the language compiles
                               (Int, Float, Bool, String, functions, control flow,
                               contracts). --emit-llvm writes the LLVM IR instead
  builtins [--json]            list built-in functions and their signatures

With --file, commands operate on a source file instead of a codebase.";

struct Args {
    pos: Vec<String>,
    flags: HashMap<String, Vec<String>>,
}

impl Args {
    fn parse() -> Args {
        let raw: Vec<String> = std::env::args().skip(1).collect();
        let valued = ["-m", "--level", "--cases", "--file", "--rm", "--seed", "--port", "--module", "--split", "--from", "--to", "--step", "--base", "--require", "--cross-check", "-o"];
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
/// `main.strata` -> the root module). A binary file (`strata pack`) holds a
/// whole program, already parsed.
fn read_sources(path: &str, module: Option<&str>) -> Result<Program, String> {
    let root = std::path::Path::new(path);
    if !root.is_dir() {
        let bytes = std::fs::read(root).map_err(|e| format!("{path}: {e}"))?;
        if binary::is_binary(&bytes) {
            return binary::decode(&bytes).map(|img| img.program).map_err(|e| format!("{path}: {e}"));
        }
        let src = String::from_utf8(bytes).map_err(|_| format!("{path}: not UTF-8 text (nor a binary Strata file)"))?;
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
        "help" | "--help" => {
            println!("{USAGE}");
            Ok(true)
        }
        "init" => cmd::init(),
        "add" | "edit" => cmd::edit(&args, cmd, json_out),
        "rename" => cmd::rename(&args, json_out),
        "move" => cmd::move_def(&args, json_out),
        "status" => cmd::status(&args),
        "done" => cmd::done(&args),
        "abort" => cmd::abort(&args),
        "diff" => cmd::diff(&args, json_out),
        "merge-file" => cmd::merge_file(&args),
        "setup-git" => cmd::setup_git(),
        "sync" => cmd::sync(&args),
        "ci" => cmd::ci(args.flag("--fix")),
        "log" => cmd::log(&args, json_out),
        "ui" => cmd::ui(&args),
        "builtins" => cmd::builtins(json_out),
        "render" => cmd::render(&args),
        "pack" => cmd::pack(&args),
        "unpack" => cmd::unpack(&args),
        "interface" => cmd::interface(&args),
        "ast" => cmd::ast(&args),
        "view" => cmd::view(&args),
        "check" => cmd::check(&args, json_out),
        "holes" => cmd::holes(&args, json_out),
        "verify" => cmd::verify(&args, json_out),
        "run" => cmd::run_program(&args),
        "serve" => cmd::serve_program(&args),
        "test" => cmd::run_tests(&args),
        "build" => cmd::build(&args),
        other => Err(format!("unknown command `{other}` (try `strata help`)")),
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
