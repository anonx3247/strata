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
        // compiled in memory when this strata has the JIT (unless asked not
        // to, or tracing); `--jit` insists, and says why when it can't
        let jit = args.flag("--jit") || (cfg!(feature = "jit") && !args.flag("--interpret") && !args.flag("--trace"));
        if jit && env.fns.contains_key(&Symbol::new("main")) {
            if let Some(code) = run_jit(p, &prog, &program_args, args.flag("--jit"))? {
                std::process::exit(code);
            }
        }
        std::process::exit(run_main(p, &env, &prog, program_args, args.flag("--trace"))?);
    };
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

/// `strata run` with the JIT: compile `main` and run it in this process;
/// `None` when the program uses what doesn't compile yet (it is then
/// interpreted, with a note saying why if `--jit` was asked for).
#[cfg(feature = "jit")]
fn run_jit(p: &Program, prog: &str, args: &[String], asked: bool) -> Result<Option<i32>, String> {
    match llvm::compile(p, "main") {
        Ok(ir) => llvm::jit::run(&ir, prog, args).map(Some),
        Err(e) => {
            if asked {
                eprintln!("note: interpreted, not compiled: {e}");
            }
            Ok(None)
        }
    }
}

#[cfg(not(feature = "jit"))]
fn run_jit(_: &Program, _: &str, _: &[String], _: bool) -> Result<Option<i32>, String> {
    Err("this strata was built without the JIT: build it with `cargo build --release --features jit` (needs LLVM 23)".into())
}

/// `strata test`: each `test` block, compiled when this strata has the JIT
/// (unless `--interpret`; `--jit` says why when it can't), else interpreted;
/// the same report either way.
pub fn run_tests(args: &Args) -> Result<bool, String> {
    let src = load(&args)?;
    let p = src.program();
    let types::Checked { report: rep, program: resolved, env } = full_checked(p);
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
    let jit = args.flag("--jit") || (cfg!(feature = "jit") && !args.flag("--interpret"));
    let outcomes = match jit.then(|| test_jit(p, &filter, args.flag("--jit"))).transpose()?.flatten() {
        Some(o) => o,
        None => test_interpreted(&resolved, &env, &filter),
    };
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
        let fault = it.eval(&t.body, &None).err().map(|f| (format!("{:?}", f.kind), f.msg, crate::interp::show_stack(&f.stack)));
        let printed = match &it.output {
            interp::Output::Buffer(b) => b.clone(),
            _ => String::new(),
        };
        out.push((t.name.to_string(), fault, printed));
    }
    out
}

/// The tests compiled and run in this process; `None` when they use what
/// doesn't compile yet (they are then interpreted, with a note if `--jit`).
#[cfg(feature = "jit")]
fn test_jit(p: &Program, filter: &str, asked: bool) -> Result<Option<Vec<TestRun>>, String> {
    let (ir, names) = match llvm::compile_tests(p) {
        Ok(x) => x,
        Err(e) => {
            if asked {
                eprintln!("note: interpreted, not compiled: {e}");
            }
            return Ok(None);
        }
    };
    let which: Vec<usize> = (0..names.len()).filter(|&i| names[i].contains(filter)).collect();
    let runs = llvm::jit::run_tests(&ir, &which)?;
    Ok(Some(which.iter().zip(runs).map(|(&i, r)| (names[i].clone(), r.fault, r.printed)).collect()))
}

#[cfg(not(feature = "jit"))]
fn test_jit(_: &Program, _: &str, _: bool) -> Result<Option<Vec<TestRun>>, String> {
    Err("this strata was built without the JIT: build it with `cargo build --release --features jit` (needs LLVM 23)".into())
}

/// The real implementation of a capability `main` (or `handle`) asks for:
/// `Console`, `Files` or `Environment`, by the parameter's type.
pub fn capability(ty: &str) -> Option<interp::Value> {
    use interp::Value;
    let rec = |name: &str, fields: Vec<(&str, Value)>| Value::Record(name.into(), fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
    Some(match ty {
        "std.io.Console" => rec("std.io.Terminal", vec![("stdout", Value::Int(1)), ("stderr", Value::Int(2))]),
        "std.fs.Files" => rec("std.fs.Disk", vec![("root", Value::Str(".".into()))]),
        "std.process.Environment" => rec("std.process.Process", vec![("pid", Value::Int(std::process::id() as i64))]),
        "std.code.Code" => rec("std.code.Workspace", vec![("root", Value::Str(crate::ui::code_root()))]),
        _ => return None,
    })
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
            _ if capability(&ty).is_some() => capability(&ty).unwrap(),
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
                    "`main` can ask for `Console` (std.io), `Files` (std.fs), `Environment` (std.process), `Code` (std.code), and options: a record that implements `CommandLine` (std.cli). Its parameter `{}` is a `{ty}`",
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
