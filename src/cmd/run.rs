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
    let types::Checked { report: rep, env, compiled, .. } = full_checked(p);
    if !rep.ok() {
        print_errors(&rep);
        return Ok(false);
    }
    let Some(name) = rest.first() else {
        let program_args = args.flags.get("--").cloned().unwrap_or_default();
        // compiled in memory; `--interpret` runs the old interpreter (kept
        // until it is removed, to compare with)
        if args.flag("--interpret") {
            std::process::exit(run_main(p, &env, &prog, program_args, args.flag("--trace"))?);
        }
        let Some(main) = env.fns.get(&Symbol::new("main")) else {
            return Err("no `main` function to run (the entry point is `fn main` in main.strata, or in the file you run)".into());
        };
        // what `main` may ask for: a mistake of the program's, not something not compiled yet
        for pm in &main.params {
            let ty = printer::ty(&pm.ty);
            if capability(&env, &ty).is_none() && !cli::is_options(&env, &pm.ty) {
                return Err(format!("{} Its parameter `{}` is a `{ty}`", not_given(&ty, "`main`"), pm.name));
            }
        }
        let ir = if args.flag("--trace") { llvm::compile_traced(p, "main") } else { llvm::compile(p, "main") };
        let ir = ir.map_err(|e| format!("the program can't be compiled yet: {e}"))?;
        std::process::exit(llvm::jit::run(&ir, &prog, &program_args)?);
    };
    if !args.flag("--interpret") {
        return call_compiled(args, &compiled, &env, name, rest.get(1).map(|s| s.as_str()).unwrap_or("[]"));
    }
    let f = env.fns.get(&name.sym()).ok_or(format!("no function `{name}`"))?.clone();
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
    Ok(true)
}

/// `strata run f [JSON-ARGS]`, compiled: the arguments (read against the
/// parameters' types) written into a function that calls `f`, whose result
/// is read back and printed as JSON; a fault as `{"fault": ..}`.
fn call_compiled(args: &Args, resolved: &Program, env: &types::Env, name: &str, raw: &str) -> Result<bool, String> {
    let f = env.fns.get(&name.sym()).ok_or(format!("no function `{name}`"))?.clone();
    if !f.type_params.is_empty() {
        return Err(format!("`{name}` is generic: call it from a function that says which types it is used at"));
    }
    let raw: serde_json::Value = serde_json::from_str(raw).map_err(|e| format!("args: {e}"))?;
    let raw = raw.as_array().cloned().ok_or("args must be a JSON array")?;
    if raw.len() != f.params.len() {
        return Err(format!("`{name}` takes {} arguments", f.params.len()));
    }
    // JSON read against the types (their shapes; refinements are checked compiled)
    let mut reader = interp::Interp::new(env, 1);
    let mut stmts = vec![];
    let mut call_args = vec![];
    for (i, (pm, j)) in f.params.iter().zip(&raw).enumerate() {
        let v = reader.from_json_raw(&pm.ty, j, &pm.name).map_err(|e| format!("invalid input: {e}"))?;
        let x = verify::literal_of(&v).ok_or(format!("invalid input: `{}` can't be given as JSON", pm.name))?;
        let n = Symbol::new(&format!("strata_arg_{i}"));
        if pm.mutable {
            stmts.push(Stmt::Var(n, Some(pm.ty.clone()), x));
            call_args.push(Expr::Mut(Box::new(Expr::Var(n))));
        } else {
            // (its refinements are checked by `f`, which names the argument)
            stmts.push(Stmt::Let(Pattern::Bind(n), None, x));
            call_args.push(Expr::Var(n));
        }
    }
    let probe = "strata_run_call";
    let call = FnDecl {
        doc: vec![],
        comments: vec![],
        name: Symbol::new(probe),
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
    let mut prog = resolved.clone();
    prog.decls.push(Decl { comments: vec![], doc: vec![], kind: DeclKind::Fn(call), module: Default::default(), public: false });
    let ir = if args.flag("--trace") {
        llvm::compile_traced_comptime(&prog, &[probe.to_string()])
    } else {
        llvm::compile_comptime(&prog, &[probe.to_string()])
    };
    let (ir, roots, shapes) = ir.map_err(|e| format!("`{name}` can't be compiled yet: {e}"))?;
    let (result, printed) = llvm::jit::run_kept(&ir, &roots, &shapes, i64::MAX, true)?.pop().ok_or("nothing ran")?;
    print!("{printed}");
    match result {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&interp::to_json(&v)).unwrap());
            Ok(true)
        }
        Err(fault) => {
            let stack: Vec<&str> = fault.stack.split(" -> ").filter(|s| !s.is_empty() && *s != probe).collect();
            // an argument that breaks its type's refinement (checked by `f` on entry)
            let arg = fault.message.strip_prefix("argument `").and_then(|m| m.split_once(&format!("` of `{name}`: ")));
            if fault.kind == "Refinement" && stack.len() <= 1 {
                if let Some((param, rest)) = arg {
                    return Err(format!("invalid input: {param}: {rest}"));
                }
                if stack.is_empty() {
                    return Err(format!("invalid input: {}", fault.message.strip_prefix("let binding: ").unwrap_or(&fault.message)));
                }
            }
            println!("{}", serde_json::to_string_pretty(&json!({"fault": {"kind": fault.kind, "msg": fault.message, "stack": stack}})).unwrap());
            Ok(false)
        }
    }
}

/// `strata test`: each `test` block, compiled when this strata has the JIT
/// (unless `--interpret`; `--jit` says why when it can't), else interpreted;
/// the same report either way.
pub fn run_tests(args: &Args) -> Result<bool, String> {
    let src = load(&args)?;
    let p = src.program();
    let types::Checked { report: rep, program: resolved, env, .. } = full_checked(p);
    if !rep.ok() {
        print_errors(&rep);
        return Ok(false);
    }
    let filter = args.pos.get(1).cloned().unwrap_or_default();
    if args.flag("--emit-llvm") {
        // the tests' IR (`strata.test.N`, in order)
        print!("{}", llvm::compile_tests(p)?.0);
        return Ok(true);
    }
    let outcomes = if args.flag("--interpret") { test_interpreted(&resolved, &env, &filter) } else { test_jit(p, &filter)? };
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

fn test_interpreted(resolved: &Program, env: &types::Env, filter: &str) -> Vec<TestRun> {
    let mut out = vec![];
    for d in &resolved.decls {
        let DeclKind::Test(t) = &d.kind else { continue };
        if !t.name.contains(filter) {
            continue;
        }
        let mut it = interp::Interp::new(env, 7);
        // each test prints into its own buffer, shown only if it fails
        it.output = interp::Output::Buffer(String::new());
        // the capabilities it asks for, made as for `main`
        let mut scope = None;
        for p in &t.params {
            if let Some(v) = capability(env, &printer::ty(&p.ty)) {
                scope = interp::extend(&scope, &p.name, v);
            }
        }
        let fault = it.eval(&t.body, &scope).err().map(|f| (format!("{:?}", f.kind), f.msg, crate::interp::show_stack(&f.stack)));
        let printed = match &it.output {
            interp::Output::Buffer(b) => b.clone(),
            _ => String::new(),
        };
        out.push((t.name.to_string(), fault, printed));
    }
    out
}

/// The tests compiled and run in this process. A test that can't be
/// compiled yet fails, saying why (the others still run).
fn test_jit(p: &Program, filter: &str) -> Result<Vec<TestRun>, String> {
    let all: Vec<String> = p.decls.iter().filter_map(|d| match &d.kind {
        DeclKind::Test(t) => Some(t.name.to_string()),
        _ => None,
    }).collect();
    let mut skipped: Vec<(String, String)> = vec![];
    let (ir, names) = match llvm::compile_tests(p) {
        Ok(x) => x,
        Err(_) => {
            // each alone, to leave out only those that don't compile
            for t in &all {
                let others: Vec<String> = all.iter().filter(|o| *o != t).cloned().collect();
                if let Err(e) = llvm::compile_tests_except(p, &others) {
                    skipped.push((t.clone(), e));
                }
            }
            let skip: Vec<String> = skipped.iter().map(|(t, _)| t.clone()).collect();
            llvm::compile_tests_except(p, &skip)?
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

/// What the runtime gives for a parameter of `main` (or `handle`, `init`,
/// `update`, or a test), by its type: a capability (`Disk`, `Process`, the
/// program's own), which holds nothing; or, for `Console`, the one capability
/// the environment chooses: the terminal (a test's output is its own).
pub fn capability(env: &types::Env, ty: &str) -> Option<interp::Value> {
    use interp::Value;
    let made = if ty == CONSOLE { "std.io.Terminal" } else { ty };
    env.capabilities.contains(&Symbol::new(made)).then(|| Value::Record(made.into(), Default::default()))
}

/// The capability trait the environment fills in.
pub const CONSOLE: &str = "std.io.Console";

/// Why `main` can't ask for `ty`, with what to ask for instead.
pub fn not_given(ty: &str, what: &str) -> String {
    let instead = match ty {
        "std.fs.Files" => " Name the capability instead, `disk: Disk` (std.fs), and pass it where `Files` is wanted.",
        "std.process.Environment" => " Name the capability instead, `process: Process` (std.process), and pass it where `Environment` is wanted.",
        _ => "",
    };
    format!("{what} can ask for `Console` (std.io), capabilities (`Disk` from std.fs, `Process` from std.process, or the program's own `capability ...`), and options: a record that implements `CommandLine` (std.cli).{instead}")
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
    crate::serve::serve(&env, port, &name)?;
    Ok(true)
}

/// Run the program's `main` with the real system, and give its exit code.
/// `main` asks for the capabilities it needs by type (`Console`, `Files`,
/// `Environment`, in any order) and may return `()` (0), an exit code, or a
/// `Result` of either, whose `Err` is printed to standard error (exit code 1).
pub fn run_main(_p: &Program, env: &types::Env, prog: &str, program_args: Vec<String>, trace: bool) -> Result<i32, String> {
    use interp::Value;
    let f = env.fns.get(&Symbol::new("main")).ok_or("no `main` function to run (the entry point is `fn main` in main.strata, or in the file you run)")?.clone();
    let mut it = interp::Interp::new(env, 1);
    it.tracing = trace;
    it.host = Some(interp::Host { args: program_args.clone() });
    it.output = interp::Output::Terminal;
    let mut vals = Vec::new();
    for pm in &f.params {
        let ty = printer::ty(&pm.ty);
        vals.push(match ty.as_str() {
            _ if capability(env, &ty).is_some() => capability(env, &ty).unwrap(),
            // options read from the command line
            // `Options.from_args(program, args)`, derived in Strata (std.cli)
            _ if cli::is_options(env, &pm.ty) => {
                let args = Value::List(program_args.iter().map(|a| Value::Str(a.clone())).collect());
                // the method's name as resolved (`std.cli.from_args`)
                let m = env.traits.get(&Symbol::new(cli::TRAIT)).and_then(|t| t.methods.iter().find(|m| crate::names::short(&m.decl.name) == "from_args")).map(|m| m.decl.name.to_string()).ok_or("std.cli has no `from_args`")?;
                let call = types::trait_call(cli::TRAIT, &m);
                let r = it.call_method(&call, vec![Value::Str(ty.clone()), Value::Str(prog.to_string()), args]).map_err(|f| format!("reading the options: {}", f.msg))?;
                let field = |fs: &[(String, Value)], k: &str| fs.iter().find(|(n, _)| n == k).map(|(_, v)| interp::show(v)).unwrap_or_default();
                match r {
                    Value::Variant(c, interp::Fields::Pos(xs)) if c == "Ok" && xs.len() == 1 => xs[0].clone(),
                    Value::Variant(c, interp::Fields::Pos(xs)) if c == "Err" => match xs.first() {
                        Some(Value::Variant(e, interp::Fields::Rec(fs))) if e.ends_with("ShowHelp") => {
                            print!("{}", field(fs, "text"));
                            return Ok(0);
                        }
                        Some(Value::Variant(_, interp::Fields::Rec(fs))) => {
                            eprintln!("error: {}", field(fs, "message"));
                            return Ok(2);
                        }
                        _ => return Err("reading the options gave an unexpected error".into()),
                    },
                    other => return Err(format!("reading the options gave {}", interp::show(&other))),
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
        Ok(Value::Variant(c, interp::Fields::Pos(xs))) if c == "Ok" && xs.len() == 1 => code(&xs[0]),
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
                eprintln!("    stack: {}", crate::interp::show_stack(&fault.stack));
            }
            Ok(101)
        }
    }
}
