//! The environment: everything known about a program's declarations.

use super::*;

/// Everything known about the declarations of a program, shared by the
/// checker, interpreter and generators.
pub struct Env {
    pub types: HashMap<String, TypeDecl>,
    /// ctor name -> (type name)
    pub ctors: HashMap<String, String>,
    pub fns: HashMap<String, FnDecl>,
    pub traits: HashMap<String, TraitDecl>,
    /// Trait implementations (`impl Trait for Type`).
    pub impls: Vec<ImplDecl>,
    /// trait method name -> the traits that declare it
    pub method_traits: HashMap<String, Vec<String>>,
    /// type name -> its own methods (`impl Item { ... }`)
    pub inherent: HashMap<String, ImplDecl>,
}

impl Env {
    /// The environment of a program as it runs (elaborated).
    pub fn new(prog: &Program) -> (Env, Vec<Diag>) {
        Env::build(&elaborate(prog).program)
    }

    /// The environment of an already resolved program.
    pub fn build(prog: &Program) -> (Env, Vec<Diag>) {
        let prelude = prelude();
        let mut env = Env {
            types: HashMap::new(),
            ctors: HashMap::new(),
            fns: HashMap::new(),
            traits: HashMap::new(),
            impls: Vec::new(),
            method_traits: HashMap::new(),
            inherent: HashMap::new(),
        };
        let mut errs = Vec::new();
        let prelude_names: Vec<String> = prelude.decls.iter().map(|d| d.name()).collect();
        for d in prelude.decls.iter().chain(prog.decls.iter()) {
            let name = d.name();
            let dup = match &d.kind {
                DeclKind::Type(t) => {
                    match &t.body {
                        TypeBody::Sum(vs) => {
                            for v in vs {
                                // a clash with a record type is reported below, with the other name clashes
                                let prev = env.ctors.get(&v.name).cloned();
                                if prev.as_ref().is_some_and(|p| *p == v.name) {
                                    continue;
                                }
                                if env.ctors.insert(v.name.clone(), t.name.clone()).is_some() {
                                    errs.push(Diag { decl: name.clone(), msg: format!("constructor `{}` defined twice", v.name) });
                                }
                            }
                        }
                        TypeBody::Record(_) => {
                            env.ctors.insert(t.name.clone(), t.name.clone());
                        }
                        TypeBody::Alias(_) if t.distinct => {
                            env.ctors.insert(t.name.clone(), t.name.clone());
                        }
                        TypeBody::Alias(_) => {}
                    }
                    env.types.insert(t.name.clone(), t.clone()).is_some()
                }
                DeclKind::Fn(f) => env.fns.insert(f.name.clone(), f.clone()).is_some(),
                DeclKind::Test(_) | DeclKind::Use(_) => false,
                DeclKind::Trait(t) => {
                    for m in &t.methods {
                        env.method_traits.entry(m.decl.name.clone()).or_default().push(t.name.clone());
                    }
                    env.traits.insert(t.name.clone(), t.clone()).is_some()
                }
                DeclKind::Impl(i) if i.is_inherent() => {
                    let head = texpr_head(&i.for_type);
                    // `Item.new()` (no `self`) and, once elaborated, `item.describe()` on a known
                    // `Item` are calls of the functions `Item.new` / `Item.describe`
                    for m in &i.methods {
                        let f = static_fn(i, m);
                        env.fns.insert(f.name.clone(), f);
                    }
                    if env.inherent.insert(head.clone(), i.clone()).is_some() {
                        errs.push(Diag { decl: name.clone(), msg: format!("`{head}` has two `impl {head}` blocks: put all of its own methods in one") });
                    }
                    false
                }
                DeclKind::Impl(i) => {
                    let head = texpr_head(&i.for_type);
                    if env.impls.iter().any(|o| o.trait_name == i.trait_name && texpr_head(&o.for_type) == head) {
                        errs.push(Diag { decl: name.clone(), msg: format!("`{}` is already implemented for `{head}`", i.trait_name) });
                    }
                    env.impls.push(i.clone());
                    false
                }
            };
            if dup {
                let msg = if prelude_names.contains(&name) {
                    format!("`{name}` is already defined in the prelude (use it, or pick another name)")
                } else {
                    format!("`{name}` is defined twice")
                };
                errs.push(Diag { decl: name.clone(), msg });
            }
        }
        // a variant is a type too, so its name must be unique in the whole
        // project: no other variant, type or trait (in any module) may share it
        let short = |q: &str| crate::names::short(q).to_string();
        let mut owners: HashMap<String, Vec<String>> = HashMap::new();
        for n in env.types.keys().chain(env.traits.keys()) {
            owners.entry(short(n)).or_default().push(format!("the type `{n}`"));
        }
        let mut variants: Vec<(&String, &String)> = Vec::new();
        for t in env.types.values() {
            if let TypeBody::Sum(vs) = &t.body {
                variants.extend(vs.iter().map(|v| (&v.name, &t.name)));
            }
        }
        variants.sort();
        for (c, t) in &variants {
            owners.entry(short(c)).or_default().push(format!("the variant `{c}` of `{t}`"));
        }
        for (c, t) in &variants {
            let others: Vec<&String> = owners[&short(c)].iter().filter(|o| **o != format!("the variant `{c}` of `{t}`")).collect();
            if !others.is_empty() {
                let list = others.iter().map(|o| o.as_str()).collect::<Vec<_>>().join(", ");
                errs.push(Diag {
                    decl: (*t).clone(),
                    msg: format!("variant `{c}` of `{t}` has the same name as {list}: a variant is a type of its own, so its name must be unique across the project (rename one)"),
                });
            }
        }
        let mut sorted: Vec<_> = env.method_traits.iter().collect();
        sorted.sort();
        for (m, trs) in sorted {
            if env.fns.contains_key(m) {
                errs.push(Diag { decl: m.clone(), msg: format!("`{m}` is both a function and a method of trait `{}`", trs[0]) });
            }
        }
        let mut heads: Vec<_> = env.inherent.iter().collect();
        heads.sort_by_key(|(h, _)| *h);
        for (head, i) in heads {
            for m in i.methods.iter().filter(|m| crate::resolve::takes_self(m)) {
                if env.fns.contains_key(&m.name) {
                    errs.push(Diag { decl: i.name(), msg: format!("`{}` is both a function and a method of `{head}`", m.name) });
                }
            }
        }
        (env, errs)
    }

    /// The type parameters of `f` that must be known when it runs: those
    /// bounded by a trait (or a supertrait) with a method without `self`.
    pub fn type_args(&self, f: &FnDecl) -> Vec<String> {
        let has_static = |tr: &str| {
            let mut stack = vec![tr.to_string()];
            let mut seen = std::collections::HashSet::new();
            while let Some(t) = stack.pop() {
                if !seen.insert(t.clone()) {
                    continue;
                }
                let Some(td) = self.traits.get(&t) else { continue };
                if td.methods.iter().any(|m| !crate::resolve::takes_self(&m.decl)) {
                    return true;
                }
                stack.extend(td.supers.iter().map(texpr_head));
            }
            false
        };
        f.type_params.iter().filter(|p| f.bounds.iter().any(|b| &b.param == *p && b.traits.iter().any(|t| has_static(&texpr_head(t))))).cloned().collect()
    }

    /// The traits declaring `method` that values of type `head` implement.
    pub fn traits_with(&self, method: &str, head: &str) -> Vec<String> {
        let Some(trs) = self.method_traits.get(method) else { return vec![] };
        trs.iter().filter(|tr| self.impls.iter().any(|i| &i.trait_name == *tr && texpr_head(&i.for_type) == head)).cloned().collect()
    }

    /// Whether `name` is a method (of a trait, or of a type itself), or a
    /// call through a specific trait (`Trait::method`, see `elaborate`).
    pub fn is_method(&self, name: &str) -> bool {
        split_trait_call(name).is_some()
            || self.method_traits.contains_key(name) || self.inherent.values().any(|i| i.methods.iter().any(|m| m.name == name && crate::resolve::takes_self(m)))
    }

    /// The declaration of method `name` when it means the same thing on every
    /// type: declared by exactly one trait and by no type itself. Its
    /// signature and contract then hold for every call.
    pub fn method_contract(&self, name: &str) -> Option<(&TraitDecl, &FnDecl)> {
        if let Some((tr, m)) = split_trait_call(name) {
            let t = self.traits.get(tr)?;
            return Some((t, &t.methods.iter().find(|x| x.decl.name == m)?.decl));
        }
        match self.method_traits.get(name).map(|v| v.as_slice()) {
            Some([tr]) if !self.inherent.values().any(|i| i.methods.iter().any(|m| m.name == name)) => {
                let t = self.traits.get(tr)?;
                Some((t, &t.methods.iter().find(|m| m.decl.name == name)?.decl))
            }
            _ => None,
        }
    }

    /// The parameters of method `name` (for `mut`-ness: every declaration of
    /// a shared name must agree, which impls are checked for).
    pub fn method_params(&self, name: &str) -> Option<Vec<Param>> {
        if split_trait_call(name).is_some() {
            return self.method_contract(name).map(|(_, d)| d.params.clone());
        }
        let from_trait = self.method_traits.get(name).and_then(|trs| trs.first()).and_then(|t| self.traits.get(t)).and_then(|t| t.methods.iter().find(|m| m.decl.name == name)).map(|m| m.decl.params.clone());
        let mut owns: Vec<_> = self.inherent.iter().collect();
        owns.sort_by_key(|(h, _)| *h);
        from_trait.or_else(|| owns.into_iter().find_map(|(_, i)| i.methods.iter().find(|m| m.name == name && crate::resolve::takes_self(m)).map(|m| m.params.clone())))
    }

    /// Which parameters of the function or method `name` are `mut` (none for
    /// a builtin).
    pub fn mut_params(&self, name: &str) -> Vec<bool> {
        let ps = self.fns.get(name).map(|f| f.params.clone()).or_else(|| self.method_params(name));
        ps.unwrap_or_default().iter().map(|p| p.mutable).collect()
    }

    /// The implementation of `method` for values whose type head is `head`:
    /// the impl's method or the trait's default, with the trait's contract.
    /// `method` is `Trait::m` for a call through a specific trait (see
    /// `elaborate`), else the trait is the one of `head`'s traits declaring it.
    pub fn resolve_method(&self, method: &str, head: &str) -> Option<(FnDecl, Vec<Expr>, Vec<Expr>)> {
        let (tr, method) = match split_trait_call(method) {
            Some((tr, m)) => (tr.to_string(), m),
            None => match self.traits_with(method, head).as_slice() {
                [tr] => (tr.clone(), method),
                _ => return None,
            },
        };
        let tm = self.traits.get(&tr)?.methods.iter().find(|m| m.decl.name == method)?;
        let imp = self.impls.iter().find(|i| i.trait_name == tr && texpr_head(&i.for_type) == head)?;
        let decl = match imp.methods.iter().find(|m| m.name == method) {
            Some(m) => m.clone(),
            None if tm.default => tm.decl.clone(),
            None => return None,
        };
        // the trait's contract binds every implementation
        Some((decl, tm.decl.requires.clone(), tm.decl.ensures.clone()))
    }

    /// The sum type a variant belongs to (`Circle` -> `Shape`). A variant is a
    /// type of its own, with its sum's type parameters.
    pub fn variant_parent(&self, name: &str) -> Option<String> {
        let p = self.ctors.get(name)?;
        (p != name && matches!(self.types.get(p).map(|t| &t.body), Some(TypeBody::Sum(_)))).then(|| p.clone())
    }

    /// Whether `name` names a type (not only a constructor): a primitive, a
    /// declared type, a variant or a trait. Such a name in a pattern matches
    /// the values of that type.
    pub fn is_type_name(&self, name: &str) -> bool {
        matches!(name, "Int" | "Float" | "Bool" | "String" | "Unit" | "List" | "Set" | "Dict")
            || self.types.contains_key(name)
            || self.traits.contains_key(name)
            || self.variant_parent(name).is_some()
    }

    /// Follow refinement aliases to the underlying (unrefined) type expression.
    pub fn expand_alias(&self, t: &TypeExpr) -> TypeExpr {
        if let TypeExpr::Refined(b, _) = t {
            return self.expand_alias(b);
        }
        if let TypeExpr::Named(n, args) = t {
            if let Some(TypeDecl { body: TypeBody::Alias(a), params, distinct: false, .. }) = self.types.get(n) {
                let sub: HashMap<String, TypeExpr> = params.iter().cloned().zip(args.iter().cloned()).collect();
                return self.expand_alias(&subst_texpr(a, &sub));
            }
        }
        t.clone()
    }

    /// Constructors of a named type with type args substituted: (ctor, field names, field types)
    pub fn variants_of(&self, name: &str, args: &[TypeExpr]) -> Option<Vec<(String, Option<Vec<String>>, Vec<TypeExpr>)>> {
        let td = self.types.get(name)?;
        let sub: HashMap<String, TypeExpr> = td.params.iter().cloned().zip(args.iter().cloned()).collect();
        let s = |t: &TypeExpr| subst_texpr(t, &sub);
        match &td.body {
            TypeBody::Sum(vs) => Some(
                vs.iter()
                    .map(|v| match &v.payload {
                        Payload::None => (v.name.clone(), None, vec![]),
                        Payload::Positional(ts) => (v.name.clone(), None, ts.iter().map(s).collect()),
                        Payload::Record(fs) => (
                            v.name.clone(),
                            Some(fs.iter().map(|f| f.name.clone()).collect()),
                            fs.iter().map(|f| s(&f.ty)).collect(),
                        ),
                    })
                    .collect(),
            ),
            TypeBody::Record(fs) => Some(vec![(
                name.to_string(),
                Some(fs.iter().map(|f| f.name.clone()).collect()),
                fs.iter().map(|f| s(&f.ty)).collect(),
            )]),
            TypeBody::Alias(a) if td.distinct => Some(vec![(name.to_string(), None, vec![s(a)])]),
            TypeBody::Alias(_) => None,
        }
    }
}
