use crate::ast::*;
use crate::lexer::{lex, Tok, Token};

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
    no_struct: bool,
}

type R<T> = Result<T, String>;

pub fn parse_program(src: &str) -> R<Program> {
    let mut p = Parser { toks: lex(src)?, pos: 0, no_struct: false };
    let mut decls = Vec::new();
    while p.peek() != &Tok::Eof {
        p.decls_into("", &mut decls)?;
    }
    Ok(Program { decls })
}

/// Parse a file as the given module (e.g. `orders.strata` -> `orders`).
/// `module` blocks inside it nest under that path.
pub fn parse_module_file(src: &str, module: &str) -> R<Program> {
    let mut p = Parser { toks: lex(src)?, pos: 0, no_struct: false };
    let mut decls = Vec::new();
    while p.peek() != &Tok::Eof {
        p.decls_into(module, &mut decls)?;
    }
    Ok(Program { decls })
}

#[allow(dead_code)] // used by upcoming REPL / agent API
pub fn parse_expr(src: &str) -> R<Expr> {
    let mut p = Parser { toks: lex(src)?, pos: 0, no_struct: false };
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
    "type", "fn", "test", "let", "if", "else", "match", "mock", "where", "mut", "while", "for", "in",
    "requires", "ensures", "true", "false", "is", "assert", "trait", "impl", "law", "for", "pub", "use",
];

impl Parser {
    pub fn from_str(src: &str) -> Parser {
        Parser { toks: lex(src).expect("lexes"), pos: 0, no_struct: false }
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
    fn ident(&mut self) -> R<String> {
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
    fn qualified_upper(&mut self) -> Option<String> {
        let mut k = 0;
        let mut parts = Vec::new();
        loop {
            let Tok::Ident(seg) = self.peek_at(k).clone() else { return None };
            if KEYWORDS.contains(&seg.as_str()) {
                return None;
            }
            parts.push(seg.clone());
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
        Some(parts.join("."))
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
    fn path(&mut self) -> R<String> {
        let mut s = self.ident()?;
        while self.is_sym(".") && matches!(self.peek_at(1), Tok::Ident(_)) {
            self.next();
            s.push('.');
            s.push_str(&self.ident()?);
        }
        Ok(s)
    }

    /// One declaration, or a whole `module name { ... }` block.
    fn decls_into(&mut self, module: &str, out: &mut Vec<Decl>) -> R<()> {
        if self.is_kw("module") && !matches!(self.peek_at(1), Tok::Sym(_)) {
            self.next();
            let name = self.path()?;
            let full = if module.is_empty() { name } else { format!("{module}.{name}") };
            self.expect_sym("{")?;
            while !self.eat_sym("}") {
                self.decls_into(&full, out)?;
            }
            return Ok(());
        }
        let mut d = self.decl()?;
        d.module = module.to_string();
        out.push(d);
        Ok(())
    }

    pub fn decl(&mut self) -> R<Decl> {
        let mut doc = Vec::new();
        while let Tok::Doc(d) = self.peek().clone() {
            doc.push(d);
            self.next();
        }
        let public = self.eat_kw("pub");
        if self.eat_kw("use") {
            let mut path = self.ident()?;
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
            return Ok(Decl { doc, kind: DeclKind::Use(UseDecl { path, items }), module: String::new(), public });
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
        } else if self.is_kw("effect") {
            return Err(self.err("`effect` was removed: declare a `trait` and pass a value that implements it"));
        } else if self.eat_kw("fn") {
            DeclKind::Fn(self.fn_decl()?)
        } else if self.eat_kw("trait") {
            DeclKind::Trait(self.trait_decl()?)
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
        Ok(Decl { doc, kind, module: String::new(), public })
    }

    fn type_params(&mut self) -> R<Vec<String>> {
        Ok(self.bounded_params()?.0)
    }

    /// `[T: Ord + Hash, U]`
    fn bounded_params(&mut self) -> R<(Vec<String>, Vec<Bound>)> {
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

    fn trait_decl(&mut self) -> R<TraitDecl> {
        let name = self.ident()?;
        let params = self.type_params()?;
        let mut supers = Vec::new();
        if self.eat_sym(":") {
            supers.push(self.type_expr_plain()?);
            while self.eat_sym("+") {
                supers.push(self.type_expr_plain()?);
            }
        }
        self.expect_sym("{")?;
        let (mut methods, mut laws) = (Vec::new(), Vec::new());
        while !self.eat_sym("}") {
            let mut doc = Vec::new();
            while let Tok::Doc(d) = self.peek().clone() {
                doc.push(d);
                self.next();
            }
            if self.eat_kw("law") {
                let lname = self.ident()?;
                let params = self.params()?;
                self.expect_sym(":")?;
                laws.push(Law { name: lname, params, body: self.expr()? });
                continue;
            }
            self.expect_kw("fn")?;
            let (decl, default) = self.fn_rest(true)?;
            if decl.params.first().map(|p| p.name.as_str()) != Some("self") {
                return Err(self.err(&format!("trait method `{}` must take `self` as its first parameter", decl.name)));
            }
            methods.push(TraitMethod { doc, decl, default });
        }
        Ok(TraitDecl { name, params, supers, methods, laws })
    }

    fn impl_decl(&mut self) -> R<ImplDecl> {
        let (type_params, bounds) = self.bounded_params()?;
        let trait_name = self.path()?;
        let trait_args = if self.eat_sym("[") { self.list("]", |p| p.type_expr())? } else { vec![] };
        self.expect_kw("for")?;
        let for_type = self.type_expr_plain()?;
        self.expect_sym("{")?;
        let mut methods = Vec::new();
        while !self.eat_sym("}") {
            while let Tok::Doc(_) = self.peek() {
                self.next();
            }
            self.expect_kw("fn")?;
            let (decl, has_body) = self.fn_rest(false)?;
            if !has_body {
                return Err(self.err(&format!("`{}` in an impl needs a body", decl.name)));
            }
            methods.push(decl);
        }
        Ok(ImplDecl { type_params, bounds, trait_name, trait_args, for_type, methods })
    }

    fn type_decl(&mut self) -> R<TypeDecl> {
        let name = self.ident()?;
        let params = self.type_params()?;
        self.expect_sym("=")?;
        let body = if self.is_sym("|") {
            let mut vs = Vec::new();
            while self.eat_sym("|") {
                let vname = self.ident()?;
                let payload = self.payload()?;
                vs.push(Variant { name: vname, payload });
            }
            TypeBody::Sum(vs)
        } else if self.eat_sym("{") {
            TypeBody::Record(self.fields()?)
        } else {
            TypeBody::Alias(self.type_expr_plain()?)
        };
        let refinement = if self.eat_kw("where") { Some(self.expr()?) } else { None };
        Ok(TypeDecl { distinct: false, name, params, body, refinement })
    }

    fn fields(&mut self) -> R<Vec<Field>> {
        self.list("}", |p| {
            let name = p.ident()?;
            p.expect_sym(":")?;
            Ok(Field { name, ty: p.type_expr()? })
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

    /// A type, optionally refined inline: `Int where it > 0`.
    pub fn type_expr(&mut self) -> R<TypeExpr> {
        let t = self.type_expr_plain()?;
        if self.eat_kw("where") {
            return Ok(TypeExpr::Refined(Box::new(t), Box::new(self.expr_ns()?)));
        }
        Ok(t)
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
            let mutable = p.eat_kw("mut");
            let name = if p.is_kw("self") { p.next(); "self".to_string() } else { p.ident()? };
            // `self` alone means `self: Self`
            if name == "self" && !p.is_sym(":") {
                return Ok(Param { name, ty: TypeExpr::Named("Self".into(), vec![]), mutable });
            }
            p.expect_sym(":")?;
            Ok(Param { name, ty: p.type_expr()?, mutable })
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
        let (mut requires, mut ensures) = (Vec::new(), Vec::new());
        loop {
            if self.eat_kw("requires") {
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
        Ok((FnDecl { name, type_params, bounds, params, ret, requires, ensures, body }, has_body))
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
        let lhs = self.binary(0)?;
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
            3 => &[("++", Concat)],
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
        self.postfix()
    }

    fn args(&mut self) -> R<Vec<Expr>> {
        let old = self.no_struct;
        self.no_struct = false;
        let r = self.list(")", |p| p.expr());
        self.no_struct = old;
        r
    }

    fn postfix(&mut self) -> R<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.is_sym("(") && !self.nl_before() {
                self.next();
                e = Expr::Call(Box::new(e), self.args()?);
            } else if self.is_sym(".") {
                self.next();
                if let Tok::Int(n) = self.peek().clone() {
                    self.next();
                    e = Expr::Field(Box::new(e), n.to_string());
                    continue;
                }
                let name = self.ident()?;
                if self.is_sym("(") && !self.nl_before() {
                    self.next();
                    e = Expr::Method(Box::new(e), name, self.args()?);
                } else {
                    e = Expr::Field(Box::new(e), name);
                }
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn primary(&mut self) -> R<Expr> {
        match self.peek().clone() {
            Tok::Int(n) => {
                self.next();
                Ok(Expr::Int(n))
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
                Ok(Expr::List(self.list("]", |p| p.expr())?))
            }
            Tok::Sym("{") => self.block(),
            Tok::Sym("||") => {
                self.next();
                Ok(Expr::Lambda(vec![], Box::new(self.expr()?)))
            }
            Tok::Sym("|") => {
                self.next();
                let mut ps = Vec::new();
                while !self.eat_sym("|") {
                    ps.push(self.ident()?);
                    self.eat_sym(",");
                }
                Ok(Expr::Lambda(ps, Box::new(self.expr()?)))
            }
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
                self.expect_sym("{")?;
                let mut arms = Vec::new();
                while !self.eat_sym("}") {
                    let pat = self.pattern()?;
                    self.expect_sym("=>")?;
                    let body = self.expr()?;
                    self.eat_sym(",");
                    arms.push(Arm { pat, body });
                }
                Ok(Expr::Match(Box::new(scrut), arms))
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
                if !is_upper(name.rsplit('.').next().unwrap_or(&name)) {
                    return Ok(Expr::Var(name));
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

    fn block(&mut self) -> R<Expr> {
        self.expect_sym("{")?;
        let old = self.no_struct;
        self.no_struct = false;
        let mut stmts = Vec::new();
        while !self.is_sym("}") {
            if self.eat_kw("let") {
                if self.eat_kw("mut") {
                    let name = self.ident()?;
                    let ty = if self.eat_sym(":") { Some(self.type_expr()?) } else { None };
                    self.expect_sym("=")?;
                    stmts.push(Stmt::Var(name, ty, self.expr()?));
                } else {
                    let pat = self.pattern()?;
                    let ty = if self.eat_sym(":") { Some(self.type_expr()?) } else { None };
                    self.expect_sym("=")?;
                    stmts.push(Stmt::Let(pat, ty, self.expr()?));
                }
            } else if self.eat_kw("assert") {
                stmts.push(Stmt::Assert(self.expr()?));
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
                if self.eat_sym("=") {
                    if !matches!(e, Expr::Var(_) | Expr::Field(..)) {
                        return Err(self.err("only a variable or a field (`x`, `x.f`) can be assigned"));
                    }
                    stmts.push(Stmt::Assign(e, self.expr()?));
                } else {
                    stmts.push(Stmt::Expr(e));
                }
            }
            while self.eat_sym(";") {}
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

    fn pattern(&mut self) -> R<Pattern> {
        match self.peek().clone() {
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
                        rest = Some(if self.is_sym("]") { "_".to_string() } else { self.ident()? });
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
