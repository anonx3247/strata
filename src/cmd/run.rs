//! Commands that run the code: run, test.

use crate::*;

/// `strata run`.
pub fn run_program(args: &Args) -> Result<bool, String> {
    // `strata run hello.strata`: the program's path in place of --file
    let (src, rest, path) = match args.pos.get(1) {
        Some(f) if f.ends_with(".strata") || f.ends_with(".stb") || std::path::Path::new(f).join(package::FILE).is_file() => (Source::File(read_sources(f, None)?), &args.pos[2..], f.clone()),
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
    Ok(true)
}

/// `strata test`.
pub fn run_tests(args: &Args) -> Result<bool, String> {
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
    Ok(fail == 0)
}

/// Run the program's `main` with the real system, and give its exit code.
/// `main` asks for the capabilities it needs by type (`Console`, `Files`,
/// `Environment`, in any order) and may return `()` (0), an exit code, or a
/// `Result` of either, whose `Err` is printed to standard error (exit code 1).
pub fn run_main(p: &Program, env: &types::Env, prog: &str, program_args: Vec<String>, trace: bool) -> Result<i32, String> {
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
