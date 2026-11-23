mod ast;
mod fx;
mod par;
mod symbol;
#[cfg(test)]
mod bench;
mod ast_value;
mod binary;
mod cli;
mod cmd;
mod desugar;
mod core;
mod derive;
mod diff;
mod history;
mod git;
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
use crate::fx::HashMap;
use std::io::Read;
use std::process::{Command, ExitCode};
use store::Codebase;

const USAGE: &str = "strata — an experimental language for working with agents

USAGE: strata [--file F.strata] <command> [args]

Code is stored as its syntax tree: a .strata file is a binary module file
(docs/binary-format.md). Text goes in only through an edit (`edit`, `add`), and
comes out through the views (`render`, `view`).

Codebase (the package around the current folder: its files, and strata.log, the log of
every change made through strata, committed with them):
  init                         make the current folder a package (a strata.toml named
                               after the folder)
  add <FILE> [MODULE] [-m MSG] import definitions from a file (checked, atomic): text, or
                               a .strata file or folder
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
                               print a module as text (L2): the file that holds it (the
                               root module is `main`); --split writes one file per module
                               (as text files: `shop/orders.st`, never read back)
  pack <FILE.strata>           the whole program in one file (a bundle, with strata.toml);
                               --file, run, ui and every view read it like a package;
                               --prelude: the prelude's definitions instead (for compiler/eval)
  header <FILE.strata>         a module file's table of contents: each definition's kind,
                               name, hashes, size and dependencies, read without decoding it;
                               --tree: each definition (a bundle's too) as a `compiler/ast` value, one per line
                               --text: the definitions as the printer writes them, as stored
  lex <FILE> [--rust]          a text file's tokens, one per line (then its comments), by
                               the lexer written in Strata (`compiler/lex`); --rust: by the
                               Rust lexer (the reference `compiler/lex` is tested against)
  parse <FILE> [--rust]        a text file parsed as a script: each definition as a `compiler/ast`
                               value, one per line, as parsed (not stored), by `compiler/parse`;
                               --stored: as stored (references qualified), by `compiler/resolve`;
                               --rust: by the Rust parser
  (`--file` accepts a folder: each .strata file is a module named after its path.
   `--file F edit [MODULE]` and `--file F add TEXT` change F outside any log: a script,
   or a folder's module, created if new; only what doesn't parse is refused)

Packages: a folder with a strata.toml (`name = \"...\"`, optional `exports = [...]`);
its .strata files are modules named after their path, main.strata is the entry point.
A .strata file outside every package is a script.

Git:
  merge-file O A B [P]         git merge driver: merge each definition separately, then
                               type-check; writes the result to A; a conflict leaves A as
                               text with conflict markers
  resolve <FILE.strata>        a file a merge conflict left as text, fixed by hand: store
                               it as a module file again
  setup-git                    use merge-file for `git merge` / `git pull` in this clone,
                               and show .strata files as text in `git diff` and `git log -p`
  sync [BRANCH]                merge BRANCH (default main; origin's if there is a remote)
                               into this branch with the driver, then check; commits the
                               merge only if every package and script checks
  upgrade                      rewrite every package and script in the repository in the
                               current binary form (after a change to the format)

Views:
  ui [--port 7777]                the workspace in the browser: agents over ACP, the code
                                  with its git activity, PRs (through gh)
  ui --strata                     the viewer written in Strata (std.web, std.code): code view only for now
  view [SELECTOR] [--level 0-2] [--mocks]
                                  SELECTOR: name | prefix* | slice:Name | a,b (mocks
                                  only with --mocks or by name)
  ast <name>                      the stored syntax tree as JSON

Checking:
  check [--json]               types, exhaustiveness, literal refinements
  holes [--json]               typed holes with goal type, context and obligations
  verify [SELECTOR] [--cases N] [--checks] [--smt] [--obligations] [--require R,..] [--no-race] [--no-cache] [--json]
                               prove every contract with an SMT solver (z3), confirm
                               its counterexamples by running them, fuzz the rest.
                               A check's verdict is kept (in the git repository, shared
                               by its worktrees, else ~/.cache/strata/verdicts) under
                               a hash of its definition and all it depends on, the
                               flags, solver and strata binary: an unchanged check
                               says (cached) (--no-cache: run all). --checks lists the
                               proven checks, --smt prints the solver queries,
                               --obligations each obligation with the solver's
                               answer (proven, refuted, unknown), nothing run.
                               Each function also says whether it terminates.
                               --require (proven, terminates, no-recursion) fails
                               the run when a checked function falls short (as do
                               strata.toml's [verify] rules). Proven functions that
                               terminate are still run on --cross-check N cases
                               (default 20, 0: off), to catch a wrong proof.
                               The solver and the fuzzer race: the first to find a
                               failure stops the other (--no-race: the solver
                               first, then the fuzzer; every result the same from
                               run to run, and a failure of proven code is shown
                               as a bug in the prover).
                               Everything run is compiled (--jit: say why what
                               can't be compiled yet is skipped)

Running:
  run [PATH] [-- ARGS..]       run the program: `main` gets the capabilities it asks for
                               (Console; Disk, Process from std; its own) and ARGS; it
                               returns (), an ExitCode, or a Result (Err: exit code 1).
                               PATH is a .strata file or a package folder (or --file).
                               It is compiled to machine code in memory (LLVM); what
                               can't be compiled yet is an error saying why.
                               --trace prints each call and return to stderr;
                               the checks the last `verify` proved are left out
                               (--all-checks keeps them; `build` too)
  serve [PATH] [--port N]      serve the web: each request is passed to the program's
                               `fn handle(req: Request) -> Response` (std.http), which may
                               also ask for Console and capabilities (default port 8000);
                               or run it as an app: `init`, `update` and `view` (std.web)
  run <fn> [ARGS] [--trace]
                               call one function: ARGS are its arguments written as
                               code (`\"abc\", 3`, `Circle { radius: 2 }`), checked
                               against the types; the result is printed as code;
                               --trace prints each call and return
                               to stderr
  test [FILTER] [--with-deps]  run the project's own test blocks, compiled (a test that
                               can't be compiled yet fails, saying why); --with-deps: also
                               those of what it imports (std, dependencies), which are
                               otherwise tested on their own
  build [PATH] [-o OUT] [--emit-llvm] [--all-checks] [--no-verify]
                               compile the program to a native executable, through LLVM
                               (needs clang; STRATA_CC chooses another): `main` takes no
                               parameters yet, and only part of the language compiles
                               (Int, Float, Bool, String, functions, control flow,
                               contracts). --emit-llvm writes the LLVM IR instead
  builtins [--json]            list built-in functions and their signatures

With --file, commands operate on a file or folder instead of a codebase.";

struct Args {
    pos: Vec<String>,
    flags: HashMap<String, Vec<String>>,
}

impl Args {
    fn parse() -> Args {
        let raw: Vec<String> = std::env::args().skip(1).collect();
        let valued = ["-m", "--level", "--cases", "--file", "--rm", "--seed", "--port", "--split", "--members", "--from", "--to", "--step", "--base", "--require", "--cross-check", "-o"];
        let (mut pos, mut flags) = (Vec::new(), HashMap::<String, Vec<String>>::default());
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

/// Read a program: a binary file (a module file, or a bundle), or a folder
/// where each `.strata` file is a module named after its path
/// (`shop/orders.strata` -> `shop.orders`, `main.strata` -> the root module).
/// Text is never read here: it goes in through an edit.
fn read_sources(path: &str) -> Result<Program, String> {
    package::program(std::path::Path::new(path))
}

fn load(args: &Args) -> Result<Source, String> {
    // commands that write code load a package whose settings may name what they are about to write
    let writes = matches!(args.pos.first().map(|s| s.as_str()), Some("add" | "edit" | "rename" | "move" | "status" | "done" | "abort"));
    if let Some(f) = args.val("--file") {
        let p = read_sources(f)?;
        if !writes {
            package::check_settings(std::path::Path::new(f), &p)?;
        }
        return Ok(Source::File(p));
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = Codebase::find(&cwd).ok_or("no package here (no strata.toml in this folder or above: run `strata init`, or pass --file)")?;
    let cb = Codebase::open(&root)?;
    if !writes {
        package::check_settings(&root, &cb.program)?;
    }
    Ok(Source::Db(cb))
}

/// Full static check: types, exhaustiveness, then literal refinements.
fn full_check(p: &Program) -> types::CheckReport {
    full_checked(p).report
}

/// `full_check`, keeping the elaborated program and its environment, which
/// running, testing and verifying work from: the program is resolved and
/// type-checked once.
pub fn full_checked(p: &Program) -> types::Checked {
    let mut c = types::check_and_elaborate(p);
    for d in verify::refinements_in(&c.program, &c.compiled, &c.env) {
        if !c.report.errors.iter().any(|e| e.decl == d.decl && e.msg == d.msg) {
            c.report.errors.push(d);
        }
    }
    c
}

fn print_errors(rep: &types::CheckReport) {
    for e in &rep.errors {
        eprintln!("error in {}: {}", e.decl, e.msg);
    }
}

/// Where `verify` keeps what it proved of `src` for `build` (a script's
/// by its path `file`).
fn proofs_dir(src: &Source, file: Option<&str>) -> Option<std::path::PathBuf> {
    match src {
        Source::Db(cb) => Some(cb.proofs_dir()),
        Source::File(_) => store::script_proofs_dir(file?),
    }
}

/// What `build` and `run` leave out of `src`'s compiled code: the checks the
/// last `verify` proved, for code unchanged since (none with `--all-checks`).
fn proofs_for(src: &Source, file: Option<&str>, args: &Args) -> HashMap<String, llvm::Proofs> {
    if args.flag("--all-checks") {
        return HashMap::default();
    }
    proofs_dir(src, file).map(|d| store::current_proofs(src.program(), &d)).unwrap_or_default()
}

/// `verify`'s verdicts for `src` (a package, or the script or folder `file`).
fn verdicts_at(src: &Source, file: Option<&str>) -> store::Verdicts {
    match (src, file) {
        (Source::Db(cb), _) => cb.load_verdicts(),
        (Source::File(_), Some(f)) => store::Verdicts::at(std::path::Path::new(f)),
        _ => store::Verdicts::none(),
    }
}

fn verdicts_for(src: &Source) -> HashMap<String, String> {
    let Source::Db(cb) = src else { return HashMap::default() };
    let cache = cb.load_verdicts();
    let closures = store::Closures::new(&cb.program);
    cb.program
        .decls
        .iter()
        .filter_map(|d| cache.get(&closures.hash(&d.qname())).map(|v| (d.qname(), v.tag())))
        .collect()
}

fn run() -> Result<bool, String> {
    let args = Args::parse();
    let Some(cmd) = args.pos.first().map(|s| s.as_str()) else {
        println!("{USAGE}");
        return Ok(true);
    };
    let json_out = args.flag("--json");
    // `lex` and `parse` are the Strata front end's (`compiler/`) unless
    // `--rust`, or when no compiler package is found
    let strata_default = matches!(cmd, "lex" | "parse") && !args.flag("--rust") && cmd::compare::available();
    if strata_default || matches!(cmd, "lex" | "parse" | "render" | "run" | "build") && (args.flag("--strata") || args.flag("--compare")) {
        return cmd::compare::command(&args, cmd);
    }
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
        "resolve" => cmd::resolve(&args),
        "setup-git" => cmd::setup_git(),
        "sync" => cmd::sync(&args),
        "upgrade" => cmd::upgrade(),
        "log" => cmd::log(&args, json_out),
        "ui" => cmd::ui(&args),
        "builtins" => cmd::builtins(json_out),
        "render" => cmd::render(&args),
        "pack" => cmd::pack(&args),
        "header" => cmd::header(&args),
        "lex" => cmd::lex(&args),
        "parse" => cmd::parse(&args),
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
    // compiled code (the JIT, compile-time code) runs on this thread: a deep
    // but legal recursion goes until the memory ends, not the default stack
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
