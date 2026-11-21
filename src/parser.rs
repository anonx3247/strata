use crate::ast::*;
use crate::lexer::{lex_with_comments, Comment, FPart, Tok, Token};

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
    no_struct: bool,
    comments: Vec<Comment>,
    used: Vec<bool>,
}

type R<T> = Result<T, String>;

pub fn parse_program(src: &str) -> R<Program> {
    parse_module_file(src, "")
}

/// Parse a file as the given module (e.g. `orders.strata` -> `orders`): a
/// file is one module, and every definition in it belongs to that module.
pub fn parse_module_file(src: &str, module: &str) -> R<Program> {
    let mut p = Parser::new(src)?;
    let mut decls = Vec::new();
    while p.peek() != &Tok::Eof {
        let mut d = p.decl()?;
        d.module = module.into();
        decls.push(d);
    }
    p.check_comments()?;
    Ok(Program { decls })
}

#[allow(dead_code)] // used by upcoming REPL / agent API
pub fn parse_expr(src: &str) -> R<Expr> {
    let mut p = Parser::new(src)?;
    let e = p.expr()?;
    if p.peek() != &Tok::Eof {
        return Err(p.err("trailing input after expression"));
    }
    Ok(e)
}

fn is_upper(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_uppercase())
}

const KEYWORDS: &[&str] = &[
    "type", "fn", "test", "let", "if", "else", "match", "mock", "where", "mut", "while", "for", "in", "loop", "break", "continue",
    "requires", "ensures", "terminates", "true", "false", "is", "assert", "trait", "impl", "law", "for", "pub", "use", "return", "except", "or", "comptime",
];

impl Parser {
    pub fn from_str(src: &str) -> Parser {
        Parser::new(src).expect("lexes")
    }
    fn new(src: &str) -> R<Parser> {
        let (toks, comments) = lex_with_comments(src)?;
        let used = vec![false; comments.len()];
        Ok(Parser { toks, pos: 0, no_struct: false, comments, used })
    }

    // ---------- comments ----------

    /// Take the comments between the previous token and the next one, as
    /// lines with "" wherever a blank line separates them from what follows.
    fn leading(&mut self) -> Vec<String> {
        let next = (self.toks[self.pos].line, self.toks[self.pos].col);
        let prev = if self.pos > 0 { Some((self.toks[self.pos - 1].line, self.toks[self.pos - 1].col)) } else { None };
        let mut out = Vec::new();
        let mut last_line = None;
        for (i, c) in self.comments.iter().enumerate() {
            if self.used[i] || prev.is_some_and(|p| (c.line, c.col) < p) || (c.line, c.col) > next {
                continue;
            }
            if let Some(l) = last_line {
                if c.line > l + 1 {
                    out.push(String::new());
                }
            }
            out.push(c.text.clone());
            last_line = Some(c.line);
            self.used[i] = true;
        }
        if last_line.is_some_and(|l| next.0 > l + 1) {
            out.push(String::new());
        }
        out
    }

    /// Whether a blank line separates the previous token from what comes next
    /// (the next token, or a comment before it).
    fn blank_line_before(&self) -> bool {
        let Some(prev) = self.pos.checked_sub(1).map(|i| &self.toks[i]) else { return false };
        let next = &self.toks[self.pos];
        let first = self
            .comments
            .iter()
            .zip(&self.used)
            .filter(|(c, u)| !**u && (c.line, c.col) > (prev.line, prev.col) && (c.line, c.col) < (next.line, next.col))
            .map(|(c, _)| c.line)
            .min()
            .unwrap_or(next.line);
        first > prev.line + 1
    }

    /// Take a comment that ends the line of the token just consumed (`x = 1 // why`).
    fn trailing(&mut self) -> Vec<String> {
        let Some(prev) = self.pos.checked_sub(1).map(|i| &self.toks[i]) else { return vec![] };
        let (line, col) = (prev.line, prev.col);
        let mut out = Vec::new();
        for (i, c) in self.comments.iter().enumerate() {
            if !self.used[i] && c.trailing && c.line == line && c.col > col {
                out.push(c.text.clone());
                self.used[i] = true;
            }
        }
        out
    }

    /// Every comment must have found a place in the tree, or it would be lost.
    fn check_comments(&self) -> R<()> {
        match self.comments.iter().zip(&self.used).find(|(_, u)| !**u) {
            Some((c, _)) => Err(format!(
                "{}:{}: this comment would be lost: put comments on their own line before a definition, field, variant, method, statement or match arm",
                c.line, c.col
            )),
            None => Ok(()),
        }
    }
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }
    fn peek_at(&self, n: usize) -> &Tok {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].tok
    }
    fn nl_before(&self) -> bool {
        self.toks[self.pos].nl_before
    }
    fn next(&mut self) -> Tok {
        let t = self.toks[self.pos].tok.clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn err(&self, msg: &str) -> String {
        let t = &self.toks[self.pos];
        format!("{}:{}: {} (found {:?})", t.line, t.col, msg, t.tok)
    }
    fn is_sym(&self, s: &str) -> bool {
        matches!(self.peek(), Tok::Sym(x) if *x == s)
    }
    fn is_kw(&self, s: &str) -> bool {
        matches!(self.peek(), Tok::Ident(x) if x == s)
    }
    fn eat_sym(&mut self, s: &str) -> bool {
        if self.is_sym(s) {
            self.next();
            true
        } else {
            false
        }
    }
    fn eat_kw(&mut self, s: &str) -> bool {
        if self.is_kw(s) {
            self.next();
            true
        } else {
            false
        }
    }
    fn expect_sym(&mut self, s: &str) -> R<()> {
        if self.eat_sym(s) { Ok(()) } else { Err(self.err(&format!("expected `{s}`"))) }
    }
    fn expect_kw(&mut self, s: &str) -> R<()> {
        if self.eat_kw(s) { Ok(()) } else { Err(self.err(&format!("expected `{s}`"))) }
    }
    fn ident(&mut self) -> R<Symbol> {
        match self.peek().clone() {
            Tok::Ident(s) if !KEYWORDS.contains(&s.as_str()) => {
                self.next();
                Ok(s)
            }
            _ => Err(self.err("expected identifier")),
        }
    }

    /// Parse a comma-separated list until `close`.
    fn list<T>(&mut self, close: &str, mut f: impl FnMut(&mut Self) -> R<T>) -> R<Vec<T>> {
        let mut out = Vec::new();
        while !self.is_sym(close) {
            out.push(f(self)?);
            if !self.eat_sym(",") {
                break;
            }
        }
        self.expect_sym(close)?;
        Ok(out)
    }

    // ---------- declarations ----------

    /// If the tokens ahead are `lower(.lower)*.Upper`, consume them and return
    /// the qualified name (`shop.orders.Pending`). Field names are lowercase and
    /// constructors uppercase, so this can only be a module path.
    fn qualified_upper(&mut self) -> Option<Symbol> {
        let mut k = 0;
        let mut parts = Vec::new();
        loop {
            let Tok::Ident(seg) = self.peek_at(k).clone() else { return None };
            if KEYWORDS.contains(&seg.as_str()) {
                return None;
            }
            parts.push(seg.as_str());
            if is_upper(&seg) {
                break;
            }
            if !matches!(self.peek_at(k + 1), Tok::Sym(".")) {
                return None;
            }
            k += 2;
        }
        if parts.len() < 2 {
            return None;
        }
        for _ in 0..=k {
            self.next();
        }
        Some(parts.join(".").into())
    }

    fn qualified_upper_ahead(&self) -> bool {
        let mut k = 0;
        loop {
            match self.peek_at(k) {
                Tok::Ident(seg) if is_upper(seg) => return k > 0,
                Tok::Ident(_) if matches!(self.peek_at(k + 1), Tok::Sym(".")) => k += 2,
                _ => return false,
            }
        }
    }

    /// A dotted path: `shop.orders`, `orders.Line`.
    fn path(&mut self) -> R<Symbol> {
        let first = self.ident()?;
        if !(self.is_sym(".") && matches!(self.peek_at(1), Tok::Ident(_))) {
            return Ok(first);
        }
        let mut s = first.to_string();
        while self.is_sym(".") && matches!(self.peek_at(1), Tok::Ident(_)) {
            self.next();
            s.push('.');
            s.push_str(&self.ident()?);
        }
        Ok(s.into())
    }

    pub fn decl(&mut self) -> R<Decl> {
        let mut comments = self.leading();
        let mut d = self.decl_inner()?;
        comments.append(&mut d.comments);
        comments.extend(self.trailing());
        d.comments = comments;
        Ok(d)
    }

    fn decl_inner(&mut self) -> R<Decl> {
        let mut doc = Vec::new();
        while let Tok::Doc(d) = self.peek().clone() {
            doc.push(d);
            self.next();
        }
        // comments between the doc and the definition move above the doc
        let comments = self.leading();
        let public = self.eat_kw("pub");
        if self.eat_kw("use") {
            let mut path = self.ident()?.to_string();
            let mut items = Some(vec![]);
            while self.eat_sym(".") {
                if self.eat_sym("*") {
                    items = None;
                    break;
                }
                if self.eat_sym("{") {
                    items = Some(self.list("}", |p| p.ident())?);
                    break;
                }
                let seg = self.ident()?;
                // `use orders.Line` imports one item
                if seg.chars().next().is_some_and(|c| c.is_uppercase()) || !self.is_sym(".") {
                    items = Some(vec![seg]);
                    break;
                }
                path.push('.');
                path.push_str(&seg);
            }
            return Ok(Decl { comments, doc, kind: DeclKind::Use(UseDecl { path: path.into(), items }), module: Symbol::EMPTY, public });
        }
        // `distinct` is only a keyword at the start of a declaration
        let kind = if self.is_kw("distinct") && matches!(self.peek_at(1), Tok::Ident(t) if t == "type") {
            self.next();
            self.expect_kw("type")?;
            let mut t = self.type_decl()?;
            if !matches!(t.body, TypeBody::Alias(_)) {
                return Err(self.err("only an alias can be `distinct` (records and sum types already are)"));
            }
            t.distinct = true;
            DeclKind::Type(t)
        } else if self.eat_kw("type") {
            DeclKind::Type(self.type_decl()?)
        } else if self.is_kw("comptime") && matches!(self.peek_at(1), Tok::Ident(f) if f == "fn") {
            self.next();
            self.next();
            let mut f = self.fn_decl()?;
            f.comptime = true;
            DeclKind::Fn(f)
        } else if self.is_kw("derivable") && matches!(self.peek_at(1), Tok::Sym("(")) {
            // `derivable(t: Record) trait ...`: contextual, like `distinct`
            self.next();
            self.next();
            let name = self.ident()?;
            self.expect_sym(":")?;
            let kind = self.type_expr()?;
            self.expect_sym(")")?;
            self.expect_kw("trait")?;
            DeclKind::Trait(self.trait_decl(Some((name, kind)))?)
        } else if self.is_kw("derive") && matches!(self.peek_at(1), Tok::Ident(n) if !KEYWORDS.contains(&n.as_str())) {
            // `derive Trait for Type`, with `{ ... }` for methods written by hand
            self.next();
            DeclKind::Impl(self.impl_rest(true)?)
        } else if self.is_kw("effect") {
            return Err(self.err("`effect` was removed: declare a `trait` and pass a value that implements it"));
        } else if self.eat_kw("fn") {
            DeclKind::Fn(self.fn_decl()?)
        } else if self.eat_kw("trait") {
            DeclKind::Trait(self.trait_decl(None)?)
        } else if self.eat_kw("impl") {
            DeclKind::Impl(self.impl_decl()?)
        } else if self.eat_kw("test") {
            let name = match self.next() {
                Tok::Str(s) => s,
                _ => return Err(self.err("expected test name string")),
            };
            DeclKind::Test(TestDecl { name, body: self.block()? })
        } else {
            return Err(self.err("expected `type`, `effect`, `trait`, `impl`, `fn` or `test`"));
        };
        Ok(Decl { comments, doc, kind, module: Symbol::EMPTY, public })
    }

    fn type_params(&mut self) -> R<Vec<Symbol>> {
        Ok(self.bounded_params()?.0)
    }

    /// `[T: Ord + Hash, U]`
    fn bounded_params(&mut self) -> R<(Vec<Symbol>, Vec<Bound>)> {
        if !self.eat_sym("[") {
            return Ok((vec![], vec![]));
        }
        let mut bounds = Vec::new();
        let names = self.list("]", |p| {
            let n = p.ident()?;
            if p.eat_sym(":") {
                let mut traits = vec![p.type_expr_plain()?];
                while p.eat_sym("+") {
                    traits.push(p.type_expr_plain()?);
                }
                bounds.push(Bound { param: n.clone(), traits });
            }
            Ok(n)
        })?;
        Ok((names, bounds))
    }

    fn trait_decl(&mut self, derivable: Option<(Symbol, TypeExpr)>) -> R<TraitDecl> {
        let name = self.ident()?;
        let params = self.type_params()?;
        let mut supers = Vec::new();
        if self.eat_sym(":") {
            supers.push(self.type_expr_plain()?);
            while self.eat_sym("+") {
                supers.push(self.type_expr_plain()?);
            }
        }
        // what a derivable trait needs of the types it is derived for
        let mut requires = Vec::new();
        while self.eat_kw("requires") {
            requires.push(self.expr_ns()?);
        }
        let derivable = match derivable {
            Some((name, kind)) => Some(Derivable { name, kind, requires }),
            None if !requires.is_empty() => return Err(self.err("only a `derivable` trait has `requires`: `derivable(t: Record) trait ... requires ...`")),
            None => None,
        };
        self.expect_sym("{")?;
        let (mut methods, mut laws) = (Vec::new(), Vec::new());
        while !self.eat_sym("}") {
            let mut comments = self.leading();
            let mut doc = Vec::new();
            while let Tok::Doc(d) = self.peek().clone() {
                doc.push(d);
                self.next();
            }
            comments.extend(self.leading());
            if self.eat_kw("law") {
                let lname = self.ident()?;
                let params = self.params()?;
                self.expect_sym(":")?;
                let body = self.expr()?;
                comments.extend(self.trailing());
                laws.push(Law { comments, doc, name: lname, params, body });
                continue;
            }
            // `derived fn`: its body is specialized for each type the trait is derived for
            let derived = self.is_kw("derived") && matches!(self.peek_at(1), Tok::Ident(f) if f == "fn");
            if derived {
                self.next();
            }
            self.expect_kw("fn")?;
            // a method without `self` is called on a type: `Money.empty()`, `T.empty()`
            let (mut decl, default) = self.fn_rest(true)?;
            if derived && !default {
                return Err(self.err(&format!("`derived fn {}` needs a body: the code specialized for each type", decl.name)));
            }
            decl.derived = derived;
            comments.extend(self.trailing());
            methods.push(TraitMethod { comments, doc, decl, default });
        }
        Ok(TraitDecl { name, params, supers, methods, laws, derivable })
    }

    fn impl_decl(&mut self) -> R<ImplDecl> {
        self.impl_rest(false)
    }

    /// After `impl` (or `derive`, which needs a trait and may leave out its block).
    fn impl_rest(&mut self, derived: bool) -> R<ImplDecl> {
        let (type_params, bounds) = self.bounded_params()?;
        let head = self.path()?;
        let head_args = if self.eat_sym("[") { self.list("]", |p| p.type_expr())? } else { vec![] };
        // `impl Trait for Type { ... }`, or `impl Type { ... }` for the type's own methods
        let (trait_name, trait_args, for_type) = if self.eat_kw("for") {
            (head, head_args, self.type_expr_plain()?)
        } else if derived {
            return Err(self.err("`derive` needs a trait and a type: `derive Trait for Type`"));
        } else {
            (Symbol::EMPTY, vec![], TypeExpr::Named(head, head_args))
        };
        let mut methods = Vec::new();
        if derived && !self.is_sym("{") {
            return Ok(ImplDecl { type_params, bounds, trait_name, trait_args, for_type, methods, derived });
        }
        self.expect_sym("{")?;
        while !self.eat_sym("}") {
            let mut comments = self.leading();
            let mut doc = Vec::new();
            while let Tok::Doc(d) = self.peek().clone() {
                doc.push(d);
                self.next();
            }
            comments.extend(self.leading());
            self.expect_kw("fn")?;
            let (mut decl, has_body) = self.fn_rest(false)?;
            if !has_body {
                return Err(self.err(&format!("`{}` in an impl needs a body", decl.name)));
            }
            comments.extend(self.trailing());
            (decl.doc, decl.comments) = (doc, comments);
            methods.push(decl);
        }
        Ok(ImplDecl { type_params, bounds, trait_name, trait_args, for_type, methods, derived })
    }

    fn type_decl(&mut self) -> R<TypeDecl> {
        let name = self.ident()?;
        let params = self.type_params()?;
        self.expect_sym("=")?;
        let body = if self.is_sym("|") {
            let mut vs = Vec::new();
            while self.is_sym("|") {
                let mut comments = self.leading();
                self.next();
                let vname = self.ident()?;
                let payload = self.payload()?;
                comments.extend(self.trailing());
                vs.push(Variant { comments, name: vname, payload });
            }
            TypeBody::Sum(vs)
        } else if self.eat_sym("{") {
            TypeBody::Record(self.fields()?)
        } else {
            TypeBody::Alias(self.type_union()?)
        };
        let refinement = if self.eat_kw("where") { Some(self.expr()?) } else { None };
        Ok(TypeDecl { distinct: false, name, params, body, refinement })
    }

    fn fields(&mut self) -> R<Vec<Field>> {
        self.list("}", |p| {
            let mut comments = p.leading();
            let mut doc = Vec::new();
            while let Tok::Doc(d) = p.peek().clone() {
                doc.push(d);
                p.next();
            }
            let name = p.ident()?;
            p.expect_sym(":")?;
            let ty = p.type_expr()?;
            let default = if p.eat_sym("=") { Some(p.expr()?) } else { None };
            comments.extend(p.trailing());
            Ok(Field { comments, doc, name, ty, default })
        })
    }

    fn payload(&mut self) -> R<Payload> {
        if self.eat_sym("(") {
            Ok(Payload::Positional(self.list(")", |p| p.type_expr())?))
        } else if self.eat_sym("{") {
            Ok(Payload::Record(self.fields()?))
        } else {
            Ok(Payload::None)
        }
    }

    /// A type, optionally a union (`Square | Circle`) and refined inline
    /// (`Int where it > 0`; the refinement applies to the whole union).
    pub fn type_expr(&mut self) -> R<TypeExpr> {
        let t = self.type_union()?;
        if self.eat_kw("where") {
            return Ok(TypeExpr::Refined(Box::new(t), Box::new(self.expr_ns()?)));
        }
        Ok(t)
    }

    /// `A | B | C`, or a single type.
    fn type_union(&mut self) -> R<TypeExpr> {
        let first = self.type_expr_plain()?;
        if !self.is_sym("|") {
            return Ok(first);
        }
        let mut ms = vec![first];
        while self.eat_sym("|") {
            ms.push(self.type_expr_plain()?);
        }
        Ok(TypeExpr::Union(ms))
    }

    pub fn type_expr_plain(&mut self) -> R<TypeExpr> {
        if self.eat_sym("(") {
            let args = self.list(")", |p| p.type_expr())?;
            if self.eat_sym("->") {
                return Ok(TypeExpr::Fn(args, Box::new(self.type_expr()?)));
            }
            return match args.len() {
                0 => Ok(TypeExpr::Unit),
                1 => Ok(args.into_iter().next().unwrap()),
                _ => Ok(TypeExpr::Named("Tuple".into(), args)),
            };
        }
        let name = self.path()?;
        let args = if self.eat_sym("[") { self.list("]", |p| p.type_expr())? } else { vec![] };
        Ok(TypeExpr::Named(name, args))
    }

    fn params(&mut self) -> R<Vec<Param>> {
        self.expect_sym("(")?;
        self.list(")", |p| {
            let comptime = p.eat_kw("comptime");
            let mutable = p.eat_kw("mut");
            let name = if p.is_kw("self") { p.next(); Symbol::new("self") } else { p.ident()? };
            // `self` alone means `self: Self`
            if name == "self" && !p.is_sym(":") {
                return Ok(Param { name, ty: TypeExpr::Named("Self".into(), vec![]), mutable, comptime });
            }
            p.expect_sym(":")?;
            Ok(Param { name, ty: p.type_expr()?, mutable, comptime })
        })
    }

    fn fn_decl(&mut self) -> R<FnDecl> {
        let (f, has_body) = self.fn_rest(false)?;
        if !has_body {
            return Err(self.err(&format!("`{}` needs a body (use `= ?todo` for a hole)", f.name)));
        }
        Ok(f)
    }

    /// Everything after `fn`. In a trait (`optional_body`), a method may end
    /// without a body; it is then a required method.
    fn fn_rest(&mut self, optional_body: bool) -> R<(FnDecl, bool)> {
        let name = self.ident()?;
        let (type_params, bounds) = self.bounded_params()?;
        let params = self.params()?;
        let ret = if self.eat_sym("->") { self.type_expr()? } else { TypeExpr::Unit };
        if self.is_sym("/") {
            return Err(self.err("effect lists (`/ {...}`) were removed: pass the capability as a parameter instead"));
        }
        let (mut requires, mut ensures, mut terminates) = (Vec::new(), Vec::new(), false);
        loop {
            if self.eat_kw("terminates") {
                terminates = true;
            } else if self.eat_kw("requires") {
                requires.push(self.expr_ns()?);
            } else if self.eat_kw("ensures") {
                ensures.push(self.expr_ns()?);
            } else {
                break;
            }
        }
        let has_body = !optional_body || self.is_sym("=") || self.is_sym("{");
        let body = if !has_body {
            Expr::Unit
        } else if self.eat_sym("=") {
            self.expr()?
        } else {
            self.block()?
        };
        Ok((FnDecl { doc: vec![], comments: vec![], name, type_params, bounds, params, ret, requires, ensures, terminates, body, comptime: false, derived: false }, has_body))
    }

    // ---------- expressions ----------

    /// expression where a `{` cannot start a record literal / be swallowed
    fn expr_ns(&mut self) -> R<Expr> {
        let old = self.no_struct;
        self.no_struct = true;
        let e = self.expr();
        self.no_struct = old;
        e
    }

    pub fn expr(&mut self) -> R<Expr> {
        let mut lhs = self.binary(0)?;
        // `a..b`, `a..=b`
        if !self.nl_before() && (self.is_sym("..") || self.is_sym("..=")) {
            let inclusive = self.is_sym("..=");
            self.next();
            let hi = self.binary(0)?;
            lhs = Expr::Range(Some(Box::new(lhs)), Some(Box::new(hi)), inclusive);
        }
        if self.eat_sym("==>") {
            let rhs = self.expr()?;
            return Ok(Expr::Bin(BinOp::Implies, Box::new(lhs), Box::new(rhs)));
        }
        Ok(lhs)
    }

    fn binop_at(&self, level: usize) -> Option<BinOp> {
        use BinOp::*;
        let Tok::Sym(s) = self.peek() else { return None };
        let table: &[(&str, BinOp)] = match level {
            0 => &[("||", Or)],
            1 => &[("&&", And)],
            2 => &[("==", Eq), ("!=", Ne), ("<=", Le), (">=", Ge), ("<", Lt), (">", Gt)],
            4 => &[("+", Add), ("-", Sub)],
            5 => &[("*", Mul), ("/", Div), ("%", Mod)],
            _ => &[],
        };
        table.iter().find(|(t, _)| t == s).map(|(_, op)| *op)
    }

    fn binary(&mut self, level: usize) -> R<Expr> {
        if level > 5 {
            return self.unary();
        }
        let mut lhs = self.binary(level + 1)?;
        // `a ?? b`: an Option's `Some` value, or else `b` (`a or { b }`); `a !! b`: a Result's
        // `Ok` value, or else `b` (`a except { b }`). `a ?? b ?? c` is `a ?? (b ?? c)`.
        if level == 3 {
            if self.eat_sym("??") {
                let rhs = self.binary(3)?;
                return Ok(Expr::Or(Box::new(lhs), Box::new(rhs)));
            }
            if self.eat_sym("!!") {
                let rhs = self.binary(3)?;
                return Ok(Expr::Except(Box::new(lhs), None, Box::new(rhs)));
            }
            return Ok(lhs);
        }
        loop {
            if level == 2 && self.eat_kw("is") {
                let ctor = self.path()?;
                lhs = Expr::Is(Box::new(lhs), ctor);
                continue;
            }
            // `|` cannot be a binary op; `-` on a new line starts a new statement
            if self.nl_before() && matches!(self.peek(), Tok::Sym("-")) {
                break;
            }
            if self.is_sym("++") {
                return Err(self.err("`++` is gone: join strings and lists with `+`"));
            }
            match self.binop_at(level) {
                Some(op) => {
                    self.next();
                    let rhs = self.binary(level + 1)?;
                    lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
                }
                None => break,
            }
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> R<Expr> {
        if self.eat_sym("-") {
            return Ok(match self.unary()? {
                Expr::Int(n) => Expr::Int(-n),
                Expr::Float(f) => Expr::Float(-f),
                e => Expr::Un(UnOp::Neg, Box::new(e)),
            });
        }
        if self.eat_sym("!") {
            return Ok(Expr::Un(UnOp::Not, Box::new(self.unary()?)));
        }
        // `!!x` lexes as one symbol
        if self.eat_sym("!!") {
            return Ok(Expr::Un(UnOp::Not, Box::new(Expr::Un(UnOp::Not, Box::new(self.unary()?)))));
        }
        if self.eat_kw("mut") {
            return self.marked();
        }
        // `comptime e`: evaluated when the program is compiled
        if self.eat_kw("comptime") {
            return Ok(Expr::Comptime(Box::new(self.unary()?)));
        }
        self.postfix()
    }

    /// After `mut`: a place (`x`, `o.lines`), passed to a `mut` parameter
    /// (`f(mut x)`), or a call on a place that changes it: `mut q.claim(2)`
    /// marks the first call, and any calls after it are on its result
    /// (`mut q.claim(2).len()` is `(mut q.claim(2)).len()`).
    fn marked(&mut self) -> R<Expr> {
        let mut place = Expr::Var(self.ident()?);
        loop {
            if (self.is_sym("(") || self.is_sym("[")) && !self.nl_before() {
                return Err(self.err("only a variable or a field can be `mut`: name the value first (`mut x := …`)"));
            }
            if !self.is_sym(".") {
                return Ok(Expr::Mut(Box::new(place)));
            }
            if let Tok::Int(n) = self.peek_at(1).clone() {
                self.next();
                self.next();
                place = Expr::Field(Box::new(place), Symbol::from(n.to_string()));
                continue;
            }
            let Tok::Ident(name) = self.peek_at(1).clone() else { return Err(self.err("expected a field or method name")) };
            if matches!(self.peek_at(2), Tok::Sym("(")) && !self.toks[self.pos + 2].nl_before {
                self.next();
                self.next();
                self.next();
                let call = Expr::Method(Box::new(place), name, self.args()?);
                return self.postfix_from(Expr::Mut(Box::new(call)));
            }
            self.next();
            self.next();
            place = Expr::Field(Box::new(place), name);
        }
    }

    fn args(&mut self) -> R<Vec<Expr>> {
        let old = self.no_struct;
        self.no_struct = false;
        // `f(mut x)`: the call may change `x` (its parameter is `mut`); see `marked`
        let r = self.list(")", |p| p.expr());
        self.no_struct = old;
        r
    }

    fn postfix(&mut self) -> R<Expr> {
        let e = self.primary()?;
        self.postfix_from(e)
    }

    /// Calls, fields, indexes, `!`, `?` and `except` after `e`.
    fn postfix_from(&mut self, mut e: Expr) -> R<Expr> {
        loop {
            if self.is_sym("(") && !self.nl_before() {
                self.next();
                e = Expr::Call(Box::new(e), self.args()?);
            } else if self.is_sym(".") {
                self.next();
                if let Tok::Int(n) = self.peek().clone() {
                    self.next();
                    e = Expr::Field(Box::new(e), n.to_string().into());
                    continue;
                }
                // `f.type`: a field may be named like a keyword after a dot
                let name = if self.is_kw("type") {
                    self.next();
                    Symbol::new("type")
                } else {
                    self.ident()?
                };
                if self.is_sym("(") && !self.nl_before() {
                    self.next();
                    e = Expr::Method(Box::new(e), name, self.args()?);
                } else {
                    e = Expr::Field(Box::new(e), name);
                }
            } else if self.is_sym("[") && !self.nl_before() {
                self.next();
                e = Expr::Index(Box::new(e), Box::new(self.index()?));
            } else if self.is_sym("!") && !self.nl_before() {
                self.next();
                e = Expr::Try(Box::new(e), TryKind::Result);
            } else if self.is_sym("?") && !self.nl_before() {
                self.next();
                e = Expr::Try(Box::new(e), TryKind::Option);
            } else if self.eat_kw("except") {
                let name = match self.peek().clone() {
                    Tok::Ident(n) if !KEYWORDS.contains(&n.as_str()) => {
                        self.next();
                        Some(n)
                    }
                    _ => None,
                };
                e = Expr::Except(Box::new(e), name, Box::new(self.block()?));
            } else if self.is_kw("or") && matches!(self.peek_at(1), Tok::Sym("{")) {
                self.next();
                e = Expr::Or(Box::new(e), Box::new(self.block()?));
            } else {
                break;
            }
        }
        Ok(e)
    }

    /// What is inside `xs[...]`, past the `[`: an index `i`, or a range
    /// `a..b`, `a..=b`, `a..`, `..b`, `..`.
    fn index(&mut self) -> R<Expr> {
        let old = self.no_struct;
        self.no_struct = false;
        let lo = if self.is_sym("..") || self.is_sym("..=") { None } else { Some(Box::new(self.binary(0)?)) };
        let inclusive = if self.eat_sym("..=") {
            true
        } else if self.eat_sym("..") {
            false
        } else if let Some(i) = lo {
            self.expect_sym("]")?;
            self.no_struct = old;
            return Ok(*i);
        } else {
            unreachable!()
        };
        let hi = if self.is_sym("]") { None } else { Some(Box::new(self.binary(0)?)) };
        if inclusive && hi.is_none() {
            return Err(self.err("`..=` needs an end: `xs[a..=b]`"));
        }
        self.expect_sym("]")?;
        self.no_struct = old;
        Ok(Expr::Range(lo, hi, inclusive))
    }

    fn primary(&mut self) -> R<Expr> {
        match self.peek().clone() {
            Tok::Int(n) => {
                self.next();
                Ok(Expr::Int(n))
            }
            Tok::BigInt(s) => {
                self.next();
                Ok(Expr::BigInt(s))
            }
            Tok::Float(f) => {
                self.next();
                Ok(Expr::Float(f))
            }
            Tok::Str(s) => {
                self.next();
                Ok(Expr::Str(s))
            }
            Tok::Hole(h) => {
                self.next();
                Ok(Expr::Hole(h))
            }
            Tok::FStr(parts) => {
                self.next();
                let mut out = Vec::new();
                for part in parts {
                    out.push(match part {
                        FPart::Lit(s) => StrPart::Lit(s),
                        FPart::Code(code, line, col) => {
                            let at = |e: String| format!("{line}:{col}: in f-string: {e}");
                            let (code, spec) = match spec_colon(&code) {
                                Some(k) => (code[..k].to_string(), Some(FmtSpec::parse(&code[k + 1..]).map_err(at)?)),
                                None => (code, None),
                            };
                            let mut p = Parser::new(&code).map_err(at)?;
                            let e = p.expr().map_err(at)?;
                            if p.peek() != &Tok::Eof {
                                return Err(at(p.err("expected `}` after the expression")));
                            }
                            if !p.comments.is_empty() {
                                return Err(at("comments are not allowed inside `{}`".into()));
                            }
                            StrPart::Expr(e, spec)
                        }
                    });
                }
                Ok(Expr::Interp(out))
            }
            Tok::Sym("(") if self.lambda_ahead() => {
                self.next();
                let ps = self.list(")", |p| {
                    let name = p.ident()?;
                    let ty = if p.eat_sym(":") { Some(p.type_expr()?) } else { None };
                    Ok(LamParam { name, ty })
                })?;
                self.expect_sym("->")?;
                Ok(Expr::Lambda(ps, Box::new(self.expr()?)))
            }
            Tok::Ident(name) if !KEYWORDS.contains(&name.as_str()) && !is_upper(&name) && matches!(self.peek_at(1), Tok::Sym("->")) => {
                self.next();
                self.next();
                Ok(Expr::Lambda(vec![LamParam { name, ty: None }], Box::new(self.expr()?)))
            }
            Tok::Sym("(") => {
                self.next();
                if self.eat_sym(")") {
                    return Ok(Expr::Unit);
                }
                let old = self.no_struct;
                self.no_struct = false;
                let e = self.expr()?;
                if self.eat_sym(",") {
                    let mut items = vec![e];
                    items.extend(self.list(")", |p| p.expr())?);
                    self.no_struct = old;
                    return Ok(Expr::Tuple(items));
                }
                self.no_struct = old;
                self.expect_sym(")")?;
                Ok(e)
            }
            Tok::Sym("[") => {
                self.next();
                if self.eat_sym("]") {
                    return Ok(Expr::List(vec![]));
                }
                let old = self.no_struct;
                self.no_struct = false;
                let first = self.expr()?;
                // `[e for x in xs where c for y in ys]`
                if self.is_kw("for") {
                    let gens = self.comp_fors("]")?;
                    self.no_struct = old;
                    return Ok(Expr::Comp(CompKind::List, Box::new(first), gens));
                }
                let mut items = vec![first];
                if self.eat_sym(",") {
                    items.extend(self.list("]", |p| p.expr())?);
                } else {
                    self.expect_sym("]")?;
                }
                self.no_struct = old;
                Ok(Expr::List(items))
            }
            Tok::Sym("{") => self.block(),
            Tok::Sym("|" | "||") => Err(self.err("lambdas are written `x -> body`, `(a, b) -> body` or `() -> body`")),
            Tok::Ident(kw) if kw == "true" || kw == "false" => {
                self.next();
                Ok(Expr::Bool(kw == "true"))
            }
            Tok::Ident(kw) if kw == "if" => {
                self.next();
                let c = self.expr_ns()?;
                let t = self.block()?;
                let f = if self.eat_kw("else") {
                    if self.is_kw("if") { self.primary()? } else { self.block()? }
                } else {
                    Expr::Unit
                };
                Ok(Expr::If(Box::new(c), Box::new(t), Box::new(f)))
            }
            Tok::Ident(kw) if kw == "match" => {
                self.next();
                let scrut = self.expr_ns()?;
                let name = if matches!(self.peek(), Tok::Ident(s) if s == "as") {
                    self.next();
                    Some(self.ident()?)
                } else {
                    None
                };
                self.expect_sym("{")?;
                let mut arms = Vec::new();
                while !self.eat_sym("}") {
                    let mut comments = self.leading();
                    let pat = self.pattern()?;
                    self.expect_sym("=>")?;
                    let body = self.expr()?;
                    self.eat_sym(",");
                    comments.extend(self.trailing());
                    arms.push(Arm { comments, pat, body });
                }
                Ok(Expr::Match(Box::new(scrut), name, arms))
            }
            Tok::Ident(kw) if kw == "return" => {
                self.next();
                if self.nl_before() || matches!(self.peek(), Tok::Sym("}" | ")" | "," | ";") | Tok::Eof) {
                    return Ok(Expr::Return(Box::new(Expr::Unit)));
                }
                Ok(Expr::Return(Box::new(self.expr()?)))
            }
            Tok::Ident(kw) if kw == "mock" => {
                self.next();
                Ok(Expr::Mock(self.type_expr_plain()?))
            }
            Tok::Ident(name) if !KEYWORDS.contains(&name.as_str()) => {
                let name = match self.qualified_upper() {
                    Some(q) => q,
                    None => {
                        self.next();
                        name
                    }
                };
                if !is_upper(crate::names::short(&name)) {
                    return Ok(Expr::Var(name));
                }
                if name == "Dict" || name == "Set" {
                    return self.collection(name == "Dict");
                }
                if self.is_sym("(") && !self.nl_before() {
                    self.next();
                    return Ok(Expr::Ctor(name, CtorArgs::Positional(self.args()?)));
                }
                if self.is_sym("{") && !self.no_struct {
                    self.next();
                    if self.eat_sym("..") {
                        let base = self.expr()?;
                        let mut fields = Vec::new();
                        while self.eat_sym(",") && !self.is_sym("}") {
                            let f = self.ident()?;
                            let v = if self.eat_sym(":") { self.expr()? } else { Expr::Var(f.clone()) };
                            fields.push((f, v));
                        }
                        self.expect_sym("}")?;
                        return Ok(Expr::Ctor(name, CtorArgs::Update(Box::new(base), fields)));
                    }
                    let fields = self.list("}", |p| {
                        let f = p.ident()?;
                        if p.eat_sym(":") { Ok((f, p.expr()?)) } else { Ok((f.clone(), Expr::Var(f))) }
                    })?;
                    return Ok(Expr::Ctor(name, CtorArgs::Record(fields)));
                }
                Ok(Expr::Ctor(name, CtorArgs::None))
            }
            _ => Err(self.err("expected expression")),
        }
    }

    /// The `for p in xs where c` parts of a comprehension, and its closing `close`.
    fn comp_fors(&mut self, close: &str) -> R<Vec<CompFor>> {
        let mut gens = Vec::new();
        while self.eat_kw("for") {
            let pat = self.pattern()?;
            self.expect_kw("in")?;
            let iter = self.expr()?;
            let cond = if self.eat_kw("where") { Some(self.expr()?) } else { None };
            gens.push(CompFor { pat, iter, cond });
        }
        self.expect_sym(close)?;
        Ok(gens)
    }

    /// `Dict { k: v, … }`, `Set { x, … }`, with optional type arguments
    /// (`Dict[String, Int] {}`), or a comprehension (`Set { x % 3 for x in xs }`).
    /// At the `[` or `{` after the name.
    fn collection(&mut self, dict: bool) -> R<Expr> {
        let tys = if self.is_sym("[") && !self.nl_before() {
            self.next();
            self.list("]", |p| p.type_expr())?
        } else {
            vec![]
        };
        let what = if dict { "Dict" } else { "Set" };
        if !self.is_sym("{") {
            return Err(self.err(&format!("expected `{{`: `{what} {{ … }}`")));
        }
        if self.no_struct {
            return Err(self.err(&format!("put the `{what} {{ … }}` in parentheses here")));
        }
        self.next();
        let old = self.no_struct;
        self.no_struct = false;
        let r = self.collection_body(dict, tys);
        self.no_struct = old;
        r
    }

    fn collection_body(&mut self, dict: bool, tys: Vec<TypeExpr>) -> R<Expr> {
        let entry = |p: &mut Self| -> R<(Expr, Option<Expr>)> {
            let k = p.expr()?;
            if dict {
                p.expect_sym(":")?;
                Ok((k, Some(p.expr()?)))
            } else {
                Ok((k, None))
            }
        };
        if self.eat_sym("}") {
            return Ok(if dict { Expr::Dict(tys, vec![]) } else { Expr::Set(tys, vec![]) });
        }
        let first = entry(self)?;
        if self.is_kw("for") {
            if !tys.is_empty() {
                return Err(self.err("a comprehension takes no type arguments"));
            }
            let gens = self.comp_fors("}")?;
            return Ok(match first {
                (k, Some(v)) => Expr::Comp(CompKind::Dict, Box::new(Expr::Tuple(vec![k, v])), gens),
                (x, None) => Expr::Comp(CompKind::Set, Box::new(x), gens),
            });
        }
        let mut entries = vec![first];
        if self.eat_sym(",") {
            entries.extend(self.list("}", entry)?);
        } else {
            self.expect_sym("}")?;
        }
        Ok(if dict {
            Expr::Dict(tys, entries.into_iter().map(|(k, v)| (k, v.unwrap())).collect())
        } else {
            Expr::Set(tys, entries.into_iter().map(|(x, _)| x).collect())
        })
    }

    /// At a `(`: is this the parameter list of a lambda, `(a, b) -> ...`?
    fn lambda_ahead(&self) -> bool {
        let mut depth = 0;
        let mut i = self.pos;
        while i < self.toks.len() {
            match &self.toks[i].tok {
                Tok::Sym("(" | "[" | "{") => depth += 1,
                Tok::Sym(")" | "]" | "}") => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(self.toks.get(i + 1).map(|t| &t.tok), Some(Tok::Sym("->")));
                    }
                }
                Tok::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    fn block(&mut self) -> R<Expr> {
        self.expect_sym("{")?;
        let old = self.no_struct;
        self.no_struct = false;
        let mut stmts = Vec::new();
        while !self.is_sym("}") {
            // a blank line between statements is kept, as "" before what follows
            let blank = !stmts.is_empty() && self.blank_line_before();
            let mut above = self.leading();
            if blank {
                above.insert(0, String::new());
            }
            if !above.is_empty() {
                stmts.push(Stmt::Comment(above));
            }
            if self.eat_kw("let") {
                // `let` only introduces a typed binding: `let p: T = e`
                let pat = self.pattern()?;
                if self.is_sym("=") || self.is_sym(":=") {
                    return Err(self.err("`let` needs a type (`let x: T = e`); without one, write `x := e`"));
                }
                self.expect_sym(":")?;
                let ty = self.type_expr()?;
                self.expect_sym("=")?;
                stmts.push(Stmt::Let(pat, Some(ty), self.expr()?));
            } else if self.is_kw("mut")
                && matches!(self.peek_at(1), Tok::Ident(_))
                && matches!(self.peek_at(2), Tok::Sym(":=" | ":" | "="))
            {
                // `mut x := e`, `mut x: T = e`
                self.next();
                let name = self.ident()?;
                if self.eat_sym(":=") {
                    stmts.push(Stmt::Var(name, None, self.expr()?));
                } else if self.eat_sym(":") {
                    let ty = self.type_expr()?;
                    self.expect_sym("=")?;
                    stmts.push(Stmt::Var(name, Some(ty), self.expr()?));
                } else {
                    return Err(self.err("a new variable is `mut x := e` or `mut x: T = e`"));
                }
            } else if let Some(pat) = self.binding_pattern() {
                // `p := e`
                stmts.push(Stmt::Let(pat, None, self.expr()?));
            } else if self.eat_kw("assert") {
                stmts.push(Stmt::Assert(self.expr()?));
            } else if self.eat_kw("loop") {
                stmts.push(Stmt::Loop(self.block()?));
            } else if self.eat_kw("break") {
                stmts.push(Stmt::Break);
            } else if self.eat_kw("continue") {
                stmts.push(Stmt::Continue);
            } else if self.eat_kw("while") {
                let cond = self.expr_ns()?;
                stmts.push(Stmt::While(cond, self.block()?));
            } else if self.eat_kw("for") {
                let pat = self.pattern()?;
                self.expect_kw("in")?;
                let iter = self.expr_ns()?;
                stmts.push(Stmt::For(pat, iter, self.block()?));
            } else {
                let e = self.expr()?;
                let op = match self.peek() {
                    Tok::Sym("=") => Some(None),
                    Tok::Sym("+=") => Some(Some(BinOp::Add)),
                    Tok::Sym("-=") => Some(Some(BinOp::Sub)),
                    Tok::Sym("*=") => Some(Some(BinOp::Mul)),
                    Tok::Sym("/=") => Some(Some(BinOp::Div)),
                    Tok::Sym("%=") => Some(Some(BinOp::Mod)),
                    Tok::Sym(":=") => return Err(self.err("`:=` declares a new name: `x := e`, `(a, b) := e`")),
                    _ => None,
                };
                if let Some(op) = op {
                    if !matches!(e, Expr::Var(_) | Expr::Field(..)) {
                        return Err(self.err("only a variable or a field (`x`, `x.f`) can be assigned"));
                    }
                    self.next();
                    let rhs = self.expr()?;
                    // `x += e` is `x = x + e`
                    let rhs = match op {
                        Some(op) => Expr::Bin(op, Box::new(e.clone()), Box::new(rhs)),
                        None => rhs,
                    };
                    stmts.push(Stmt::Assign(e, rhs));
                } else {
                    stmts.push(Stmt::Expr(e));
                }
            }
            while self.eat_sym(";") {}
            // a comment ending the statement's line goes above the statement
            let t = self.trailing();
            if !t.is_empty() {
                let s = stmts.pop().unwrap();
                match stmts.last_mut() {
                    Some(Stmt::Comment(cs)) => cs.extend(t),
                    _ => stmts.push(Stmt::Comment(t)),
                }
                stmts.push(s);
            }
        }
        self.expect_sym("}")?;
        self.no_struct = old;
        let last = match stmts.last() {
            Some(Stmt::Expr(_)) => match stmts.pop() {
                Some(Stmt::Expr(e)) => e,
                _ => unreachable!(),
            },
            _ => Expr::Unit,
        };
        if stmts.is_empty() {
            return Ok(last);
        }
        Ok(Expr::Block(stmts, Box::new(last)))
    }

    // ---------- patterns ----------

    /// The pattern of `p := e`, with the `:=` taken; None (and nothing
    /// taken) if the statement is not a binding.
    fn binding_pattern(&mut self) -> Option<Pattern> {
        let (pos, used) = (self.pos, self.used.clone());
        if let Ok(p) = self.pattern() {
            if self.eat_sym(":=") {
                return Some(p);
            }
        }
        self.pos = pos;
        self.used = used;
        None
    }

    /// A pattern, or alternatives: `Square | Circle`.
    fn pattern(&mut self) -> R<Pattern> {
        let first = self.pattern_one()?;
        if !self.is_sym("|") {
            return Ok(first);
        }
        let mut alts = vec![first];
        while self.eat_sym("|") {
            alts.push(self.pattern_one()?);
        }
        Ok(Pattern::Or(alts))
    }

    fn pattern_one(&mut self) -> R<Pattern> {
        match self.peek().clone() {
            Tok::BigInt(_) => Err(self.err("an integer pattern must fit in 64 bits (use `where it == …` for a larger one)")),
            Tok::Int(n) => {
                self.next();
                Ok(Pattern::Int(n))
            }
            Tok::Sym("-") => {
                self.next();
                match self.next() {
                    Tok::Int(n) => Ok(Pattern::Int(-n)),
                    _ => Err(self.err("expected integer")),
                }
            }
            Tok::Str(s) => {
                self.next();
                Ok(Pattern::Str(s))
            }
            Tok::Sym("(") => {
                self.next();
                let items = self.list(")", |p| p.pattern())?;
                Ok(if items.len() == 1 { items.into_iter().next().unwrap() } else { Pattern::Tuple(items) })
            }
            Tok::Sym("[") => {
                self.next();
                let mut items = Vec::new();
                let mut rest = None;
                while !self.is_sym("]") {
                    if self.eat_sym("..") {
                        rest = Some(if self.is_sym("]") { Symbol::new("_") } else { self.ident()? });
                        break;
                    }
                    items.push(self.pattern()?);
                    if !self.eat_sym(",") {
                        break;
                    }
                }
                self.expect_sym("]")?;
                Ok(Pattern::List(items, rest))
            }
            Tok::Ident(s) if s == "_" => {
                self.next();
                Ok(Pattern::Wild)
            }
            Tok::Ident(s) if s == "where" => {
                self.next();
                Ok(Pattern::Cond(Box::new(self.expr()?)))
            }
            Tok::Ident(s) if s == "true" || s == "false" => {
                self.next();
                Ok(Pattern::Bool(s == "true"))
            }
            Tok::Ident(s) if is_upper(&s) || self.qualified_upper_ahead() => {
                let s = match self.qualified_upper() {
                    Some(q) => q,
                    None => {
                        self.next();
                        s
                    }
                };
                if self.eat_sym("(") {
                    return Ok(Pattern::Ctor(s, CtorPat::Positional(self.list(")", |p| p.pattern())?)));
                }
                if self.eat_sym("{") {
                    let mut fields = Vec::new();
                    let mut open = false;
                    while !self.is_sym("}") {
                        if self.eat_sym("..") {
                            open = true;
                            break;
                        }
                        let f = self.ident()?;
                        let p = if self.eat_sym(":") { self.pattern()? } else { Pattern::Bind(f.clone()) };
                        fields.push((f, p));
                        if !self.eat_sym(",") {
                            break;
                        }
                    }
                    self.expect_sym("}")?;
                    return Ok(Pattern::Ctor(s, CtorPat::Record(fields, open)));
                }
                Ok(Pattern::Ctor(s, CtorPat::None))
            }
            Tok::Ident(_) => Ok(Pattern::Bind(self.ident()?)),
            _ => Err(self.err("expected pattern")),
        }
    }
}

/// Where the format spec starts in an f-string part: the first `:` outside
/// brackets and string literals (`{price:.2}`; not the `:` of `{ {a: 1} }`).
fn spec_colon(code: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, c) in code.char_indices() {
        if in_str {
            match c {
                _ if esc => esc = false,
                '\\' => esc = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ':' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}
