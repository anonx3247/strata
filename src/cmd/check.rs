//! Commands that check the code: check, holes, verify.

use crate::*;

/// `strata check`.
pub fn check(args: &Args, json_out: bool) -> Result<bool, String> {
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
    Ok(rep.ok())
}

/// `strata holes`.
pub fn holes(args: &Args, json_out: bool) -> Result<bool, String> {
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
    Ok(true)
}

/// `strata verify`.
pub fn verify(args: &Args, json_out: bool) -> Result<bool, String> {
    let src = load(&args)?;
    let p = src.program();
    let types::Checked { report: rep, program: resolved, env, compiled } = full_checked(p);
    if !rep.ok() {
        print_errors(&rep);
        return Ok(false);
    }
    let cases: usize = args.val("--cases").unwrap_or("200").parse().map_err(|_| "bad --cases")?;
    let seed: u64 = args.val("--seed").unwrap_or("1").parse().map_err(|_| "bad --seed")?;
    let names = view::select(p, args.pos.get(1).map(|s| s.as_str()));
    let mut cache = match &src {
        Source::Db(cb) => cb.load_verdicts(),
        _ => HashMap::default(),
    };
    let mut all_ok = true;
    let mut results = Vec::new();
    // what is fuzzed, compiled, all in one module (`--jit` says why when something can't be)
    let chosen: Vec<bool> = p.decls.iter().map(|d| names.contains(&d.qname())).collect();
    jit_fuzzers(&compiled, &env, &chosen, args.flag("--jit"));
    let solver = smt::solver();
    let solver_bin = solver.as_ref();
    let analysis = termination::analyze(&env, solver_bin);
    let transparent = &analysis.transparent;
    let cross_check: usize = args.val("--cross-check").unwrap_or("20").parse().map_err(|_| "bad --cross-check")?;
    let require: Vec<String> = args.val("--require").map(|r| r.split(',').map(|s| s.trim().to_string()).collect()).unwrap_or_default();
    // the package's `[verify]` rules apply too
    let policy = package::find(std::path::Path::new(args.val("--file").unwrap_or("."))).ok().flatten().map(|p| p.verify).unwrap_or_default();
    let mut module_of: HashMap<String, String> = HashMap::default();
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
    for (i, (orig, d)) in p.decls.iter().zip(&resolved.decls).enumerate() {
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
            // a function with `comptime` parameters is a template for its
            // copies, which are what runs (and are checked where they are made)
            DeclKind::Fn(f) if f.params.iter().any(|p| p.comptime) => {}
            DeclKind::Fn(f) => {
                let n = cc(&f.name);
                jobs.push((f.name.to_string(), format!("{base}:{cases}:{n}:{skey}"), Box::new(move || verify::check_fn(env, f, None, &f.name, solver_bin, transparent, cases, n, seed))))
            }
            DeclKind::Impl(imp) if imp.is_inherent() => {
                // a type's own methods are functions (`Item.new`, `Item.describe`), checked as such
                for m in &imp.methods {
                    let short = crate::names::short(&m.name).to_string();
                    let f = types::static_fn(imp, m);
                    let n = cc(&f.name);
                    let key = format!("{base}:{short}:{cases}:{n}:{skey}");
                    jobs.push((f.name.to_string(), key, Box::new(move || verify::check_fn(env, &f, None, &f.name, solver_bin, transparent, cases, n, seed))));
                }
            }
            DeclKind::Impl(imp) => {
                let Some(tr) = env.traits.get(&imp.trait_name) else { continue };
                let ty = printer::ty(&imp.for_type);
                // a derived impl: the copies of the trait module's functions made
                // for this type (`std.json.encode_as.for.A.2`) are its code too
                if matches!(&orig.kind, DeclKind::Impl(i) if i.derived) {
                    let module = crate::names::module_of(&imp.trait_name).to_string();
                    let tag = format!(".for.{}.", crate::names::short(&types::texpr_head(&imp.for_type)));
                    for c in &resolved.decls[p.decls.len()..] {
                        let DeclKind::Fn(f) = &c.kind else { continue };
                        if f.name.as_str().contains(&tag) && f.name.as_str().starts_with(&format!("{module}.")) {
                            let n = cc(&f.name);
                            jobs.push((f.name.to_string(), format!("{base}:{}:{cases}:{n}:{skey}", f.name), Box::new(move || verify::check_fn(env, f, None, &f.name, solver_bin, transparent, cases, n, seed))));
                        }
                    }
                }
                for m in &tr.methods {
                    let mname = m.decl.name.clone();
                    let key = verify::wrapper_name(i, crate::names::short(&mname));
                    let decl = imp.methods.iter().find(|x| x.name == mname).or(if m.default { Some(&m.decl) } else { None });
                    match decl {
                        Some(decl) => {
                            let label = termination::impl_label(env, imp, &mname);
                            let n = cc(&label);
                            jobs.push((label, format!("{base}:{mname}:{cases}:{n}:{skey}"), Box::new(move || verify::check_fn(env, decl, Some(imp), &key, solver_bin, transparent, cases, n, seed))))
                        }
                        None => jobs.push((termination::impl_label(env, imp, &mname), format!("{base}:{mname}:{cases}"), Box::new(move || verify::fuzz_impl_method(imp, &mname, &key, cases, seed)))),
                    }
                }
                for law in &tr.laws {
                    let key = verify::wrapper_name(i, &format!("law.{}", law.name));
                    jobs.push((format!("law {} for {ty}", law.name), format!("{base}:law:{}:{cases}", law.name), Box::new(move || verify::check_law(imp, law, &key, cases, seed))));
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
            module_of.insert(label.clone(), orig.module.to_string());
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
        let out: Vec<_> = results.iter().map(|(n, v, c)| json!({"fn": n.strip_prefix(crate::resolve::ROOT_FN).unwrap_or(n), "cached": c, "verdict": v})).collect();
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        return Ok(all_ok);
    }
    for (n, v, cached) in &results {
        // (a root function named like a builtin is `main$get` once resolved)
        let n = &n.strip_prefix(crate::resolve::ROOT_FN).unwrap_or(n).to_string();
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
    Ok(all_ok)
}

/// Compile a fuzz driver for each function `verify` may fuzz (not what
/// takes a function), and for each wrapper of a trait method or law
/// (`verify::wrappers`), all in one module; what can't be compiled is left
/// out, and says why when it is checked. The program with the wrappers is
/// kept for replaying the solver's counterexamples.
fn jit_fuzzers(resolved: &Program, env: &types::Env, chosen: &[bool], say_why: bool) {
    let mut prog = resolved.clone();
    // only what is checked: the chosen declarations (`chosen[i]`) and their wrappers
    let wrappers = verify::wrappers(env, resolved, chosen);
    let mut targets = vec![];
    let push = |f: &FnDecl, targets: &mut Vec<llvm::FuzzTarget>| {
        if f.params.iter().any(|p| p.comptime || matches!(p.ty, TypeExpr::Fn(..))) || crate::stdlib::is_host(&f.name) {
            return;
        }
        // nothing can make a capability or a C handle: `verify` reports these as assumed
        let made_outside = |t: &TypeExpr| matches!(t, TypeExpr::Named(n, _) if env.capabilities.contains(n) || env.externs.contains_key(n));
        if f.params.iter().any(|p| made_outside(&p.ty)) {
            return;
        }
        let inst = if f.type_params.is_empty() { Default::default() } else {
            match types::test_instance(env, f) {
                Some(i) => i,
                None => return,
            }
        };
        let params = f.params.iter().map(|p| (p.name.to_string(), types::subst_texpr(&p.ty, &inst), p.mutable)).collect();
        targets.push(llvm::FuzzTarget { name: f.name.to_string(), params });
    };
    for (i, d) in resolved.decls.iter().enumerate() {
        if !chosen.get(i).copied().unwrap_or(false) {
            continue;
        }
        match &d.kind {
            DeclKind::Fn(f) => push(f, &mut targets),
            // a type's own methods are functions (`Item.new`)
            DeclKind::Impl(imp) if imp.is_inherent() => {
                for m in &imp.methods {
                    push(&types::static_fn(imp, m), &mut targets);
                }
            }
            _ => {}
        }
    }
    for w in &wrappers {
        push(w, &mut targets);
        prog.decls.push(Decl { comments: vec![], doc: vec![], kind: DeclKind::Fn(w.clone()), module: Default::default(), public: false });
    }
    let _ = verify::PROGRAM.set(prog.clone());
    // all at once; when that fails, each alone, to leave out only what doesn't compile
    let mut failed = HashMap::default();
    let ir = match llvm::compile_fuzz(&prog, &targets) {
        Ok(ir) => Some(ir),
        Err(_) => {
            let mut ok = vec![];
            for t in targets.drain(..) {
                match llvm::compile_fuzz(&prog, std::slice::from_ref(&t)) {
                    Ok(_) => ok.push(t),
                    Err(e) => {
                        if say_why {
                            eprintln!("note: `{}` can't be fuzzed: {e}", t.name);
                        }
                        failed.insert(t.name.clone(), format!("it can't be compiled yet: {e}"));
                    }
                }
            }
            targets = ok;
            llvm::compile_fuzz(&prog, &targets).ok()
        }
    };
    let drivers = match ir.map(|ir| llvm::jit::fuzzers(&ir, targets.len())) {
        Some(Ok(addrs)) => targets.iter().zip(addrs).map(|(t, a)| (t.name.clone(), a)).collect(),
        Some(Err(e)) => {
            if say_why {
                eprintln!("note: nothing can be fuzzed: {e}");
            }
            for t in &targets {
                failed.insert(t.name.clone(), format!("it can't be compiled yet: {e}"));
            }
            HashMap::default()
        }
        None => HashMap::default(),
    };
    let _ = verify::JIT_FUZZ.set(verify::JitFuzz { drivers, failed, case: llvm::jit::fuzz_case, lock: std::sync::Mutex::new(()) });
}

