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
        decls.push(p.decl()?);
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
    "type", "effect", "fn", "test", "let", "if", "else", "match", "handle", "with", "mock", "where",
    "requires", "ensures", "true", "false", "is", "assert",
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

    pub fn decl(&mut self) -> R<Decl> {
        let mut doc = Vec::new();
        while let Tok::Doc(d) = self.peek().clone() {
            doc.push(d);
            self.next();
        }
        let kind = if self.eat_kw("type") {
            DeclKind::Type(self.type_decl()?)
        } else if self.eat_kw("effect") {
            DeclKind::Effect(self.effect_decl()?)
        } else if self.eat_kw("fn") {
            DeclKind::Fn(self.fn_decl()?)
        } else if self.eat_kw("test") {
            let name = match self.next() {
                Tok::Str(s) => s,
                _ => return Err(self.err("expected test name string")),
            };
            DeclKind::Test(TestDecl { name, body: self.block()? })
        } else {
            return Err(self.err("expected `type`, `effect`, `fn` or `test`"));
        };
        Ok(Decl { doc, kind })
    }

    fn type_params(&mut self) -> R<Vec<String>> {
        if self.eat_sym("[") { self.list("]", |p| p.ident()) } else { Ok(vec![]) }
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
            TypeBody::Alias(self.type_expr()?)
        };
        let refinement = if self.eat_kw("where") { Some(self.expr()?) } else { None };
        Ok(TypeDecl { name, params, body, refinement })
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

    pub fn type_expr(&mut self) -> R<TypeExpr> {
        if self.eat_sym("(") {
            let args = self.list(")", |p| p.type_expr())?;
            if self.eat_sym("->") {
                return Ok(TypeExpr::Fn(args, Box::new(self.type_expr()?)));
            }
            return match args.len() {
                0 => Ok(TypeExpr::Unit),
                1 => Ok(args.into_iter().next().unwrap()),
                _ => Err(self.err("tuples are not supported yet")),
            };
        }
        let name = self.ident()?;
        let args = if self.eat_sym("[") { self.list("]", |p| p.type_expr())? } else { vec![] };
        Ok(TypeExpr::Named(name, args))
    }

    fn params(&mut self) -> R<Vec<Param>> {
        self.expect_sym("(")?;
        self.list(")", |p| {
            let name = p.ident()?;
            p.expect_sym(":")?;
            Ok(Param { name, ty: p.type_expr()? })
        })
    }

    fn effect_decl(&mut self) -> R<EffectDecl> {
        let name = self.ident()?;
        let params = self.type_params()?;
        self.expect_sym("{")?;
        let mut ops = Vec::new();
        while !self.eat_sym("}") {
            let mut doc = Vec::new();
            while let Tok::Doc(d) = self.peek().clone() {
                doc.push(d);
                self.next();
            }
            self.expect_kw("fn")?;
            let oname = self.ident()?;
            let params = self.params()?;
            let ret = if self.eat_sym("->") { self.type_expr()? } else { TypeExpr::Unit };
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
            ops.push(OpSig { doc, name: oname, params, ret, requires, ensures });
            self.eat_sym(",");
            self.eat_sym(";");
        }
        Ok(EffectDecl { name, params, ops })
    }

    fn fn_decl(&mut self) -> R<FnDecl> {
        let name = self.ident()?;
        let type_params = self.type_params()?;
        let params = self.params()?;
        let ret = if self.eat_sym("->") { self.type_expr()? } else { TypeExpr::Unit };
        let mut effects = Vec::new();
        if self.eat_sym("/") {
            self.expect_sym("{")?;
            effects = self.list("}", |p| p.type_expr())?;
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
        let body = if self.eat_sym("=") { self.expr()? } else { self.block()? };
        Ok(FnDecl { name, type_params, params, ret, effects, requires, ensures, body })
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
                let ctor = self.ident()?;
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
            Tok::Ident(kw) if kw == "handle" => {
                self.next();
                let body = self.expr_ns()?;
                let mut handlers = Vec::new();
                while self.eat_kw("with") {
                    if self.eat_kw("mock") {
                        handlers.push(Handler { effect: self.ident()?, kind: HandlerKind::MockAuto, state: None, on_return: None });
                        continue;
                    }
                    let effect = self.ident()?;
                    let state = if self.eat_sym("(") {
                        let v = self.ident()?;
                        self.expect_sym("=")?;
                        let init = self.expr()?;
                        self.expect_sym(")")?;
                        Some((v, init))
                    } else {
                        None
                    };
                    self.expect_sym("{")?;
                    let mut clauses = Vec::new();
                    let mut on_return = None;
                    while !self.eat_sym("}") {
                        let op = self.ident()?;
                        self.expect_sym("(")?;
                        let params = self.list(")", |p| p.ident())?;
                        self.expect_sym("=>")?;
                        let body = self.expr()?;
                        self.eat_sym(",");
                        if op == "return" {
                            if params.len() != 1 {
                                return Err(self.err("`return` clause takes exactly one parameter"));
                            }
                            on_return = Some((params[0].clone(), body));
                        } else {
                            clauses.push(HandlerClause { op, params, body });
                        }
                    }
                    handlers.push(Handler { effect, kind: HandlerKind::Clauses(clauses), state, on_return });
                }
                if handlers.is_empty() {
                    return Err(self.err("expected `with` after handled expression"));
                }
                Ok(Expr::Handle(Box::new(body), handlers))
            }
            Tok::Ident(name) if !KEYWORDS.contains(&name.as_str()) => {
                self.next();
                if !is_upper(&name) {
                    return Ok(Expr::Var(name));
                }
                // Effect operation: `Db.load(x)`
                if self.is_sym(".") {
                    if let (Tok::Ident(op), Tok::Sym("(")) = (self.peek_at(1).clone(), self.peek_at(2)) {
                        if !is_upper(&op) {
                            self.next();
                            self.next();
                            self.next();
                            return Ok(Expr::Perform(name, op, self.args()?));
                        }
                    }
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
                let pat = self.pattern()?;
                let ty = if self.eat_sym(":") { Some(self.type_expr()?) } else { None };
                self.expect_sym("=")?;
                stmts.push(Stmt::Let(pat, ty, self.expr()?));
            } else if self.eat_kw("assert") {
                stmts.push(Stmt::Assert(self.expr()?));
            } else {
                stmts.push(Stmt::Expr(self.expr()?));
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
            Tok::Ident(s) if is_upper(&s) => {
                self.next();
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
