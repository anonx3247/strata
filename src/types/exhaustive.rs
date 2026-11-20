//! Exhaustiveness and unreachable arms (Maranget's usefulness algorithm).
//!
//! A pattern stands for a set of values. A type is split into "atoms": its
//! constructors (variants, records, `true`/`false`, `[]`/`::`, tuples), the
//! members of a union, and, for `Int`, the intervals that the patterns' bounds
//! cut it into. Anything else is one opaque atom.

use super::*;

/// The set of values a pattern matches.
#[derive(Clone, Debug)]
pub enum P {
    Any,
    /// A constructor, with patterns for its fields.
    C(String, Vec<P>),
    /// Some values we cannot describe (a string literal, an undecidable
    /// condition): it covers no atom fully.
    Lit,
    /// Every value of a type (`String`, `Circle`, `Shape` the trait).
    Ty(Ty),
    /// These integers (a literal, `where 3 <= it && it <= 5`, `Positive`).
    Ints(ints::Ints),
    /// Strings or lists with these lengths (`where len(it) > 0`, `NonEmpty`).
    Lens(ints::Ints),
    /// These floats (`where 0.0 <= it && it < 1.0`).
    Reals(reals::Reals),
    /// Any of these.
    Or(Vec<P>),
}

/// Rows with alternatives in their first column become one row per alternative.
pub(super) fn expand(rows: &[Vec<P>]) -> Vec<Vec<P>> {
    let mut out = Vec::new();
    for r in rows {
        match &r[0] {
            P::Or(ps) => out.extend(expand(&ps.iter().map(|p| [vec![p.clone()], r[1..].to_vec()].concat()).collect::<Vec<_>>())),
            _ => out.push(r.clone()),
        }
    }
    out
}

#[derive(Clone, PartialEq)]
pub(super) enum AtomKey {
    Ctor(String),
    Region(i128, i128),
    /// Strings whose length is in this range.
    Len(i128, i128),
    /// Floats: a single point, or the open interval between two (±inf).
    Real { lo: f64, hi: f64, point: bool },
    Opaque(String),
}

impl AtomKey {
    /// A float inside the region, to test a condition on.
    pub(super) fn sample(&self) -> f64 {
        match self {
            AtomKey::Real { lo, point: true, .. } => *lo,
            AtomKey::Real { lo, hi, .. } => match (lo.is_infinite(), hi.is_infinite()) {
                (true, true) => 0.0,
                (true, false) => hi - 1.0,
                (false, true) => lo + 1.0,
                _ => (lo + hi) / 2.0,
            },
            _ => 0.0,
        }
    }
}

/// Elementary integer regions cut by the sets' bounds, within [from, MAX].
pub(super) fn regions(sets: &[&ints::Ints], from: i128) -> Vec<(i128, i128)> {
    let mut cuts: Vec<i128> = vec![from];
    for s in sets {
        for (lo, hi) in s.iter() {
            cuts.push(*lo);
            cuts.push(hi + 1);
        }
    }
    cuts.retain(|c| *c >= from && *c <= ints::MAX);
    cuts.sort();
    cuts.dedup();
    cuts.iter().enumerate().map(|(i, lo)| (*lo, cuts.get(i + 1).map_or(ints::MAX, |n| n - 1))).collect()
}

pub(super) struct Atom {
    key: AtomKey,
    args: Vec<Ty>,
    ty: Ty,
}

impl Checker<'_> {
    /// The atoms of type `t`, with `Int` cut by the integer sets in `col`.
    pub(super) fn atoms(&mut self, t: &Ty, col: &[&P]) -> Vec<Atom> {
        let t = self.resolve(t);
        let atom = |k: AtomKey, args: Vec<Ty>, ty: &Ty| Atom { key: k, args, ty: ty.clone() };
        let ctor = |c: &str| AtomKey::Ctor(c.to_string());
        match &t {
            Ty::Bool => vec![atom(ctor("true"), vec![], &t), atom(ctor("false"), vec![], &t)],
            Ty::Unit => vec![atom(ctor("()"), vec![], &t)],
            Ty::Int => {
                let sets: Vec<&ints::Ints> = col.iter().filter_map(|p| if let P::Ints(s) = p { Some(s) } else { None }).collect();
                regions(&sets, ints::MIN).into_iter().map(|(lo, hi)| atom(AtomKey::Region(lo, hi), vec![], &t)).collect()
            }
            Ty::Str => {
                let sets: Vec<&ints::Ints> = col.iter().filter_map(|p| if let P::Lens(s) = p { Some(s) } else { None }).collect();
                regions(&sets, 0).into_iter().map(|(lo, hi)| atom(AtomKey::Len(lo, hi), vec![], &t)).collect()
            }
            Ty::Float => {
                let mut cuts: Vec<f64> = col.iter().filter_map(|p| if let P::Reals(r) = p { Some(r.cuts.clone()) } else { None }).flatten().collect();
                cuts.sort_by(|a, b| a.partial_cmp(b).unwrap());
                cuts.dedup();
                let mut out = Vec::new();
                let mut lo = f64::NEG_INFINITY;
                for c in &cuts {
                    out.push(atom(AtomKey::Real { lo, hi: *c, point: false }, vec![], &t));
                    out.push(atom(AtomKey::Real { lo: *c, hi: *c, point: true }, vec![], &t));
                    lo = *c;
                }
                out.push(atom(AtomKey::Real { lo, hi: f64::INFINITY, point: false }, vec![], &t));
                out
            }
            Ty::Union(ms) => ms.iter().flat_map(|m| self.atoms(m, col)).collect(),
            Ty::Con(n, args) if n == "Tuple" => vec![atom(ctor("(,)"), args.clone(), &t)],
            Ty::Con(n, args) if n == "List" => vec![atom(ctor("[]"), vec![], &t), atom(ctor("::"), vec![args[0].clone(), t.clone()], &t)],
            Ty::Con(n, args) if !self.env.traits.contains_key(n) && (self.env.types.contains_key(n) || self.env.variant_parent(n.sym()).is_some()) => {
                let ctors: Vec<Symbol> = match self.env.types.get(n).map(|td| &td.body) {
                    Some(TypeBody::Sum(vs)) => vs.iter().map(|v| v.name).collect(),
                    _ => vec![*n],
                };
                let parent = self.env.variant_parent(n.sym()).unwrap_or(*n);
                ctors
                    .into_iter()
                    .filter_map(|c| {
                        let (ct, fields) = self.ctor_type(c.sym())?;
                        let _ = self.unify(&ct, &Ty::Con(parent.sym(), args.clone()));
                        let fs = fields.into_iter().map(|f| self.resolve(&f.1)).collect();
                        let ty = self.variant_ty(c.sym(), &ct);
                        Some(atom(AtomKey::Ctor(c.to_string()), fs, &self.resolve(&ty)))
                    })
                    .collect()
            }
            _ => vec![atom(AtomKey::Opaque(show_ty(&t)), vec![], &t)],
        }
    }

    /// The patterns for an atom's fields if `p` covers the whole atom.
    pub(super) fn covers(&mut self, p: &P, a: &Atom) -> Option<Vec<P>> {
        let any = || vec![P::Any; a.args.len()];
        match p {
            P::Any => Some(any()),
            P::C(name, subs) => (a.key == AtomKey::Ctor(name.clone())).then(|| subs.clone()),
            P::Lit | P::Or(_) => None,
            P::Ints(s) => match a.key {
                AtomKey::Region(lo, hi) if ints::contains(s, lo, hi) => Some(vec![]),
                _ => None,
            },
            P::Lens(s) => match &a.key {
                AtomKey::Len(lo, hi) if ints::contains(s, *lo, *hi) => Some(vec![]),
                // a list is `[]` (length 0) or `x :: rest` (length 1 and more)
                AtomKey::Ctor(c) if c == "[]" && ints::contains(s, 0, 0) => Some(vec![]),
                AtomKey::Ctor(c) if c == "::" && ints::contains(s, 1, ints::MAX) => Some(any()),
                _ => None,
            },
            P::Reals(r) => (matches!(a.key, AtomKey::Real { .. }) && r.holds(a.key.sample())).then(Vec::new),
            P::Ty(pt) => {
                let pt = self.resolve(pt);
                if let Ty::Con(tr, targs) = &pt {
                    if self.env.traits.contains_key(tr) {
                        let snap = (self.subst.clone(), self.pending.len());
                        let ok = self.satisfies(&a.ty, tr, targs, 0).is_ok();
                        self.subst = snap.0;
                        self.pending.truncate(snap.1);
                        return ok.then(any);
                    }
                }
                let inside = self.atoms(&pt, &[]).iter().any(|b| match (&a.key, &b.key) {
                    (AtomKey::Region(..), AtomKey::Region(..)) | (AtomKey::Len(..), AtomKey::Len(..)) | (AtomKey::Real { .. }, AtomKey::Real { .. }) => true,
                    (x, y) => x == y,
                });
                inside.then(any)
            }
        }
    }

    pub(super) fn show_atom(&self, a: &Atom, args: &[String], in_union: bool) -> String {
        match &a.key {
            AtomKey::Opaque(k) => if in_union { k.clone() } else { "_".into() },
            AtomKey::Len(lo, hi) => match (*lo, *hi) {
                (0, ints::MAX) => if in_union { "String".into() } else { "_".into() },
                (l, h) if l == h => format!("where len(it) == {l}"),
                (l, ints::MAX) => format!("where len(it) >= {l}"),
                (0, h) => format!("where len(it) <= {h}"),
                (l, h) => format!("where {l} <= len(it) && len(it) <= {h}"),
            },
            AtomKey::Real { lo, hi, point } => match (*point, lo.is_infinite(), hi.is_infinite()) {
                (true, ..) => format!("where it == {lo:?}"),
                (false, true, true) => if in_union { "Float".into() } else { "_".into() },
                (false, true, false) => format!("where it < {hi:?}"),
                (false, false, true) => format!("where it > {lo:?}"),
                _ => format!("where {lo:?} < it && it < {hi:?}"),
            },
            AtomKey::Region(lo, hi) => {
                let full = *lo == ints::MIN && *hi == ints::MAX;
                match (*lo == ints::MIN, *hi == ints::MAX) {
                    _ if full => if in_union { "Int".into() } else { "_".into() },
                    _ if lo == hi => lo.to_string(),
                    (true, false) => format!("where it <= {hi}"),
                    (false, true) => format!("where it >= {lo}"),
                    _ => format!("where {lo} <= it && it <= {hi}"),
                }
            }
            AtomKey::Ctor(c) => {
                let n = args.len();
                let named = self.env.ctors.get(&c.sym()).and_then(|t| self.env.variants_of(t, &[])).and_then(|vs| vs.into_iter().find(|v| &v.0 == c)).and_then(|v| v.1);
                match c.as_str() {
                    "::" => format!("[{}, ..]", args[0]),
                    "(,)" => format!("({})", args.join(", ")),
                    _ if n == 0 => c.clone(),
                    _ if args.iter().all(|a| a == "_") => c.clone(),
                    _ if named.is_some() => {
                        let ns = named.unwrap();
                        let parts: Vec<String> = ns.iter().zip(args).filter(|(_, a)| *a != "_").map(|(n, a)| format!("{n}: {a}")).collect();
                        format!("{c} {{ {}, .. }}", parts.join(", "))
                    }
                    _ => format!("{c}({})", args.join(", ")),
                }
            }
        }
    }

    /// A value (as pattern text, one per column) that no row matches, if any.
    pub(super) fn missing(&mut self, rows: &[Vec<P>], tys: &[Ty], depth: usize) -> Option<Vec<String>> {
        if tys.is_empty() {
            return if rows.is_empty() { Some(vec![]) } else { None };
        }
        if depth > 12 {
            return None;
        }
        let rows = &expand(rows);
        // only wildcards here: every value of this column behaves the same
        if rows.iter().all(|r| matches!(r[0], P::Any)) {
            let def: Vec<Vec<P>> = rows.iter().map(|r| r[1..].to_vec()).collect();
            return self.missing(&def, &tys[1..], depth + 1).map(|w| [vec!["_".to_string()], w].concat());
        }
        let t0 = self.resolve(&tys[0]);
        let in_union = matches!(t0, Ty::Union(_));
        let col: Vec<P> = rows.iter().map(|r| r[0].clone()).collect();
        let atoms = self.atoms(&t0, &col.iter().collect::<Vec<_>>());
        for a in atoms {
            let n = a.args.len();
            let spec: Vec<Vec<P>> = rows.iter().filter_map(|r| self.covers(&r[0], &a).map(|subs| [subs, r[1..].to_vec()].concat())).collect();
            let tys2 = [a.args.clone(), tys[1..].to_vec()].concat();
            if let Some(w) = self.missing(&spec, &tys2, depth + 1) {
                let (args, rest) = w.split_at(n);
                let shown = self.show_atom(&a, args, in_union);
                return Some([vec![shown], rest.to_vec()].concat());
            }
        }
        None
    }

    /// Whether the row `q` matches some value that none of `rows` matches.
    pub(super) fn useful(&mut self, rows: &[Vec<P>], q: &[P], tys: &[Ty], depth: usize) -> bool {
        if tys.is_empty() {
            return rows.is_empty();
        }
        if depth > 12 || matches!(q[0], P::Lit) {
            return true;
        }
        if let P::Or(alts) = &q[0] {
            return alts.iter().any(|a| self.useful(rows, &[vec![a.clone()], q[1..].to_vec()].concat(), tys, depth));
        }
        let rows = &expand(rows);
        if matches!(q[0], P::Any) && rows.iter().all(|r| matches!(r[0], P::Any)) {
            let def: Vec<Vec<P>> = rows.iter().map(|r| r[1..].to_vec()).collect();
            return self.useful(&def, &q[1..], &tys[1..], depth + 1);
        }
        let mut col: Vec<P> = rows.iter().map(|r| r[0].clone()).collect();
        col.push(q[0].clone());
        let atoms = self.atoms(&tys[0], &col.iter().collect::<Vec<_>>());
        for a in atoms {
            let Some(qs) = self.covers(&q[0], &a) else {
                // part of an opaque type (a `Log` among a trait's values), or some of
                // a list's lengths: it may match something
                if matches!(a.key, AtomKey::Opaque(_)) && !matches!(q[0], P::Any) {
                    return true;
                }
                if matches!(q[0], P::Lens(_)) && matches!(&a.key, AtomKey::Ctor(c) if c == "::") {
                    return true;
                }
                continue;
            };
            let spec: Vec<Vec<P>> = rows.iter().filter_map(|r| self.covers(&r[0], &a).map(|subs| [subs, r[1..].to_vec()].concat())).collect();
            let tys2 = [a.args.clone(), tys[1..].to_vec()].concat();
            if self.useful(&spec, &[qs, q[1..].to_vec()].concat(), &tys2, depth + 1) {
                return true;
            }
        }
        false
    }

    pub(super) fn check_matches(&mut self) {
        let ms = std::mem::take(&mut self.matches);
        for Coverage { ty: t, arms, excluded } in ms {
            // the values the refinement rules out count as covered, by a hidden first row
            let pre: Vec<Vec<P>> = excluded.iter().map(|(p, _)| vec![p.clone()]).collect();
            let rows: Vec<Vec<P>> = pre.iter().cloned().chain(arms.iter().map(|(p, _)| vec![p.clone()])).collect();
            let k = pre.len();
            for (i, (_, text)) in arms.iter().enumerate() {
                if !self.useful(&rows[..k + i], &rows[k + i], &[t.clone()], 0) {
                    match &excluded {
                        Some((_, w)) if !self.useful(&pre, &rows[k + i], &[t.clone()], 0) => {
                            self.err(format!("unreachable match arm `{text}`: no `{w}` matches it"));
                        }
                        _ => self.err(format!("unreachable match arm `{text}`: the arms above already cover it")),
                    }
                }
            }
            if let Some(w) = self.missing(&rows, &[t.clone()], 0) {
                let shown = excluded.as_ref().map_or(self.show(&t), |(_, w)| w.clone());
                self.err(format!("non-exhaustive match on `{shown}`: `{}` is not covered", w[0]));
            }
        }
    }
}
