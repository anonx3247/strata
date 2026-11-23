//! Commands that check the code: check, holes, verify.

use crate::*;

/// `strata check`.
pub fn check(args: &Args, json_out: bool) -> Result<bool, String> {
    if let Some(dir) = crate::core::dir_of(args.val("--file")) {
        let rep = crate::core::check(&dir);
        print_errors(&rep);
        if rep.ok() {
            println!("ok: core checks");
        }
        return Ok(rep.ok());
    }
    if args.flag("--strata") {
        return super::compare::check_strata(args.val("--file").unwrap_or("."));
    }
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
    if args.flag("--compare") {
        return super::compare::compare(args.val("--file").unwrap_or("."), &rep);
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
    // `compiler verify`, the command written in Strata, unless `--rust`,
    // `STRATA_BACKEND=rust` or `--json` (Rust's only), or no compiler
    // package is found; `--compare`: that and `verify --rust --no-race`,
    // line by line
    if crate::strata_verifies(args) && !json_out || args.flag("--strata") || args.flag("--compare") {
        return crate::cmd::compare::verify(args);
    }
    let src = load(&args)?;
    let how = Verifying { file: args.val("--file"), only: None, quiet: false, json: json_out };
    Ok(verify_source(args, &src, &how)?.0)
}

/// How `verify_source` runs: for `strata verify`, or before `build`.
pub struct Verifying<'a> {
    /// The script's path (where its caches go), or the package's folder.
    pub file: Option<&'a str>,
    /// Only these definitions (by qualified name); else the command's selection.
    pub only: Option<std::collections::BTreeSet<String>>,
    /// Print nothing: the caller reports (`build`).
    pub quiet: bool,
    pub json: bool,
}

/// Verify `src` (what `how` selects), keep the verdicts and proofs, and
/// report them (unless quiet). Whether nothing failed, and each verdict.
pub fn verify_source(args: &Args, src: &Source, how: &Verifying) -> Result<(bool, Vec<(String, verify::Verdict, bool)>), String> {
    let json_out = how.json;
    let p = src.program();
    let types::Checked { report: rep, program: resolved, env, compiled } = full_checked(p);
    if !rep.ok() {
        print_errors(&rep);
        return Ok((false, vec![]));
    }
    let cases: usize = args.val("--cases").unwrap_or("200").parse().map_err(|_| "bad --cases")?;
    let seed: u64 = args.val("--seed").unwrap_or("1").parse().map_err(|_| "bad --seed")?;
    let selected = how.only.is_some() || args.pos.get(1).is_some();
    let names = match &how.only {
        Some(only) => only.clone(),
        None => view::select(p, args.pos.get(1).map(|s| s.as_str()))?,
    };
    // the proofs so far (a package's in its `.strata/`, a script's in ~/.cache/strata/scripts)
    let proofs_at = proofs_dir(src, how.file);
    let mut all_ok = true;
    let mut results = Vec::new();
    let solver = smt::solver();
    let solver_bin = solver.as_ref();
    // with a selection, termination is analysed for what the selected code reaches
    // (`--termination` analyses what the selection reaches, as compiler/termination does)
    let roots: Option<crate::fx::HashSet<String>> = (selected || args.flag("--termination")).then(|| {
        let mut rs = crate::fx::HashSet::default();
        for (orig, d) in p.decls.iter().zip(&resolved.decls) {
            if !names.contains(&orig.qname()) {
                continue;
            }
            match &d.kind {
                DeclKind::Fn(f) => {
                    rs.insert(f.name.to_string());
                }
                DeclKind::Impl(imp) if imp.is_inherent() => rs.extend(imp.methods.iter().map(|m| types::static_fn(imp, m).name.to_string())),
                DeclKind::Impl(imp) => {
                    if matches!(&orig.kind, DeclKind::Impl(i) if i.derived) {
                        let module = crate::names::module_of(&imp.trait_name).to_string();
                        let tag = format!(".for.{}.", crate::names::short(&types::texpr_head(&imp.for_type)));
                        for c in &resolved.decls[p.decls.len()..] {
                            if let DeclKind::Fn(f) = &c.kind {
                                if f.name.as_str().contains(&tag) && f.name.as_str().starts_with(&format!("{module}.")) {
                                    rs.insert(f.name.to_string());
                                }
                            }
                        }
                    }
                    if let Some(tr) = env.traits.get(&imp.trait_name) {
                        rs.extend(tr.methods.iter().map(|m| termination::impl_label(&env, imp, &m.decl.name)));
                    }
                }
                _ => {}
            }
        }
        rs
    });
    let analysis = termination::analyze(&env, solver_bin, roots.as_ref());
    let transparent = &analysis.transparent;
    verify::RACE.store(!args.flag("--no-race"), std::sync::atomic::Ordering::Relaxed);
    let cross_check: usize = args.val("--cross-check").unwrap_or("20").parse().map_err(|_| "bad --cross-check")?;
    let require: Vec<String> = args.val("--require").map(|r| r.split(',').map(|s| s.trim().to_string()).collect()).unwrap_or_default();
    // the package's `[verify]` rules apply too
    let policy = package::find(std::path::Path::new(how.file.unwrap_or("."))).ok().flatten().map(|p| p.verify).unwrap_or_default();
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
        return Ok((true, vec![]));
    }
    if args.flag("--termination") {
        // each function's and implementation method's termination, and
        // whether a contract may see through it (compared with `compiler
        // termination`, the analysis written in Strata: tests/smt.rs)
        let mut shown: Vec<String> = vec![];
        for (orig, d) in p.decls.iter().zip(&resolved.decls) {
            if !names.contains(&orig.qname()) {
                continue;
            }
            match &d.kind {
                DeclKind::Fn(f) if !f.params.iter().any(|p| p.comptime) => shown.push(f.name.to_string()),
                DeclKind::Impl(imp) if !imp.is_inherent() => {
                    if let Some(tr) = env.traits.get(&imp.trait_name) {
                        for m in &tr.methods {
                            if m.default || imp.methods.iter().any(|x| x.name == m.decl.name) {
                                shown.push(termination::impl_label(&env, imp, &m.decl.name));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for n in shown {
            println!("fn {n}");
            match analysis.status.get(&n) {
                Some(termination::Termination::Proven { how }) => println!("  proven: {how}"),
                Some(termination::Termination::Assumed { by }) => println!("  assumed: {}", by.join(", ")),
                Some(termination::Termination::Unknown { reason }) => println!("  unknown: {reason}"),
                None => println!("  none"),
            }
            match analysis.transparent.get(&n) {
                Some(true) => println!("  transparent (assumed)"),
                Some(false) => println!("  transparent"),
                None => {}
            }
        }
        return Ok((true, vec![]));
    }
    if args.flag("--obligations") {
        // the solver's answer to each obligation, nothing run (compared with
        // `compiler smt`, the verifier written in Strata: tests/smt.rs)
        let Some(bin) = solver_bin else { return Err("no SMT solver was found (install z3)".into()) };
        for (orig, d) in p.decls.iter().zip(&resolved.decls) {
            if let (true, DeclKind::Fn(f)) = (names.contains(&orig.qname()), &d.kind) {
                if f.params.iter().any(|p| p.comptime) {
                    continue;
                }
                println!("fn {}", f.name);
                for line in verify::obligation_outcomes(&env, f, transparent, bin) {
                    println!("  {line}");
                }
            }
        }
        return Ok((true, vec![]));
    }
    let closures = store::Closures::new(p);
    // results depend on the solver: it is part of the cache key
    let skey = solver.as_ref().map(|s| s.version.clone()).unwrap_or("no-solver".into());
    // every check to make, by definition: (label, cache key, how), with the
    // definition's key and module; run in parallel below, reported in order
    type Job<'a> = Box<dyn Fn() -> verify::Verdict + Send + Sync + 'a>;
    let mut defs: Vec<(usize, String, String, String, Vec<(String, String, Job)>)> = vec![];
    for (i, (orig, d)) in p.decls.iter().zip(&resolved.decls).enumerate() {
        if !names.contains(&orig.qname()) {
            continue;
        }
        let env = &env;
        // only code shown to terminate is cross-checked: running anything else may never end
        let analysis = &analysis;
        let cc = move |label: &str| if matches!(analysis.status.get(label), Some(termination::Termination::Proven { .. })) { cross_check } else { 0 };
        // (label, cache key, how to check)
        let mut jobs: Vec<(String, String, Job)> = Vec::new();
        let base = closures.hash(&orig.qname());
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
                    let own = imp.methods.iter().find(|x| x.name == mname);
                    // a default of the prelude's traits (`map` on any iterator) is library code
                    if own.is_none() && crate::core::CODE.iter().any(|m| mname.as_str().starts_with(&format!("{m}."))) {
                        continue;
                    }
                    let decl = own.or(if m.default { Some(&m.decl) } else { None });
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
                    let label = format!("law {} for {ty}", law.name);
                    let n = cc(&label);
                    let wrapper = verify::law_wrapper(env, imp, i, law);
                    jobs.push((label, format!("{base}:law:{}:{cases}:{n}:{skey}", law.name), Box::new(move || verify::check_law(env, imp, &wrapper, law, &key, solver_bin, transparent, cases, n, seed))));
                }
            }
            _ => {}
        }
        defs.push((i, base, orig.module.to_string(), orig.qname(), jobs));
    }
    // verdicts kept from earlier runs (by this compiler, with these
    // settings), unless --no-cache
    let race = !args.flag("--no-race");
    let solver_path = solver.as_ref().map(|s| s.bin.as_str()).unwrap_or("");
    // std built into strata is part of every program but std itself, whose
    // closures cover it
    let is_std = p.decls.iter().any(|d| crate::stdlib::is_std(&d.module) && !types::is_prelude_module(&d.module));
    let std = if is_std { "" } else { verify::std_hash() };
    let kept = verdicts_at(src, how.file).with_settings(&format!("seed {seed} race {race} solver {solver_path} {skey} std {std}"));
    let mut found: Vec<Option<verify::Verdict>> =
        defs.iter().flat_map(|d| &d.4).map(|j| if args.flag("--no-cache") || args.flag("--fuzz") { None } else { kept.get_check(&j.0, &j.1) }).collect();
    // what is fuzzed, compiled, all in one module (`--jit` says why when
    // something can't be): only the definitions with a check to run
    let mut chosen = vec![false; p.decls.len()];
    let mut at = 0;
    for (i, _, _, _, jobs) in &defs {
        chosen[*i] = found[at..at + jobs.len()].iter().any(|v| v.is_none());
        at += jobs.len();
    }
    if chosen.contains(&true) {
        jit_fuzzers(&compiled, &env, &chosen, args.flag("--jit"));
    }
    // the checks not kept, on as many threads as there are cores: each
    // solver query is a process of its own
    if args.flag("--fuzz") {
        // each function's fuzzing alone, no solver (compared with `compiler
        // fuzz`, which runs compiler/codegen's drivers in its own JIT: tests/codegen.rs)
        for (orig, d) in p.decls.iter().zip(&resolved.decls) {
            if let (true, DeclKind::Fn(f)) = (names.contains(&orig.qname()), &d.kind) {
                if f.params.iter().any(|p| p.comptime) {
                    continue;
                }
                println!("fn {}", f.name);
                for line in verify::fuzz_lines(&f.name, cases, seed) {
                    println!("  {line}");
                }
            }
        }
        return Ok((true, vec![]));
    }
    let todo: Vec<&(String, String, Job)> = defs.iter().flat_map(|d| &d.4).zip(&found).filter(|(_, v)| v.is_none()).map(|(j, _)| j).collect();
    let made = crate::par::map(&todo, |(label, key, job)| {
        let v = job();
        kept.put_check(label, key, &v);
        v
    });
    // what `build` leaves out: each function's proven checks, for its code as it is now
    let mut proofs = proofs_at.as_deref().map(store::load_proofs).unwrap_or_default();
    let mut made = made.into_iter();
    let mut found = found.iter_mut();
    for (_, base, module, qname, jobs) in defs {
        // verified as it is now (`build` verifies what isn't)
        proofs.verified.insert(qname.clone(), base.clone());
        let mut worst: Option<verify::Verdict> = None;
        for (label, _, _) in jobs {
            let (verdict, cached) = match found.next().and_then(|v| v.take()) {
                Some(v) => (v, true),
                None => (made.next().expect("a verdict per check"), false),
            };
            let verdict = verdict.with_termination(analysis.status.get(&label).cloned());
            if matches!(verdict, verify::Verdict::Counterexample { .. }) {
                all_ok = false;
            }
            if worst.as_ref().map_or(true, |w| verdict.rank() > w.rank()) {
                worst = Some(verdict.clone());
            }
            match &verdict {
                verify::Verdict::Proven { checks, given, .. } => {
                    let proven = checks.iter().chain(given).cloned().collect();
                    proofs.functions.insert(label.clone(), store::Proved { decl: qname.clone(), hash: base.clone(), proven, open: vec![] });
                }
                verify::Verdict::Fuzzed { checks, open, .. } if !checks.is_empty() => {
                    proofs.functions.insert(label.clone(), store::Proved { decl: qname.clone(), hash: base.clone(), proven: checks.clone(), open: open.clone() });
                }
                _ => {
                    proofs.functions.remove(&label);
                }
            }
            module_of.insert(label.clone(), module.clone());
            results.push((label, verdict, cached));
        }
        // the viewer shows one status per definition
        if let Some(w) = worst {
            kept.put(&base, &w);
        }
    }
    if let Some(d) = &proofs_at {
        store::save_proofs(d, &proofs);
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
    if how.quiet {
        return Ok((all_ok, results));
    }
    if json_out {
        let out: Vec<_> = results.iter().map(|(n, v, c)| json!({"fn": n.strip_prefix(crate::resolve::ROOT_FN).unwrap_or(n), "cached": c, "verdict": v})).collect();
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        return Ok((all_ok, results));
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
                println!("✗ {n:<24} {kind} violation{by}{c}\n    input:   {}", args.join(", "));
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
    Ok((all_ok, results))
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
    // a target that can't be compiled is named in the error: leave it out and
    // compile the others again (one more compile per such target, not one per target)
    let fuzz = |targets: &[llvm::FuzzTarget]| llvm::compile_fuzz(&prog, targets);
    let mut first = fuzz(&targets);
    while let Err(e) = &first {
        let Some(i) = targets.iter().position(|t| e.contains(&format!("can't compile the fuzzing of `{}`", t.name))) else { break };
        let t = targets.remove(i);
        if say_why {
            eprintln!("note: `{}` can't be fuzzed: {e}", t.name);
        }
        failed.insert(t.name.clone(), format!("it can't be compiled yet: {e}"));
        first = fuzz(&targets);
    }
    let ir = match first {
        Ok(made) => Some(made),
        Err(_) => {
            // an error that names no target (it is in code a target reaches):
            // halve the targets until each failing one is alone, a few
            // compiles per such target, not one per target
            fn split(prog: &Program, ts: Vec<llvm::FuzzTarget>, ok: &mut Vec<llvm::FuzzTarget>, failed: &mut HashMap<String, String>, say_why: bool) {
                match llvm::compile_fuzz(prog, &ts) {
                    Ok(_) => ok.extend(ts),
                    Err(e) if ts.len() == 1 => {
                        if say_why {
                            eprintln!("note: `{}` can't be fuzzed: {e}", ts[0].name);
                        }
                        failed.insert(ts[0].name.clone(), format!("it can't be compiled yet: {e}"));
                    }
                    Err(_) => {
                        let mut ts = ts;
                        let back = ts.split_off(ts.len() / 2);
                        split(prog, ts, ok, failed, say_why);
                        split(prog, back, ok, failed, say_why);
                    }
                }
            }
            let mut ok = vec![];
            if targets.len() > 1 {
                let back = targets.split_off(targets.len() / 2);
                split(&prog, std::mem::take(&mut targets), &mut ok, &mut failed, say_why);
                split(&prog, back, &mut ok, &mut failed, say_why);
            } else {
                split(&prog, std::mem::take(&mut targets), &mut ok, &mut failed, say_why);
            }
            targets = ok;
            llvm::compile_fuzz(&prog, &targets).ok()
        }
    };
    let drivers = match ir.map(|(ir, shapes, named)| llvm::jit::fuzzers(&ir, shapes, named)) {
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

