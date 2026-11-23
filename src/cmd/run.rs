//! Commands that run the code: run, test.

use crate::*;

/// `strata run`.
pub fn run_program(args: &Args) -> Result<bool, String> {
    // `strata run hello.strata`: the program's path in place of --file
    let (src, rest, path) = match args.pos.get(1) {
        Some(f) if f.ends_with(".strata") || std::path::Path::new(f).join(package::FILE).is_file() => (Source::File(read_sources(f)?), &args.pos[2..], f.clone()),
        _ => (load(&args)?, &args.pos[1..], args.val("--file").unwrap_or(".").to_string()),
    };
    // the program's name, for `--help`: its package's, or the file's
    let path = std::path::Path::new(&path);
    let prog = match package::find(path) {
        Ok(Some(pkg)) => pkg.name,
        _ => path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "main".into()),
    };
    let p = src.program();
    let types::Checked { report: rep, env, .. } = full_checked(p);
    if !rep.ok() {
        print_errors(&rep);
        return Ok(false);
    }
    let Some(name) = rest.first() else {
        let program_args = args.flags.get("--").cloned().unwrap_or_default();
        // compiled in memory
        let Some(main) = env.fns.get(&Symbol::new("main")) else {
            return Err("no `main` function to run (the entry point is `fn main` in main.strata, or in the file you run)".into());
        };
        // what `main` may ask for: a mistake of the program's, not something not compiled yet
        for pm in &main.params {
            let ty = printer::ty(&pm.ty);
            if !capability(&env, &ty) && !cli::is_options(&env, &pm.ty) {
                return Err(format!("{} Its parameter `{}` is a `{ty}`", not_given(&ty, "`main`"), pm.name));
            }
        }
        // the checks the last `verify` proved are left out (unless --all-checks)
        let file = if matches!(src, Source::File(_)) && args.val("--file").is_none() { Some(path.to_string_lossy().to_string()) } else { args.val("--file").map(String::from) };
        let proofs = proofs_for(&src, file.as_deref(), args);
        // compiled by compiler/codegen and run by the compiler's JIT (unless
        // `--rust` or `STRATA_BACKEND=rust`); what it doesn't cover yet is
        // `llvm.rs`'s
        if let Some(at) = jit_path(args, file.as_deref()) {
            let facts = if at.is_dir() { String::new() } else { llvm::proofs_text(p, &proofs) };
            match cmd::compare::jit_run(&at, &prog, &facts, args.flag("--trace"), None, &program_args) {
                Ok(code) => std::process::exit(code),
                Err(e) if e.contains("doesn't cover") => note_fallback(&e),
                Err(e) => return Err(e),
            }
        }
        let ir = llvm::compile_proven(p, "main", proofs, args.flag("--trace")).map(|r| r.0);
        let ir = ir.map_err(|e| format!("the program can't be compiled yet: {e}"))?;
        std::process::exit(llvm::jit::run(&ir, &prog, &program_args)?);
    };
    let file = if matches!(src, Source::File(_)) && args.val("--file").is_none() { Some(path.to_string_lossy().to_string()) } else { args.val("--file").map(String::from) };
    call(args, p, &env, name, rest.get(1).map(|s| s.as_str()).unwrap_or(""), jit_path(args, file.as_deref()))
}

/// Where the program `strata run` runs is, when the compiler's JIT runs it
/// (compiler/codegen's code): a script or package found on disk, not the
/// compiler package itself, without `--rust` or `STRATA_BACKEND=rust`.
fn jit_path(args: &Args, file: Option<&str>) -> Option<std::path::PathBuf> {
    if args.flag("--rust") || cmd::compare::rust_backend() || !cmd::compare::available() {
        return None;
    }
    let at = std::path::PathBuf::from(file.unwrap_or("."));
    (at.exists() && !cmd::compare::is_compiler(&at)).then_some(at)
}

/// What `strata run` says when compiler/codegen doesn't cover the program
/// yet, before `llvm.rs` runs it.
fn note_fallback(e: &str) {
    eprintln!("note: {}; using llvm.rs", e.lines().next().unwrap_or_default().trim_end_matches(':'));
}

/// The function `strata run f ARGS` runs: `f` called on the arguments, as
/// they are written (Strata code: `"abc", 3`, `Circle { radius: 2 }`).
const PROBE: &str = "strata_run_call";

/// `strata run f ARGS`: the arguments, written as code, in a function that
/// calls `f`, checked with the program (so a wrong argument is an error
/// before anything runs), then run, compiled; the result printed as the
/// code that writes it.
fn call(args: &Args, p: &Program, env: &types::Env, name: &str, raw: &str, jit: Option<std::path::PathBuf>) -> Result<bool, String> {
    let f = env.fns.get(&name.sym()).ok_or(format!("no function `{name}`"))?.clone();
    if !f.type_params.is_empty() {
        return Err(format!("`{name}` is generic: call it from a function that says which types it is used at"));
    }
    let Expr::Call(_, given) = crate::parser::parse_expr(&format!("f({raw})")).map_err(|e| format!("the arguments: {e}"))? else {
        return Err("the arguments: expected values separated by commas".into());
    };
    if given.len() != f.params.len() {
        return Err(format!("`{name}` takes {} arguments", f.params.len()));
    }
    let mut stmts = vec![];
    let mut call_args = vec![];
    for (i, (pm, x)) in f.params.iter().zip(given).enumerate() {
        let n = Symbol::new(&format!("strata_arg_{i}"));
        if pm.mutable {
            stmts.push(Stmt::Var(n, Some(pm.ty.clone()), x));
            call_args.push(Expr::Mut(Box::new(Expr::Var(n))));
        } else {
            // (its type and refinements are checked where `f` takes it, which names the
            // argument; a sized integer's type is given, so a literal is one)
            let sized = matches!(&pm.ty, TypeExpr::Named(t, a) if a.is_empty() && types::Width::of(t.as_str()).is_some());
            stmts.push(Stmt::Let(Pattern::Bind(n), sized.then(|| pm.ty.clone()), x));
            call_args.push(Expr::Var(n));
        }
    }
    let probe = FnDecl {
        doc: vec![],
        comments: vec![],
        name: Symbol::new(PROBE),
        type_params: vec![],
        bounds: vec![],
        params: vec![],
        ret: f.ret.clone(),
        requires: vec![],
        ensures: vec![],
        terminates: false,
        body: Expr::Block(stmts, Box::new(Expr::Call(Box::new(Expr::Var(Symbol::new(name))), call_args))),
        comptime: false,
        derived: false,
    };
    // in the function's own module (it may be private there), the arguments
    // written as that module writes values
    let module = p
        .decls
        .iter()
        .find(|d| matches!(&d.kind, DeclKind::Fn(g) if crate::names::qualified(d.module, g.name).as_str() == name))
        .map(|d| d.module)
        .unwrap_or_default();
    let mut probe = probe;
    if let Expr::Block(_, call) = &mut probe.body {
        if let Expr::Call(f, _) = &mut **call {
            **f = Expr::Var(Symbol::new(crate::names::short(name)));
        }
    }
    let mut with = p.clone();
    with.decls.push(Decl { comments: vec![], doc: vec![], kind: DeclKind::Fn(probe), module, public: false });
    let probe_name = crate::names::qualified(module, Symbol::new(PROBE)).to_string();
    let checked = full_checked(&with);
    if let Some(e) = checked.report.errors.iter().find(|e| e.decl == probe_name) {
        return Err(format!("invalid input: {}", e.msg));
    }
    // compiled by compiler/codegen and run by the compiler's JIT: the probe
    // again, its result's `debug` text (the value as code) as its own, in a
    // module file of its own (stored: references qualified) for `codegen
    // --call` to add to the program
    if let Some(at) = jit {
        let mut shown = with.clone();
        if let Some(Decl { kind: DeclKind::Fn(f), .. }) = shown.decls.last_mut() {
            f.ret = TypeExpr::Named(Symbol::new("String"), vec![]);
            if let Expr::Block(_, last) = &mut f.body {
                let made = std::mem::replace(&mut **last, Expr::Unit);
                **last = Expr::Call(Box::new(Expr::Var(Symbol::new("debug"))), vec![made]);
            }
        }
        let stored = crate::resolve::stored_form(&shown);
        let probe = Program { decls: stored.decls.last().cloned().into_iter().collect() };
        let call = cmd::compare::Call { module_file: crate::binary::encode_module(&probe, module.as_str(), &Default::default()), fun: name.to_string() };
        match cmd::compare::jit_run(&at, "main", "", args.flag("--trace"), Some(&call), &[]) {
            Ok(0) => return Ok(true),
            // (`error: invalid input: …`, said by the JIT)
            Ok(2) => std::process::exit(2),
            Ok(_) => return Ok(false),
            Err(e) if e.contains("doesn't cover") => note_fallback(&e),
            Err(e) => return Err(e),
        }
    }
    let ir = if args.flag("--trace") {
        llvm::compile_traced_comptime(&checked.compiled, &[probe_name.clone()])
    } else {
        llvm::compile_comptime(&checked.compiled, &[probe_name.clone()])
    };
    let (ir, roots, shapes) = ir.map_err(|e| format!("`{name}` can't be compiled yet: {e}"))?;
    let (result, printed) = llvm::jit::run_kept(&ir, &roots, &shapes, i64::MAX, llvm::jit::release())?.pop().ok_or("nothing ran")?;
    print!("{printed}");
    match result {
        Ok(v) => {
            println!("{}", printer::literal(&v));
            Ok(true)
        }
        Err(fault) => {
            let stack: Vec<&str> = fault.stack.split(" -> ").filter(|s| !s.is_empty() && *s != probe_name).collect();
            // an argument that breaks its type's refinement (checked by `f` on entry)
            let arg = fault.message.strip_prefix("argument `").and_then(|m| m.split_once(&format!("` of `{name}`: ")));
            if fault.kind == "Refinement" && stack.len() <= 1 {
                if let Some((param, rest)) = arg {
                    return Err(format!("invalid input: {param}: {rest}"));
                }
            }
            eprintln!("fault ({}): {}", fault.kind, fault.message);
            if !stack.is_empty() {
                eprintln!("    stack: {}", stack.join(" -> "));
            }
            Ok(false)
        }
    }
}

/// `strata test`: each `test` block, compiled; a test that can't be
/// compiled yet fails, saying why.
pub fn run_tests(args: &Args) -> Result<bool, String> {
    // `strata --file core test`: core's own tests (core.mem's), with core as
    // this strata is built with it
    let core_tests;
    let src;
    let p = match crate::core::dir_of(args.val("--file")) {
        Some(_) => {
            core_tests = crate::core::tests();
            &core_tests
        }
        None => {
            src = load(&args)?;
            src.program()
        }
    };
    let rep = full_checked(p).report;
    if !rep.ok() {
        print_errors(&rep);
        return Ok(false);
    }
    let filter = args.pos.get(1).cloned().unwrap_or_default();
    // the tests of the project's own files; `--with-deps`: also of what it imports
    let with_deps = args.flag("--with-deps");
    if args.flag("--emit-llvm") {
        // the tests' IR (`strata.test.N`, in order)
        print!("{}", llvm::compile_tests(p, with_deps)?.0);
        return Ok(true);
    }
    let outcomes = test_jit(p, &filter, with_deps)?;
    let (mut pass, mut fail) = (0, 0);
    for (name, fault, printed) in outcomes {
        match fault {
            None => {
                pass += 1;
                println!("✓ {name}");
            }
            Some((kind, msg, stack)) => {
                fail += 1;
                println!("✗ {name}\n    {kind}: {msg}");
                if !stack.is_empty() {
                    println!("    stack: {stack}");
                }
                if !printed.is_empty() {
                    println!("    printed:");
                    for l in printed.lines() {
                        println!("      {l}");
                    }
                }
            }
        }
    }
    println!("\n{pass} passed, {fail} failed");
    Ok(fail == 0)
}

/// A test's name, the fault that ended it (kind, message, call stack), and what it printed.
type TestRun = (String, Option<(String, String, String)>, String);

/// The tests compiled and run in this process. A test that can't be
/// compiled yet fails, saying why (the others still run).
fn test_jit(p: &Program, filter: &str, with_deps: bool) -> Result<Vec<TestRun>, String> {
    let all: Vec<String> = p.decls.iter().filter_map(|d| match &d.kind {
        DeclKind::Test(t) => Some(t.name.to_string()),
        _ => None,
    }).collect();
    let mut skipped: Vec<(String, String)> = vec![];
    let (ir, names) = match llvm::compile_tests(p, with_deps) {
        Ok(x) => x,
        Err(_) => {
            // each alone, to leave out only those that don't compile
            for t in &all {
                let others: Vec<String> = all.iter().filter(|o| *o != t).cloned().collect();
                if let Err(e) = llvm::compile_tests_except(p, &others, with_deps) {
                    skipped.push((t.clone(), e));
                }
            }
            let skip: Vec<String> = skipped.iter().map(|(t, _)| t.clone()).collect();
            llvm::compile_tests_except(p, &skip, with_deps)?
        }
    };
    let which: Vec<usize> = (0..names.len()).filter(|&i| names[i].contains(filter)).collect();
    let runs = llvm::jit::run_tests(&ir, &which)?;
    let mut out: Vec<TestRun> = which.iter().zip(runs).map(|(&i, r)| (names[i].clone(), r.fault, r.printed)).collect();
    for (t, e) in skipped {
        if t.contains(filter) {
            out.push((t, Some(("Unsupported".into(), format!("can't be compiled yet: {e}"), String::new())), String::new()));
        }
    }
    // in the program's order
    out.sort_by_key(|(n, _, _)| all.iter().position(|a| a == n));
    Ok(out)
}

/// Whether the runtime gives a parameter of `main` (or `handle`, `init`,
/// `update`, or a test) of this type: a capability (`Disk`, `Process`, the
/// program's own), which holds nothing; `Console`, the one capability the
/// environment chooses (the terminal; a test's output is its own); or the
/// `RunMode`, when the package lets programs ask for it.
pub fn capability(env: &types::Env, ty: &str) -> bool {
    if ty == RUN_MODE {
        return run_mode_allowed();
    }
    let made = if ty == CONSOLE { "std.io.Terminal" } else { ty };
    env.capabilities.contains(&Symbol::new(made))
}

/// The capability trait the environment fills in.
pub const CONSOLE: &str = "std.io.Console";

/// How the program is run (std.process): hidden unless strata.toml says
/// `mode = true` under `[run]`.
pub const RUN_MODE: &str = "std.process.RunMode";

/// Whether the package lets `main` and tests ask for its `RunMode`.
pub fn run_mode_allowed() -> bool {
    crate::package::setting("run.mode").as_deref() == Some("true")
}

/// Why `main` can't ask for `ty`, with what to ask for instead.
pub fn not_given(ty: &str, what: &str) -> String {
    let instead = match ty {
        RUN_MODE => " How the program is run is hidden by default, so it behaves the same everywhere: to let it ask, say `mode = true` under `[run]` in strata.toml.",
        "std.fs.Files" => " Name the capability instead, `disk: Disk` (std.fs), and pass it where `Files` is wanted.",
        "std.process.Environment" => " Name the capability instead, `process: Process` (std.process), and pass it where `Environment` is wanted.",
        "std.process.Commands" => " Name the capability instead, `mut launcher: Launcher` (std.process), and pass it where `Commands` is wanted.",
        _ => "",
    };
    format!("{what} can ask for `Console` (std.io), capabilities (`Disk` from std.fs, `Process` or `Launcher` from std.process, or the program's own `capability ...`), and options: a record that implements `CommandLine` (std.cli).{instead}")
}

/// `strata serve [PATH] [--port N]`: serve the program's `handle`, or run
/// it as an app (`init`, `update`, `view`).
pub fn serve_program(args: &Args) -> Result<bool, String> {
    // `strata serve app/`: the program's path in place of --file
    let (src, path) = match args.pos.get(1) {
        Some(f) => (Source::File(read_sources(f)?), f.clone()),
        None => (load(args)?, args.val("--file").unwrap_or(".").to_string()),
    };
    // the program's name, for an app's page title: its package's, or the file's
    let path = std::path::Path::new(&path);
    let name = match package::find(path) {
        Ok(Some(pkg)) => pkg.name,
        _ => path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "app".into()),
    };
    let port: u16 = args.val("--port").unwrap_or("8000").parse().map_err(|_| "bad --port")?;
    let p = src.program();
    let types::Checked { report: rep, env, .. } = full_checked(p);
    if !rep.ok() {
        print_errors(&rep);
        return Ok(false);
    }
    crate::serve::serve(p, &env, port, &name)?;
    Ok(true)
}
