//! Command-line options: a record that implements `std.cli.CommandLine` is
//! read from the program's arguments (see `std/cli.strata` for the rules).
//! The record is built as JSON and read with `Interp::from_json`, so every
//! refinement is checked exactly as for any other input.

use crate::names::short;
use crate::ast::*;
use crate::interp::{Interp, Value};
use crate::types::Env;
use serde_json::{json, Map, Value as Json};

pub const TRAIT: &str = "std.cli.CommandLine";

/// Whether values of the named type are read from the command line.
pub fn is_options(env: &Env, ty: &TypeExpr) -> bool {
    let head = crate::types::texpr_head(ty);
    env.impls.iter().any(|i| i.trait_name == TRAIT && crate::types::texpr_head(&i.for_type) == head)
}

/// What the arguments came to.
pub enum Parsed {
    Options(Value),
    /// `--help`: the text to print, then exit 0.
    Help(String),
    /// A mistake in the arguments: the message (with a hint), then exit 2.
    Error(String),
}

/// A scalar value an argument can hold.
enum Scalar {
    Int,
    Float,
    Text,
    /// Variants without data, as (written form, variant name).
    Choice(Vec<(String, String)>),
}

enum Kind {
    Flag,
    Optional(Scalar),
    Rest(Scalar),
    Positional(Scalar),
}

struct Opt<'a> {
    field: &'a Field,
    /// `--dry-run` / `<dry-run>`
    flag: String,
    kind: Kind,
    /// The type of one value: the field's, or its `Option`'s or `List`'s element.
    elem: TypeExpr,
}

fn dashed(name: &str) -> String {
    name.replace('_', "-")
}


/// `DryRun` -> `dry-run`
fn kebab(n: &str) -> String {
    let mut s = String::new();
    for (i, c) in short(n).chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                s.push('-');
            }
            s.extend(c.to_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

fn scalar(env: &Env, t: &TypeExpr) -> Option<Scalar> {
    match env.expand_alias(t) {
        TypeExpr::Named(n, a) if a.is_empty() => match n.as_str() {
            "Int" => Some(Scalar::Int),
            "Float" => Some(Scalar::Float),
            "String" => Some(Scalar::Text),
            _ => match &env.types.get(&n.sym())?.body {
                TypeBody::Sum(vs) if vs.iter().all(|v| v.payload == Payload::None) => Some(Scalar::Choice(vs.iter().map(|v| (kebab(&v.name), v.name.to_string())).collect())),
                _ => None,
            },
        },
        _ => None,
    }
}

/// How a field is given, and the type of one of its values.
fn kind(env: &Env, t: &TypeExpr) -> Option<(Kind, TypeExpr)> {
    match env.expand_alias(t) {
        TypeExpr::Named(n, a) if n == "Bool" && a.is_empty() => Some((Kind::Flag, t.clone())),
        TypeExpr::Named(n, a) if n == "Option" && a.len() == 1 => scalar(env, &a[0]).map(|s| (Kind::Optional(s), a[0].clone())),
        TypeExpr::Named(n, a) if n == "List" && a.len() == 1 => scalar(env, &a[0]).map(|s| (Kind::Rest(s), a[0].clone())),
        _ => scalar(env, t).map(|s| (Kind::Positional(s), t.clone())),
    }
}

/// The options' fields, or why the record can't be read from a command line.
fn options<'a>(env: &'a Env, name: &str) -> Result<(&'a TypeDecl, Vec<Opt<'a>>), String> {
    let td = env.types.get(&name.sym()).ok_or(format!("no type `{name}`"))?;
    let TypeBody::Record(fs) = &td.body else {
        return Err(format!("`{}` implements `CommandLine`, so it must be a record (each field is an option)", short(name)));
    };
    let mut out = Vec::new();
    for f in fs {
        let (kind, elem) = kind(env, &f.ty).ok_or(format!(
            "`{}.{}` is a `{}`: a command-line option is a Bool, an Int, Float or String (or a refinement of one), a choice (variants without data), or an Option or List of those",
            short(name),
            f.name,
            crate::printer::ty(&f.ty)
        ))?;
        out.push(Opt { field: f, flag: dashed(&f.name), kind, elem });
    }
    if out.iter().filter(|o| matches!(o.kind, Kind::Rest(_))).count() > 1 {
        return Err(format!("`{}` has more than one `List` field: only one can take the remaining arguments", short(name)));
    }
    Ok((td, out))
}

/// Check that a type can be read from the command line (before running).
pub fn validate(env: &Env, name: &str) -> Result<(), String> {
    options(env, name).map(|_| ())
}

/// `Int`, `Positive`, `quiet|loud`: what one value looks like. An inline
/// refinement shows its base type (the condition goes in the help text).
fn placeholder(s: &Scalar, elem: &TypeExpr) -> String {
    match (s, elem) {
        (Scalar::Choice(cs), _) => cs.iter().map(|c| c.0.clone()).collect::<Vec<_>>().join("|"),
        (_, TypeExpr::Refined(b, _)) => short(&crate::printer::ty(b)).to_string(),
        _ => short(&crate::printer::ty(elem)).to_string(),
    }
}

fn help(prog: &str, opts: &[Opt], doc: &[String]) -> String {
    let mut usage = format!("Usage: {prog}");
    let mut args: Vec<(String, String)> = Vec::new();
    let mut flags: Vec<(String, String)> = Vec::new();
    for o in opts {
        let mut text = o.field.doc.join(" ");
        if let TypeExpr::Refined(_, pred) = &o.elem {
            text = format!("{text} (where {})", crate::printer::expr(pred, 0)).trim_start().to_string();
        }
        match &o.kind {
            Kind::Flag => flags.push((format!("--{}", o.flag), text)),
            Kind::Optional(s) => flags.push((format!("--{} <{}>", o.flag, placeholder(s, &o.elem)), text)),
            Kind::Positional(s) => {
                let p = match s {
                    Scalar::Choice(_) => format!("<{}>", placeholder(s, &o.elem)),
                    _ => format!("<{}>", o.flag),
                };
                usage += &format!(" {p}");
                args.push((p, text));
            }
            Kind::Rest(_) => {
                let p = format!("[{}...]", o.flag);
                args.push((p.clone(), text));
                usage += &format!(" {p}");
            }
        }
    }
    flags.push(("-h, --help".into(), "Show this help.".into()));
    if opts.iter().any(|o| matches!(o.kind, Kind::Flag | Kind::Optional(_))) {
        usage = usage.replacen(prog, &format!("{prog} [OPTIONS]"), 1);
    }
    let width = args.iter().chain(&flags).map(|(a, _)| a.chars().count()).max().unwrap_or(0) + 2;
    let rows = |rs: &[(String, String)]| rs.iter().map(|(a, t)| format!("  {a:width$}{t}").trim_end().to_string() + "\n").collect::<String>();
    let mut s = String::new();
    if !doc.is_empty() {
        s += &doc.join("\n");
        s += "\n\n";
    }
    s += &usage;
    s += "\n";
    if !args.is_empty() {
        s += "\nArguments:\n";
        s += &rows(&args);
    }
    s += "\nOptions:\n";
    s += &rows(&flags);
    s
}

/// One argument's text as a value of `elem` (as JSON), with its refinement checked.
fn read(it: &mut Interp, s: &Scalar, elem: &TypeExpr, text: &str) -> Result<Json, String> {
    let j = value(s, text)?;
    it.from_json(elem, &j, "").map(|_| j).map_err(|e| e.trim_start_matches(": ").to_string())
}

/// The JSON for one argument's text, read as `s`.
fn value(s: &Scalar, text: &str) -> Result<Json, String> {
    match s {
        Scalar::Int => text.parse::<i64>().map(|n| json!(n)).map_err(|_| format!("`{text}` is not a whole number")),
        Scalar::Float => text.parse::<f64>().ok().filter(|f| f.is_finite()).map(|f| json!(f)).ok_or(format!("`{text}` is not a number")),
        Scalar::Text => Ok(json!(text)),
        Scalar::Choice(cs) => match cs.iter().find(|(w, n)| w == &text.to_lowercase() || short(n) == text) {
            Some((_, n)) => Ok(json!(short(n))),
            None => Err(format!("`{text}` is not one of {}", cs.iter().map(|c| c.0.as_str()).collect::<Vec<_>>().join(", "))),
        },
    }
}

/// Read `args` into a value of the record type `name`. `doc` is the type's
/// doc comment, shown by `--help` above the usage line.
pub fn parse(it: &mut Interp, env: &Env, name: &str, doc: &[String], prog: &str, args: &[String]) -> Parsed {
    let (_, opts) = match options(env, name) {
        Ok(x) => x,
        Err(e) => return Parsed::Error(e),
    };
    let hint = format!("\n\nRun `{prog} --help` for the options.");
    let fail = |m: String| Parsed::Error(format!("{m}{}", hint));
    let mut given: Map<String, Json> = Map::new();
    let mut flags: Vec<&Opt> = Vec::new();
    let mut positional: Vec<&String> = Vec::new();
    let mut i = 0;
    let mut only_positional = false;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if only_positional || !a.starts_with("--") && a != "-h" {
            positional.push(a);
            continue;
        }
        if a == "--" {
            only_positional = true;
            continue;
        }
        if a == "--help" || a == "-h" {
            return Parsed::Help(help(prog, &opts, doc));
        }
        let (key, inline) = match a[2..].split_once('=') {
            Some((k, v)) => (k.to_string(), Some(v.to_string())),
            None => (a[2..].to_string(), None),
        };
        let Some(o) = opts.iter().find(|o| matches!(o.kind, Kind::Flag | Kind::Optional(_)) && (o.flag == key || o.field.name == key)) else {
            let known: Vec<String> = opts.iter().filter(|o| matches!(o.kind, Kind::Flag | Kind::Optional(_))).map(|o| format!("--{}", o.flag)).collect();
            return fail(if known.is_empty() { format!("unknown option `--{key}` (this program takes no options)") } else { format!("unknown option `--{key}` (the options are {})", known.join(", ")) });
        };
        if flags.iter().any(|f| f.flag == o.flag) {
            return fail(format!("`--{}` is given twice", o.flag));
        }
        flags.push(o);
        match &o.kind {
            Kind::Flag => {
                if let Some(v) = inline {
                    return fail(format!("`--{}` is a flag: it takes no value (got `{v}`)", o.flag));
                }
                given.insert(o.field.name.to_string(), json!(true));
            }
            Kind::Optional(s) => {
                let text = match inline {
                    Some(v) => v,
                    None if i < args.len() => {
                        i += 1;
                        args[i - 1].clone()
                    }
                    None => return fail(format!("`--{}` needs a value", o.flag)),
                };
                match read(it, s, &o.elem, &text) {
                    Ok(j) => {
                        given.insert(o.field.name.to_string(), json!({ "Some": j }));
                    }
                    Err(e) => return fail(format!("--{}: {e}", o.flag)),
                }
            }
            _ => unreachable!(),
        }
    }
    // positional arguments: the required ones in order, then the list
    let mut rest = positional.into_iter();
    for o in &opts {
        if let Kind::Positional(s) = &o.kind {
            let Some(text) = rest.next() else {
                return fail(format!("missing <{}>{}", o.flag, if o.field.doc.is_empty() { String::new() } else { format!(": {}", o.field.doc.join(" ")) }));
            };
            match read(it, s, &o.elem, text) {
                Ok(j) => {
                    given.insert(o.field.name.to_string(), j);
                }
                Err(e) => return fail(format!("<{}>: {e}", o.flag)),
            }
        }
    }
    let left: Vec<&String> = rest.collect();
    match opts.iter().find(|o| matches!(o.kind, Kind::Rest(_))) {
        Some(o) => {
            let Kind::Rest(s) = &o.kind else { unreachable!() };
            let mut xs = Vec::new();
            for text in left {
                match read(it, s, &o.elem, text) {
                    Ok(j) => xs.push(j),
                    Err(e) => return fail(format!("[{}...]: {e}", o.flag)),
                }
            }
            given.insert(o.field.name.to_string(), Json::Array(xs));
        }
        None if !left.is_empty() => return fail(format!("unexpected argument `{}`", left[0])),
        None => {}
    }
    for o in &opts {
        match o.kind {
            Kind::Flag if !given.contains_key(o.field.name.as_str()) => {
                given.insert(o.field.name.to_string(), json!(false));
            }
            Kind::Optional(_) if !given.contains_key(o.field.name.as_str()) => {
                given.insert(o.field.name.to_string(), json!("None"));
            }
            _ => {}
        }
    }
    match it.from_json(&TypeExpr::Named(Symbol::from(name), vec![]), &Json::Object(given), "") {
        Ok(v) => Parsed::Options(v),
        Err(e) => fail(e.trim_start_matches(": ").to_string()),
    }
}
