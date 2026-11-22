//! Commands that show the code: diff, log, the viewer, render, interface, ast, view, builtins.

use crate::*;

/// `strata diff`.
pub fn diff(args: &Args, json_out: bool) -> Result<bool, String> {
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
    Ok(true)
}

/// `strata log`.
pub fn log(args: &Args, json_out: bool) -> Result<bool, String> {
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
    Ok(true)
}

/// `strata ui`.
pub fn ui(args: &Args) -> Result<bool, String> {
    let port: u16 = args.val("--port").unwrap_or("7777").parse().map_err(|_| "bad --port")?;
    let target = match args.val("--file") {
        Some(f) => ui::Target::File(f.into()),
        None => {
            let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
            ui::Target::Db(Codebase::find(&cwd).ok_or("no package here (no strata.toml in this folder or above: run `strata init`, or pass --file)")?)
        }
    };
    if args.flag("--strata") {
        return ui_in_strata(target, port);
    }
    ui::serve(target, port)?;
    Ok(true)
}

/// The viewer written in Strata (`src/ui/viewer.strata`), built into the
/// binary: an app served by `strata serve`'s runtime, reading the codebase
/// through `Code` (std.code), which reads the target.
const VIEWER: &[u8] = include_bytes!("../ui/viewer.strata");

fn ui_in_strata(target: ui::Target, port: u16) -> Result<bool, String> {
    let root = match &target {
        ui::Target::Db(p) | ui::Target::File(p) => p.to_string_lossy().to_string(),
    };
    ui::set_code_root(&root);
    let p = package::decode_file(std::path::Path::new("viewer.strata"), VIEWER)?;
    let types::Checked { report, env, .. } = full_checked(&p);
    if !report.ok() {
        print_errors(&report);
        return Err("the built-in viewer doesn't check: this strata is broken".into());
    }
    crate::serve::serve(&p, &env, port, "Strata")?;
    Ok(true)
}

/// `strata builtins`.
pub fn builtins(json_out: bool) -> Result<bool, String> {
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
    Ok(true)
}

/// `strata render`.
pub fn render(args: &Args) -> Result<bool, String> {
    let src = load(&args)?;
    // shown as written: each reference as short as the module's `use`s allow
    let p = &resolve::written_form(src.program());
    // a union's members inline (by default: a type with a doc, an `impl` or
    // a `derive` of its own stays on its own) or each on its own; both read
    // back the same
    let viewed;
    let p = match args.val("--members") {
        Some("separate") => {
            viewed = resolve::members_view(p, resolve::Members::Separate);
            &viewed
        }
        None | Some("inline") => {
            viewed = resolve::members_view(p, resolve::Members::Inline);
            &viewed
        }
        Some(other) => return Err(format!("`--members {other}`: `separate` or `inline`")),
    };
    match args.val("--split") {
        None => {
            // a module is a file: print one
            let mut mods: Vec<String> = p.decls.iter().map(|d| d.module.to_string()).collect();
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
        Some(dir) => return write_modules(p, std::path::Path::new(dir)),
    }
    Ok(true)
}

/// `strata pack FILE.strata`: the whole program in one file, a bundle (with
/// the package's `strata.toml`).
pub fn pack(args: &Args) -> Result<bool, String> {
    let out = args.pos.get(1).ok_or("usage: strata pack <FILE.strata>")?;
    // the prelude's definitions, its types and its code, as a module file of
    // the root module, named as programs write them (`Option`, `CodePoints`):
    // what every program runs with, for Strata code that runs trees
    // (`std.ast.eval`). As parsed: storing would resolve its names against
    // the prelude itself.
    if args.flag("--prelude") {
        let mut p = crate::parser::parse_program(crate::types::PRELUDE).map_err(|e| format!("the prelude: {e}"))?;
        p.decls.extend(crate::parser::parse_program(crate::types::PRELUDE_CODE).map_err(|e| format!("the prelude's code: {e}"))?.decls);
        let bytes = binary::encode_module(&p, "", &Default::default());
        store::write_file(std::path::Path::new(out), &bytes)?;
        println!("wrote {out}: the prelude, {} definitions, {} bytes", p.decls.len(), bytes.len());
        return Ok(true);
    }
    let src = load(&args)?;
    let dir = match (&src, args.val("--file")) {
        (Source::Db(cb), _) => Some(cb.root().to_path_buf()),
        (_, Some(f)) if std::path::Path::new(f).is_dir() => Some(f.into()),
        _ => None,
    };
    let manifest = dir.and_then(|d| std::fs::read_to_string(d.join(package::FILE)).ok());
    let p = src.program();
    let n = p.decls.len();
    let bytes = binary::encode_bundle(&binary::Image { manifest, program: p.clone() });
    store::write_file(std::path::Path::new(out), &bytes)?;
    println!("wrote {out}: {n} definition{}, {} bytes", if n == 1 { "" } else { "s" }, bytes.len());
    Ok(true)
}

/// `strata header FILE.strata`: a module file's table of contents.
pub fn header(args: &Args) -> Result<bool, String> {
    let file = args.pos.get(1).ok_or("usage: strata header <FILE.strata>")?;
    let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
    // each definition's tree as a `std.ast` value, one per line (a bundle's too)
    // the definitions as the printer writes them, as stored (not shortened
    // to their written form, which `render` shows): what `std.ast.text` is
    // tested against
    if args.flag("--text") {
        let decls = binary::read(&bytes).map_err(|e| format!("{file}: {e}"))?.program.decls;
        print!("{}\n", decls.iter().map(crate::printer::decl).collect::<Vec<_>>().join("\n\n"));
        return Ok(true);
    }
    if args.flag("--tree") {
        for d in binary::read(&bytes).map_err(|e| format!("{file}: {e}"))?.program.decls {
            println!("{}", crate::ast_value::decl(&d));
        }
        return Ok(true);
    }
    let h = binary::read_header(&bytes).map_err(|e| format!("{file}: {e}"))?;
    let hex = |h: &binary::Hash| h[..8].iter().map(|b| format!("{b:02x}")).collect::<String>();
    println!("module {}: {} definitions", if h.module.is_empty() { "main" } else { &h.module }, h.entries.len());
    for e in &h.entries {
        let vis = if e.public { "pub " } else { "" };
        println!("{:<6} {vis}{}", e.kind, e.name);
        println!("       #{}  interface #{}  {} bytes ({} interface)", hex(&e.hash), hex(&e.interface_hash), e.size, e.interface_size);
        if !e.deps.is_empty() {
            println!("       uses {}", e.deps.join(", "));
        }
    }
    Ok(true)
}

/// One text file per module, in canonical form (`shop/orders.st`; the root
/// module is `main.st`): to read, never read back.
fn write_modules(p: &Program, dir: &std::path::Path) -> Result<bool, String> {
    let mut mods: Vec<String> = p.decls.iter().map(|d| d.module.to_string()).collect();
    mods.sort();
    mods.dedup();
    for m in mods {
        let path = dir.join(store::module_path(&m)).with_extension(store::TEXT_EXT);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, printer::module_file(p, &m)).map_err(|e| e.to_string())?;
        println!("wrote {}", path.display());
    }
    Ok(true)
}

/// `strata interface`.
pub fn interface(args: &Args) -> Result<bool, String> {
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
    let st = view::Status { holes: HashMap::default(), verdicts: verdicts_for(&src) };
    print!("{}", view::render(p, &names, 1, &st));
    Ok(true)
}

/// `strata ast`.
pub fn ast(args: &Args) -> Result<bool, String> {
    let src = load(&args)?;
    let name = args.pos.get(1).ok_or("usage: strata ast <name>")?;
    let d = src.program().decls.iter().find(|d| &d.qname() == name).ok_or(format!("no definition `{name}`"))?;
    println!("{}", serde_json::to_string_pretty(&json!({"name": name, "hash": store::self_hash(d), "interface_hash": store::interface_hash(d), "node": d})).unwrap());
    Ok(true)
}

/// `strata view`.
pub fn view(args: &Args) -> Result<bool, String> {
    let src = load(&args)?;
    let p = src.program();
    let level: u8 = args.val("--level").unwrap_or("1").parse().ok().filter(|l| *l <= 2).ok_or("--level must be 0 (map), 1 (interfaces) or 2 (source)")?;
    let mut names = view::select(p, args.pos.get(1).map(|s| s.as_str()));
    // mocks are for tests: shown when asked for, or picked by name
    if !args.flag("--mocks") && args.pos.get(1).is_none() {
        let hidden = view::mock_decls(p);
        names.retain(|n| !hidden.contains(n));
    }
    let rep = types::check(p);
    let mut holes: HashMap<String, Vec<String>> = HashMap::default();
    for h in rep.holes {
        holes.entry(h.decl).or_default().push(h.name);
    }
    let st = view::Status { holes, verdicts: verdicts_for(&src) };
    print!("{}", view::render(p, &names, level, &st));
    Ok(true)
}

/// `strata lex FILE`: a text file's tokens (`3:5 nl sp Ident(x)`: where, a
/// newline or a space before it, what), then its comments, or the lexer's
/// error. A float is shown by its bits and a string as the printer quotes
/// it, so `std.ast.lex`'s output can be compared byte for byte.
pub fn lex(args: &Args) -> Result<bool, String> {
    use crate::lexer::{FPart, Tok};
    let file = args.pos.get(1).ok_or("usage: strata lex <FILE>")?;
    let src = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let quote = |s: &str| format!("\"{}\"", crate::printer::escape(s));
    let (tokens, comments) = match crate::lexer::lex_with_comments(&src) {
        Ok(tc) => tc,
        Err(e) => {
            println!("error: {e}");
            return Ok(true);
        }
    };
    for t in &tokens {
        let what = match &t.tok {
            Tok::Int(n) => format!("Int({n})"),
            Tok::BigInt(s) => format!("BigInt({s})"),
            Tok::Float(f) => format!("Float({})", f.to_bits()),
            Tok::Str(s) => format!("Str({})", quote(s)),
            Tok::FStr(ps) => format!(
                "FStr({})",
                ps.iter()
                    .map(|p| match p {
                        FPart::Lit(s) => quote(s),
                        FPart::Code(c, l, k) => format!("{{{}@{l}:{k}}}", quote(c)),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Tok::Ident(s) => format!("Ident({s})"),
            Tok::Hole(s) => format!("Hole({s})"),
            Tok::Doc(s) => format!("Doc({})", quote(s)),
            Tok::Sym(s) => format!("Sym({s})"),
            Tok::Eof => "Eof".to_string(),
        };
        println!("{}:{}{}{} {what}", t.line, t.col, if t.nl_before { " nl" } else { "" }, if t.space_before { " sp" } else { "" });
    }
    for c in &comments {
        println!("{}:{} comment {}{}", c.line, c.col, quote(&c.text), if c.trailing { " trailing" } else { "" });
    }
    Ok(true)
}

/// `strata parse FILE`: a text file parsed as a script, each definition as
/// the `std.ast` value `debug` prints, one per line, as the parser gives it
/// (storing it would qualify its references), or the parser's error.
pub fn parse(args: &Args) -> Result<bool, String> {
    let file = args.pos.get(1).ok_or("usage: strata parse <FILE>")?;
    let src = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    match crate::parser::parse_program(&src) {
        Ok(p) => {
            // `--stored`: as it would be stored, every reference qualified
            let p = if args.flag("--stored") { crate::resolve::stored_form(&p) } else { p };
            for d in &p.decls {
                println!("{}", crate::ast_value::decl(d));
            }
        }
        Err(e) => println!("error: {e}"),
    }
    Ok(true)
}
