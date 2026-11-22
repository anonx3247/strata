//! Records, sums, tuples and unions as SMT datatypes (`declare-datatypes`).
//!
//! A structured value the code has not built itself (a parameter, a call's
//! result, a part of either) is one term of its type's datatype sort
//! (`Val::Data`). Reading it unfolds one level through the datatype's
//! selectors (`Vc::unfold`): no fresh unknowns, so the query follows what the
//! code reads, never the size of the type, and two such values are equal
//! exactly when their terms are (`(= a b)`). Lists inside them are SMT
//! sequences.
//!
//! What a type's refinements say is not part of the sort: "`t` is a valid
//! `T`" is an uninterpreted predicate (`valid_term`), defined for each term
//! the code unfolds by the check of its unfolded level, whose parts are in
//! turn the predicate of their own terms. So a recursive type is never
//! expanded, and nothing is quantified.

use super::*;

/// A sort the solver is told about.
#[derive(Clone, Debug)]
pub(super) enum SortDef {
    /// A datatype: its constructors (names without the bars) and the sorts
    /// of their fields.
    Data(Vec<(String, Vec<String>)>),
    /// A sort nothing is known of (floats, functions, dicts, a type
    /// parameter): its values are only equal or not.
    Opaque,
    /// Lists of items of this sort: a list's length and items are
    /// functions of it (`list_len`, `list_at`). Its values stand for lists,
    /// so equal terms are equal lists (not SMT sequences: Z3 finds no models
    /// for sequences of datatypes inside quantified facts).
    List(String),
}

/// The length of list `t` of list sort `name`.
pub(super) fn list_len(name: &str, t: &str) -> String {
    format!("({} {t})", sym(&format!("{name}.len")))
}

/// Item `i` of list `t` of list sort `name`.
pub(super) fn list_at(name: &str, t: &str, i: &str) -> String {
    format!("({} {t} {i})", sym(&format!("{name}.at")))
}

/// The sorts and predicates a function's queries use.
#[derive(Clone, Debug, Default)]
pub(super) struct Sorts {
    /// By name (without the bars), in the order they were met.
    defs: Vec<(String, SortDef)>,
    index: HashMap<String, usize>,
    /// The validity predicates: name, the sort they apply to; by type.
    valid: Vec<(String, String)>,
    valid_index: HashMap<String, usize>,
}

/// `name` as an SMT-LIB symbol.
fn sym(name: &str) -> String {
    format!("|{}|", name.replace(['|', '\\'], ""))
}

/// The name of the datatype of `t` (a sort type). Every sort's name starts
/// with `T:`, so that none is one the solver defines (`Set`, `Array`, `Seq`).
fn dt_name(t: &TypeExpr) -> String {
    format!("T:{}", crate::printer::ty(t).replace(['|', '\\'], ""))
}

/// The names of the datatypes a sort mentions.
fn sorts_in(sort: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = sort;
    while let Some(a) = rest.find('|') {
        let Some(b) = rest[a + 1..].find('|') else { break };
        out.push(rest[a + 1..a + 1 + b].to_string());
        rest = &rest[a + 2 + b..];
    }
    out
}

impl Sorts {
    /// The declarations of every sort and predicate, dependencies first
    /// (mutually recursive datatypes together).
    pub(super) fn declarations(&self) -> String {
        let mut s = String::new();
        for (n, d) in &self.defs {
            if matches!(d, SortDef::Opaque | SortDef::List(_)) {
                s.push_str(&format!("(declare-sort {} 0)\n", sym(n)));
            }
        }
        // strongly connected components (Tarjan), each after what it uses
        let datas: Vec<usize> = (0..self.defs.len()).filter(|&i| matches!(self.defs[i].1, SortDef::Data(_))).collect();
        let deps = |i: usize| -> Vec<usize> {
            let SortDef::Data(cs) = &self.defs[i].1 else { return vec![] };
            let mut out: Vec<usize> = cs.iter().flat_map(|c| &c.1).flat_map(|f| sorts_in(f)).filter_map(|n| self.index.get(&n).copied()).filter(|&j| matches!(self.defs[j].1, SortDef::Data(_))).collect();
            out.sort();
            out.dedup();
            out
        };
        struct Tarjan {
            index: Vec<Option<usize>>,
            low: Vec<usize>,
            on: Vec<bool>,
            stack: Vec<usize>,
            next: usize,
            groups: Vec<Vec<usize>>,
        }
        fn visit(t: &mut Tarjan, v: usize, deps: &dyn Fn(usize) -> Vec<usize>) {
            t.index[v] = Some(t.next);
            t.low[v] = t.next;
            t.next += 1;
            t.stack.push(v);
            t.on[v] = true;
            for w in deps(v) {
                match t.index[w] {
                    None => {
                        visit(t, w, deps);
                        t.low[v] = t.low[v].min(t.low[w]);
                    }
                    Some(iw) if t.on[w] => t.low[v] = t.low[v].min(iw),
                    _ => {}
                }
            }
            if Some(t.low[v]) == t.index[v] {
                let mut g = vec![];
                while let Some(w) = t.stack.pop() {
                    t.on[w] = false;
                    g.push(w);
                    if w == v {
                        break;
                    }
                }
                g.sort();
                t.groups.push(g);
            }
        }
        let n = self.defs.len();
        let mut t = Tarjan { index: vec![None; n], low: vec![0; n], on: vec![false; n], stack: vec![], next: 0, groups: vec![] };
        for &i in &datas {
            if t.index[i].is_none() {
                visit(&mut t, i, &deps);
            }
        }
        // (Tarjan finishes a component after the ones it reaches)
        for g in &t.groups {
            let heads: Vec<String> = g.iter().map(|&i| format!("({} 0)", sym(&self.defs[i].0))).collect();
            let bodies: Vec<String> = g
                .iter()
                .map(|&i| {
                    let SortDef::Data(cs) = &self.defs[i].1 else { unreachable!() };
                    let cs: Vec<String> = cs
                        .iter()
                        .map(|(c, fs)| {
                            let fs: Vec<String> = fs.iter().enumerate().map(|(k, f)| format!(" ({} {f})", sym(&format!("{c}.{k}")))).collect();
                            format!("({}{})", sym(c), fs.join(""))
                        })
                        .collect();
                    format!("({})", cs.join(" "))
                })
                .collect();
            s.push_str(&format!("(declare-datatypes ({}) ({}))\n", heads.join(" "), bodies.join(" ")));
        }
        for (n, d) in &self.defs {
            if let SortDef::Data(cs) = d {
                let mut e = (cs.len().saturating_sub(1)).to_string();
                for k in (0..cs.len().saturating_sub(1)).rev() {
                    e = format!("(ite ((_ is {}) x) {k} {e})", sym(&cs[k].0));
                }
                s.push_str(&format!("(define-fun {} ((x {})) Int {e})\n", sym(&format!("{n}.tag")), sym(n)));
            }
        }
        for (n, d) in &self.defs {
            if let SortDef::List(item) = d {
                s.push_str(&format!("(declare-fun {} ({}) Int)\n(declare-fun {} ({} Int) {item})\n", sym(&format!("{n}.len")), sym(n), sym(&format!("{n}.at")), sym(n)));
            }
        }
        for (v, sort) in &self.valid {
            s.push_str(&format!("(declare-fun {v} ({sort}) Bool)\n"));
        }
        s
    }

    fn data(&self, sort: &str) -> Option<&Vec<(String, Vec<String>)>> {
        let n = sort.strip_prefix('|')?.strip_suffix('|')?;
        match &self.defs[*self.index.get(n)?].1 {
            SortDef::Data(cs) => Some(cs),
            SortDef::Opaque | SortDef::List(_) => None,
        }
    }
}

impl Vc<'_> {
    /// `t` as far as its sort is concerned: refinements, aliases (distinct
    /// ones too: a distinct value is its inner value), variants (their sum)
    /// and tasks (their result) looked through.
    pub(super) fn sort_type(&self, t: &TypeExpr) -> TypeExpr {
        match t {
            TypeExpr::Refined(b, _) => self.sort_type(b),
            TypeExpr::Union(ms) => {
                let mut out = vec![];
                for m in ms {
                    self.flatten_union(m, &mut out);
                }
                TypeExpr::Union(out)
            }
            TypeExpr::Unit | TypeExpr::Fn(..) => t.clone(),
            TypeExpr::Named(n, args) => {
                if let Some(p) = self.env.variant_parent(n.sym()) {
                    return self.sort_type(&TypeExpr::Named(p, args.clone()));
                }
                if n == "Task" && args.len() == 1 {
                    return self.sort_type(&args[0]);
                }
                if let Some(td) = self.env.types.get(&n.sym()) {
                    if let TypeBody::Alias(a) = &td.body {
                        let sub: HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                        return self.sort_type(&subst_texpr(a, &sub));
                    }
                }
                TypeExpr::Named(n.clone(), args.iter().map(|a| self.sort_type(a)).collect())
            }
        }
    }

    fn opaque_sort(&mut self, name: &str) -> String {
        let name = &format!("T:{name}");
        if !self.sorts.index.contains_key(name) {
            self.sorts.index.insert(name.to_string(), self.sorts.defs.len());
            self.sorts.defs.push((name.to_string(), SortDef::Opaque));
        }
        sym(name)
    }

    /// Declare datatype `name` (once), its constructors made by `ctors`.
    fn datatype(&mut self, name: &str, ctors: impl FnOnce(&mut Self) -> Vec<(String, Vec<String>)>) -> String {
        let name = name.replace(['|', '\\'], "");
        let name = if name.starts_with("T:") { name } else { format!("T:{name}") };
        if !self.sorts.index.contains_key(&name) {
            let i = self.sorts.defs.len();
            self.sorts.index.insert(name.clone(), i);
            // (a placeholder while its fields are declared: they may be of this sort)
            self.sorts.defs.push((name.clone(), SortDef::Data(vec![])));
            let cs = ctors(self);
            self.sorts.defs[i].1 = SortDef::Data(cs);
        }
        sym(&name)
    }

    /// The sort of a field of type `t`: its own, or an opaque one.
    fn field_sort(&mut self, t: &TypeExpr) -> String {
        self.sort_of(t).unwrap_or_else(|| self.opaque_sort("Unmodelled"))
    }

    /// The SMT sort of the values of `t`; `None` for a union whose members
    /// can't be told apart by shape.
    pub(super) fn sort_of(&mut self, t: &TypeExpr) -> Option<String> {
        let t = self.sort_type(t);
        Some(match &t {
            TypeExpr::Unit => self.datatype("T:Unit", |_| vec![("T:Unit.unit".into(), vec![])]),
            TypeExpr::Fn(..) => self.opaque_sort("Fn"),
            TypeExpr::Refined(..) => unreachable!("refinements are looked through"),
            TypeExpr::Union(ms) => {
                let groups = self.union_groups(ms)?;
                let sorts: Vec<String> = groups.iter().map(|g| self.field_sort(&g.1)).collect();
                let name = format!("T:Union({})", sorts.join(",").replace('|', ""));
                self.datatype(&name, |_| sorts.iter().enumerate().map(|(k, s)| (format!("{name}.{k}"), vec![s.clone()])).collect())
            }
            TypeExpr::Named(n, args) => match n.as_str() {
                "Int" | "Bool" | "String" => n.to_string(),
                "Float" | "Dict" | "Set" | "_" => self.opaque_sort(n),
                "List" => match args.first() {
                    Some(a) => {
                        let item = self.field_sort(a);
                        let name = format!("T:List({})", item.replace('|', ""));
                        if !self.sorts.index.contains_key(&name) {
                            self.sorts.index.insert(name.clone(), self.sorts.defs.len());
                            self.sorts.defs.push((name.clone(), SortDef::List(item)));
                        }
                        sym(&name)
                    }
                    None => self.opaque_sort("List"),
                },
                "Tuple" => {
                    let name = dt_name(&t);
                    self.datatype(&name, |vc| vec![(format!("{name}.mk"), args.iter().map(|a| vc.field_sort(a)).collect())])
                }
                _ => match self.env.variants_of(n, args) {
                    Some(vs) => {
                        let name = dt_name(&t);
                        self.datatype(&name, |vc| vs.iter().map(|(c, _, tys)| (format!("{name}.{c}"), tys.iter().map(|t| vc.field_sort(t)).collect())).collect())
                    }
                    None => self.opaque_sort(&crate::printer::ty(&t)),
                },
            },
        })
    }

    /// The members of a union grouped by shape: each group's key, the type
    /// of its slot, and (for a sum's variants) the constructors it allows.
    /// `None` when two members could have the same shape in a way the slots
    /// can't tell apart (`List[Int] | List[String]`), or a member's shape is
    /// not known.
    pub(super) fn union_groups(&self, ms: &[TypeExpr]) -> Option<Vec<(String, TypeExpr, Option<Vec<String>>)>> {
        let mut members = vec![];
        for m in ms {
            self.flatten_union(m, &mut members);
        }
        let mut groups: Vec<(String, Vec<TypeExpr>)> = vec![];
        for m in members {
            let k = self.type_key(&m)?;
            match groups.iter_mut().find(|g| g.0 == k) {
                Some(g) => g.1.push(m),
                None => groups.push((k, vec![m])),
            }
        }
        let mut out = vec![];
        for (k, ms) in groups {
            let heads: Vec<TypeExpr> = ms.iter().map(|m| self.env.expand_alias(m)).collect();
            if let Some(sum) = k.strip_prefix("sum:") {
                // the sum itself or some of its variants: a value of the sum,
                // whose constructor is one of those variants
                let mut args: Option<Vec<TypeExpr>> = None;
                let mut ctors: Option<Vec<String>> = Some(vec![]);
                for h in &heads {
                    let TypeExpr::Named(n, a) = h else { return None };
                    if args.get_or_insert_with(|| a.clone()) != a {
                        return None;
                    }
                    if n == sum {
                        ctors = None;
                    } else if let Some(cs) = &mut ctors {
                        cs.push(n.to_string());
                    }
                }
                let t = TypeExpr::Named(Symbol::from(sum), args.unwrap_or_default());
                out.push((k.clone(), t, ctors));
            } else if ms.len() == 1 {
                out.push((k.clone(), ms[0].clone(), None));
            } else {
                match k.as_str() {
                    // `Positive | Negative`: both are integers
                    "Int" | "Bool" | "String" | "Unit" => out.push((k.clone(), TypeExpr::Named(Symbol::from(k.as_str()), vec![]), None)),
                    _ => return None,
                }
            }
        }
        Some(out)
    }

    /// The constructors of a datatype sort.
    fn ctors_of(&self, sort: &str) -> Vec<(String, Vec<String>)> {
        self.sorts.data(sort).cloned().unwrap_or_default()
    }

    /// "`t` is a valid value of `ty`": the predicate of `ty`'s refinements
    /// applied to the term.
    pub(super) fn valid_term(&mut self, ty: &TypeExpr, t: &str) -> T {
        let Some(sort) = self.sort_of(ty) else { return self.unknown_bool("a value of an unknown shape") };
        let key = format!("{} : {sort}", crate::printer::ty(&self.erased(ty)));
        let i = match self.sorts.valid_index.get(&key) {
            Some(&i) => i,
            None => {
                let i = self.sorts.valid.len();
                self.sorts.valid.push((format!("|valid{i}|"), sort));
                self.sorts.valid_index.insert(key, i);
                i
            }
        };
        format!("({} {t})", self.sorts.valid[i].0)
    }

    /// A fact that holds of a term whatever the path (what its type
    /// guarantees, the definition of its validity). Inside a scope, or with
    /// placeholders, it stays with what is made there.
    fn define(&mut self, fact: T, term: &str) {
        if fact == "true" {
            return;
        }
        if !self.regions.is_empty() || term.contains("@@") {
            self.facts.push(fact);
        } else {
            self.defs.push(fact);
        }
    }

    /// The value a term of type `ty` stands for: a plain value for `Int`,
    /// `Bool`, `String`; a list for a sequence; a `Val::Data` for a
    /// structured value (unfolded when read).
    pub(super) fn from_term(&mut self, ty: &TypeExpr, t: T) -> Val {
        match ty {
            TypeExpr::Refined(b, _) => self.from_term(b, t),
            TypeExpr::Unit => Val::Unit,
            TypeExpr::Fn(_, ret) => Val::FnOf { ret: (**ret).clone() },
            TypeExpr::Union(_) => match self.sort_of(ty) {
                Some(_) => Val::Data { ty: ty.clone(), t },
                None => Val::Opaque("values of this union type are not modelled (members of overlapping or unknown shapes)".into()),
            },
            TypeExpr::Named(n, args) => match n.as_str() {
                "Int" => Val::Int(t),
                "Bool" => Val::Bool(t),
                "String" => {
                    if !self.str_terms.contains(&t) {
                        self.str_terms.push(t.clone());
                    }
                    Val::Str(t)
                }
                "Float" => Val::Opaque("floats are not modelled yet".into()),
                "List" => match args.first() {
                    Some(a) => {
                        let name = self.sort_of(ty).unwrap_or_default().replace('|', "");
                        let len = list_len(&name, &t);
                        self.define(format!("(>= {len} 0)"), &t);
                        self.new_list(len, Gen::Seq(t, a.clone(), name))
                    }
                    None => Val::Opaque("a list of unknown elements".into()),
                },
                "Dict" | "Set" => {
                    let l = self.fresh_why("Int", "dicts and sets are not modelled yet");
                    self.facts.push(format!("(>= {l} 0)"));
                    self.new_list(l, Gen::Unknown)
                }
                "Task" if args.len() == 1 => self.from_term(&args[0], t),
                "Tuple" => Val::Data { ty: ty.clone(), t },
                _ if self.env.variant_parent(n.sym()).is_some() => Val::Data { ty: ty.clone(), t },
                _ => match self.env.types.get(&n.sym()).cloned() {
                    None => Val::Opaque(format!("values of the generic type `{n}` are not known")),
                    Some(td) => {
                        let sub: HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                        match &td.body {
                            TypeBody::Alias(a) if td.distinct => Val::Distinct(n.to_string(), Box::new(self.from_term(&subst_texpr(a, &sub), t))),
                            TypeBody::Alias(a) => self.from_term(&subst_texpr(a, &sub), t),
                            TypeBody::Record(_) | TypeBody::Sum(_) => Val::Data { ty: ty.clone(), t },
                        }
                    }
                },
            },
        }
    }

    /// A fresh structured value of type `t`, unfolded (`None` when `t` has
    /// no datatype sort).
    pub(super) fn fresh_data(&mut self, t: &TypeExpr) -> Option<Val> {
        let sort = self.sort_of(t)?;
        let x = self.fresh(&sort);
        Some(self.unfold(t, &x))
    }

    /// One level of term `t` of type `ty`: a record's fields, a sum's tag and
    /// payloads, a tuple's items, a union's tag and slots, each the selector
    /// of `t`. The facts its type gives (a variant's constructor, its
    /// validity's definition) are stated the first time.
    pub(super) fn unfold(&mut self, ty: &TypeExpr, t: &T) -> Val {
        let key = format!("{} {t}", crate::printer::ty(ty));
        let global = self.regions.is_empty() && !t.contains("@@");
        if global {
            if let Some(v) = self.unfolded.get(&key) {
                return v.clone();
            }
        }
        let Some(sort) = self.sort_of(ty) else { return Val::Opaque("values of this type are not modelled".into()) };
        let cs = self.ctors_of(&sort);
        let sel = |c: &str, k: usize| format!("({} {t})", sym(&format!("{c}.{k}")));
        let tag = |n: usize| if n == 1 { "0".to_string() } else { format!("({} {t})", sym(&format!("{}.tag", sort.trim_matches('|')))) };
        let mut defs: Vec<T> = vec![];
        let v = match ty {
            TypeExpr::Refined(b, _) => return self.unfold(b, t),
            TypeExpr::Union(ms) => {
                let Some(groups) = self.union_groups(ms) else { return Val::Opaque("values of this union type are not modelled".into()) };
                let tg = tag(cs.len());
                let mut slots = vec![];
                for (i, ((k, st, only), (c, _))) in groups.iter().zip(&cs).enumerate() {
                    let s = sel(c, 0);
                    if let Some(only) = only {
                        let inner = self.sort_of(st).unwrap_or_default();
                        let iname = inner.trim_matches('|').to_string();
                        let is: Vec<T> = self.ctors_of(&inner).iter().filter(|(ic, _)| only.iter().any(|o| *ic == format!("{iname}.{o}"))).map(|(ic, _)| format!("((_ is {}) {s})", sym(ic))).collect();
                        defs.push(implies(&format!("(= {tg} {i})"), &or(&is)));
                    }
                    let x = self.from_term(st, s);
                    slots.push((k.clone(), x));
                }
                Val::Union { tag: tg, slots }
            }
            TypeExpr::Named(n, args) => {
                if let Some(parent) = self.env.variant_parent(n.sym()) {
                    let pt = TypeExpr::Named(parent, args.clone());
                    let v = self.unfold(&pt, t);
                    let pname = sort.trim_matches('|').to_string();
                    if let Some((c, _)) = cs.iter().find(|(c, _)| *c == format!("{pname}.{n}")) {
                        defs.push(format!("((_ is {}) {t})", sym(c)));
                    }
                    v
                } else if n == "Tuple" {
                    let c = cs.first().map(|c| c.0.clone()).unwrap_or_default();
                    Val::Tuple(args.iter().enumerate().map(|(k, a)| self.from_term(a, sel(&c, k))).collect())
                } else if n == "Task" && args.len() == 1 {
                    return self.unfold(&args[0], t);
                } else {
                    let Some(td) = self.env.types.get(&n.sym()).cloned() else { return Val::Opaque(format!("values of the generic type `{n}` are not known")) };
                    let sub: HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                    match &td.body {
                        TypeBody::Alias(a) if td.distinct => Val::Distinct(n.to_string(), Box::new(self.from_term(&subst_texpr(a, &sub), t.clone()))),
                        TypeBody::Alias(a) => return self.unfold(&subst_texpr(a, &sub), t),
                        TypeBody::Record(_) => {
                            let vs = self.env.variants_of(n, args).unwrap_or_default();
                            let (_, names, tys) = vs.into_iter().next().unwrap_or_default();
                            let c = cs.first().map(|c| c.0.clone()).unwrap_or_default();
                            let fs = names.unwrap_or_default().into_iter().zip(tys).enumerate().map(|(k, (f, ft))| (f.to_string(), self.from_term(&ft, sel(&c, k)))).collect();
                            Val::Rec(n.to_string(), fs)
                        }
                        TypeBody::Sum(_) => {
                            let vs = self.env.variants_of(n, args).unwrap_or_default();
                            let ctors = vs.iter().zip(&cs).map(|((v, _, tys), (c, _))| (v.to_string(), tys.iter().enumerate().map(|(k, ft)| self.from_term(ft, sel(c, k))).collect())).collect();
                            Val::Sum { ty: n.to_string(), args: args.clone(), tag: tag(cs.len()), ctors }
                        }
                    }
                }
            }
            TypeExpr::Unit | TypeExpr::Fn(..) => return self.from_term(ty, t.clone()),
        };
        if global {
            self.unfolded.insert(key, v.clone());
        }
        for d in defs {
            self.define(d, t);
        }
        if self.refined(ty, 0) {
            // what "valid" means for this term: its level checked, its parts by their own predicates
            let vt = self.valid_term(ty, t);
            if !global || self.defined.insert(vt.clone()) {
                let pc = std::mem::take(&mut self.pc);
                let checking = std::mem::take(&mut self.checking);
                let proving = std::mem::replace(&mut self.proving, 0);
                let binding = std::mem::replace(&mut self.binding, 0);
                let dead = std::mem::replace(&mut self.dead, false);
                self.spec += 1;
                let c = self.check_type(ty, &v, &[]);
                self.spec -= 1;
                self.dead = dead;
                self.binding = binding;
                self.proving = proving;
                self.checking = checking;
                self.pc = pc;
                self.define(format!("(= {vt} {c})"), t);
            }
        }
        v
    }

    /// Two terms of datatype values merged: the term that is either, and
    /// its validity, either's.
    pub(super) fn merge_data(&mut self, c: &str, (ta, a): (&TypeExpr, &T), (tb, b): (&TypeExpr, &T)) -> Option<Val> {
        if self.erased(ta) != self.erased(tb) || self.sort_of(ta) != self.sort_of(tb) {
            return None;
        }
        let t = ite(c, a, b);
        if self.refined(ta, 0) && t != *a {
            let (v, va, vb) = (self.valid_term(ta, &t), self.valid_term(ta, a), self.valid_term(ta, b));
            let fact = format!("(= {v} {})", ite(c, &va, &vb));
            self.define(fact, &t);
        }
        Some(Val::Data { ty: ta.clone(), t })
    }

    /// The literal a model's value of type `ty` stands for.
    pub(super) fn sx_literal(&self, ty: &TypeExpr, x: &Sx) -> Option<Expr> {
        let x = match x {
            Sx::List(xs) if xs.len() == 3 && xs[0].atom() == Some("as") => &xs[1],
            x => x,
        };
        Some(match ty {
            TypeExpr::Refined(b, _) => return self.sx_literal(b, x),
            TypeExpr::Unit => Expr::Unit,
            TypeExpr::Fn(..) => return None,
            TypeExpr::Union(ms) => {
                let groups = self.union_groups(ms)?;
                let (c, args) = x.call()?;
                let k: usize = c.rsplit_once('.')?.1.parse().ok()?;
                return self.sx_literal(&groups.get(k)?.1, args.first()?);
            }
            TypeExpr::Named(n, args) => match n.as_str() {
                "Int" => crate::llvm::native::int_expr(parse_big(&x.to_string())?),
                "Bool" => Expr::Bool(x.atom()? == "true"),
                "String" => Expr::Str(parse_str(&x.to_string())?),
                "List" => {
                    let a = args.first()?;
                    let mut out = vec![];
                    self.seq_items(a, x, &mut out)?;
                    Expr::List(out)
                }
                "Tuple" => {
                    let (_, xs) = x.call()?;
                    Expr::Tuple(args.iter().zip(xs).map(|(a, x)| self.sx_literal(a, x)).collect::<Option<_>>()?)
                }
                "Task" if args.len() == 1 => return self.sx_literal(&args[0], x),
                "Float" | "Dict" | "Set" => return some_literal(self.env, ty, 0),
                _ => {
                    if let Some(p) = self.env.variant_parent(n.sym()) {
                        return self.sx_literal(&TypeExpr::Named(p, args.clone()), x);
                    }
                    let td = self.env.types.get(&n.sym())?;
                    let sub: HashMap<Symbol, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
                    match &td.body {
                        TypeBody::Alias(a) if td.distinct => Expr::Ctor(n.clone(), CtorArgs::Positional(vec![self.sx_literal(&subst_texpr(a, &sub), x)?])),
                        TypeBody::Alias(a) => return self.sx_literal(&subst_texpr(a, &sub), x),
                        _ => {
                            let (c, xs) = x.call()?;
                            let name = dt_name(&self.sort_type(ty));
                            let c = c.strip_prefix(&format!("{name}."))?;
                            let vs = self.env.variants_of(n, args)?;
                            let (vname, names, tys) = vs.into_iter().find(|v| v.0 == c)?;
                            let fields: Vec<Expr> = tys.iter().zip(xs).map(|(ft, x)| self.sx_literal(ft, x)).collect::<Option<_>>()?;
                            Expr::Ctor(
                                vname,
                                match names {
                                    _ if fields.is_empty() => CtorArgs::None,
                                    Some(ns) => CtorArgs::Record(ns.into_iter().zip(fields).collect()),
                                    None => CtorArgs::Positional(fields),
                                },
                            )
                        }
                    }
                }
            },
        })
    }

    /// The items of a model's sequence (`seq.empty`, `seq.unit`, `seq.++`).
    fn seq_items(&self, elem: &TypeExpr, x: &Sx, out: &mut Vec<Expr>) -> Option<()> {
        match x {
            Sx::Atom(a) if a == "seq.empty" => Some(()),
            Sx::List(xs) if xs.len() == 3 && xs[0].atom() == Some("as") => self.seq_items(elem, &xs[1], out),
            Sx::List(xs) => match xs.first()?.atom()? {
                "seq.unit" => {
                    out.push(self.sx_literal(elem, xs.get(1)?)?);
                    Some(())
                }
                "seq.++" => xs[1..].iter().try_for_each(|x| self.seq_items(elem, x, out)),
                _ => None,
            },
            _ => None,
        }
    }
}

/// An s-expression of the solver's output.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Sx {
    /// A symbol (without its bars), a number, or a string literal as written.
    Atom(String),
    List(Vec<Sx>),
}

impl Sx {
    fn atom(&self) -> Option<&str> {
        match self {
            Sx::Atom(a) => Some(a),
            Sx::List(_) => None,
        }
    }

    /// A constructor applied (`(C x y)`), or a constructor alone (`C`).
    fn call(&self) -> Option<(&str, &[Sx])> {
        match self {
            Sx::Atom(a) => Some((a, &[])),
            Sx::List(xs) => Some((xs.first()?.atom()?, &xs[1..])),
        }
    }

    /// `let`s replaced by what they name.
    fn expand(&self, env: &mut Vec<(String, Sx)>) -> Sx {
        match self {
            Sx::Atom(a) => env.iter().rev().find(|(n, _)| n == a).map(|(_, v)| v.clone()).unwrap_or_else(|| self.clone()),
            Sx::List(xs) if xs.len() == 3 && xs[0].atom() == Some("let") => {
                let Sx::List(bs) = &xs[1] else { return self.clone() };
                let n = env.len();
                let mut new = vec![];
                for b in bs {
                    if let Sx::List(p) = b {
                        if let (Some(Sx::Atom(name)), Some(v)) = (p.first(), p.get(1)) {
                            new.push((name.clone(), v.expand(env)));
                        }
                    }
                }
                env.extend(new);
                let body = xs[2].expand(env);
                env.truncate(n);
                body
            }
            Sx::List(xs) => Sx::List(xs.iter().map(|x| x.expand(env)).collect()),
        }
    }
}

impl std::fmt::Display for Sx {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Sx::Atom(a) if a.starts_with('"') || a.chars().all(|c| c.is_alphanumeric() || "~!@$%^&*_-+=<>.?/".contains(c)) => write!(f, "{a}"),
            Sx::Atom(a) => write!(f, "{}", sym(a)),
            Sx::List(xs) => {
                write!(f, "(")?;
                for (i, x) in xs.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{x}")?;
                }
                write!(f, ")")
            }
        }
    }
}

/// The s-expressions of `s`, in order.
pub(super) fn parse_sx(s: &str) -> Vec<Sx> {
    let cs: Vec<char> = s.chars().collect();
    let mut stack: Vec<Vec<Sx>> = vec![vec![]];
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        match c {
            '(' => {
                stack.push(vec![]);
                i += 1;
            }
            ')' => {
                if stack.len() > 1 {
                    let l = stack.pop().unwrap();
                    stack.last_mut().unwrap().push(Sx::List(l));
                }
                i += 1;
            }
            '"' => {
                let start = i;
                i += 1;
                while i < cs.len() {
                    if cs[i] == '"' {
                        if cs.get(i + 1) == Some(&'"') {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i = (i + 1).min(cs.len());
                stack.last_mut().unwrap().push(Sx::Atom(cs[start..i].iter().collect()));
            }
            '|' => {
                let start = i + 1;
                i += 1;
                while i < cs.len() && cs[i] != '|' {
                    i += 1;
                }
                stack.last_mut().unwrap().push(Sx::Atom(cs[start..i.min(cs.len())].iter().collect()));
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            _ => {
                let start = i;
                while i < cs.len() && !cs[i].is_whitespace() && cs[i] != '(' && cs[i] != ')' {
                    i += 1;
                }
                stack.last_mut().unwrap().push(Sx::Atom(cs[start..i].iter().collect()));
            }
        }
    }
    while stack.len() > 1 {
        let l = stack.pop().unwrap();
        stack.last_mut().unwrap().push(Sx::List(l));
    }
    stack.pop().unwrap_or_default()
}

/// The values of a `get-value` answer (`((t1 v1) (t2 v2) …)`), by the terms
/// asked for, in order, `let`s expanded.
pub(super) fn model_values(out: &str, asked: &[String]) -> HashMap<String, Sx> {
    let mut m = HashMap::default();
    for x in parse_sx(out) {
        let Sx::List(pairs) = x else { continue };
        if !pairs.iter().all(|p| matches!(p, Sx::List(kv) if kv.len() == 2)) || pairs.len() != asked.len() {
            continue;
        }
        for (p, a) in pairs.iter().zip(asked) {
            if let Sx::List(kv) = p {
                m.insert(a.clone(), kv[1].expand(&mut vec![]));
            }
        }
        break;
    }
    m
}
