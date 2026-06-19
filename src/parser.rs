//! The parser: tokens in, an ast out.
//!
//! This is a hand-written recursive-descent parser. Each grammar rule is one
//! method, and expression precedence is a ladder of methods from loosest
//! (`||`) to tightest (unary `!`/`-`). Hand-rolling keeps the whole frontend
//! legible — you can read exactly how lux understands a program, which is the
//! point of a language built to be learned from.

use crate::ast::*;
use crate::diagnostic::{LuxError, Span};
use crate::lexer::{Tok, Token};

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

/// Parse a full program into a list of statements.
pub fn parse(tokens: Vec<Token>) -> Result<Vec<Stmt>, LuxError> {
    let mut p = Parser { tokens, pos: 0 };
    let mut stmts = Vec::new();
    while !p.at_eof() {
        stmts.push(p.statement()?);
    }
    Ok(stmts)
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_tok(&self) -> &Tok {
        &self.tokens[self.pos].tok
    }

    fn next_tok(&self) -> &Tok {
        let j = (self.pos + 1).min(self.tokens.len() - 1);
        &self.tokens[j].tok
    }

    fn span(&self) -> Span {
        self.peek().span
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek_tok(), Tok::Eof)
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        t
    }

    /// Consume a token of the given kind or produce a helpful error.
    fn expect(&mut self, want: &Tok, what: &str) -> Result<Token, LuxError> {
        if std::mem::discriminant(self.peek_tok()) == std::mem::discriminant(want) {
            Ok(self.advance())
        } else {
            Err(LuxError::new(format!("expected {}", what), self.span()))
        }
    }

    fn ident(&mut self, what: &str) -> Result<(String, Span), LuxError> {
        let t = self.peek().clone();
        if let Tok::Ident(name) = t.tok {
            self.advance();
            Ok((name, t.span))
        } else {
            Err(LuxError::new(format!("expected {}", what), t.span))
        }
    }

    fn optional_type(&mut self) -> Result<Option<TypeAnn>, LuxError> {
        if matches!(self.peek_tok(), Tok::Colon) {
            self.advance();
            Ok(Some(self.parse_type()?))
        } else {
            Ok(None)
        }
    }

    /// Parse a type: a name like `int`, or an array type like `[int]`.
    fn parse_type(&mut self) -> Result<TypeAnn, LuxError> {
        if matches!(self.peek_tok(), Tok::LBracket) {
            let start = self.span();
            self.advance();
            let elem = self.parse_type()?;
            let close = self.expect(&Tok::RBracket, "']' to close the array type")?;
            Ok(TypeAnn {
                kind: TypeKind::Array(Box::new(elem)),
                span: start.to(close.span),
            })
        } else {
            let (name, span) = self.ident("a type name")?;
            // `Name<...>` is a parameterized type, like `Option<int>`.
            if matches!(self.peek_tok(), Tok::Lt) {
                self.advance(); // <
                let mut args = Vec::new();
                loop {
                    args.push(self.parse_type()?);
                    if matches!(self.peek_tok(), Tok::Comma) {
                        self.advance();
                        // A trailing comma is allowed: stop if the list closed.
                        if matches!(self.peek_tok(), Tok::Gt) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                let close = self.expect(&Tok::Gt, "'>' to close the type parameters")?;
                Ok(TypeAnn {
                    kind: TypeKind::Generic(name, args),
                    span: span.to(close.span),
                })
            } else {
                Ok(TypeAnn {
                    kind: TypeKind::Named(name),
                    span,
                })
            }
        }
    }

    // ----- statements -------------------------------------------------------

    fn statement(&mut self) -> Result<Stmt, LuxError> {
        match self.peek_tok() {
            Tok::Let => self.let_stmt(),
            Tok::Var => self.var_stmt(),
            Tok::Func => self.func_stmt(),
            Tok::Return => self.return_stmt(),
            Tok::Struct => self.struct_stmt(),
            Tok::Enum => self.enum_stmt(),
            Tok::If => self.if_stmt(),
            Tok::While => self.while_stmt(),
            Tok::For => self.for_stmt(),
            Tok::Ident(_) if self.assign_ahead() => self.assign_stmt(),
            _ => Ok(Stmt::Expr(self.expression()?)),
        }
    }

    /// True when the current ident is followed by an assignment operator.
    fn assign_ahead(&self) -> bool {
        matches!(self.next_tok(), Tok::Eq | Tok::PlusEq | Tok::MinusEq)
    }

    fn let_stmt(&mut self) -> Result<Stmt, LuxError> {
        let start = self.span();
        self.advance(); // let
        let (name, _) = self.ident("a name after 'let'")?;
        let ty = self.optional_type()?;
        self.expect(&Tok::Eq, "'=' and a value ('let' always needs a value)")?;
        let value = self.expression()?;
        let span = start.to(value.span());
        Ok(Stmt::Let {
            name,
            ty,
            value,
            span,
        })
    }

    fn var_stmt(&mut self) -> Result<Stmt, LuxError> {
        let start = self.span();
        self.advance(); // var
        let (name, name_span) = self.ident("a name after 'var'")?;
        let ty = self.optional_type()?;
        let (value, end) = if matches!(self.peek_tok(), Tok::Eq) {
            self.advance();
            let v = self.expression()?;
            let sp = v.span();
            (Some(v), sp)
        } else {
            let end = ty.as_ref().map(|t| t.span).unwrap_or(name_span);
            (None, end)
        };
        Ok(Stmt::Var {
            name,
            ty,
            value,
            span: start.to(end),
        })
    }

    fn func_stmt(&mut self) -> Result<Stmt, LuxError> {
        let start = self.span();
        self.advance(); // func
        let (name, _) = self.ident("a function name after 'func'")?;
        let params = self.params()?;
        let ret = if matches!(self.peek_tok(), Tok::Arrow) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.block()?;
        Ok(Stmt::Func {
            name,
            params,
            ret,
            body,
            span: start,
        })
    }

    /// Parse a parenthesized, comma-separated list of `name: type` parameters.
    fn params(&mut self) -> Result<Vec<Param>, LuxError> {
        self.expect(&Tok::LParen, "'(' to start the parameter list")?;
        let mut params = Vec::new();
        if !matches!(self.peek_tok(), Tok::RParen) {
            loop {
                let (name, name_span) = self.ident("a parameter name")?;
                self.expect(&Tok::Colon, "':' and a type for the parameter")?;
                let ty = self.parse_type()?;
                let span = name_span.to(ty.span);
                params.push(Param { name, ty, span });
                if matches!(self.peek_tok(), Tok::Comma) {
                    self.advance();
                    if matches!(self.peek_tok(), Tok::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.expect(&Tok::RParen, "')' to close the parameter list")?;
        Ok(params)
    }

    fn struct_stmt(&mut self) -> Result<Stmt, LuxError> {
        let start = self.span();
        self.advance(); // struct
        let (name, _) = self.ident("a struct name after 'struct'")?;
        self.expect(&Tok::LBrace, "'{' to start the struct body")?;
        let mut fields = Vec::new();
        while !matches!(self.peek_tok(), Tok::RBrace | Tok::Eof) {
            fields.push(self.field_def()?);
            // Fields are separated by newlines; a comma between them is allowed.
            if matches!(self.peek_tok(), Tok::Comma) {
                self.advance();
            }
        }
        self.expect(&Tok::RBrace, "'}' to close the struct")?;
        Ok(Stmt::Struct {
            name,
            fields,
            span: start,
        })
    }

    fn enum_stmt(&mut self) -> Result<Stmt, LuxError> {
        let start = self.span();
        self.advance(); // enum
        let (name, _) = self.ident("an enum name after 'enum'")?;
        self.expect(&Tok::LBrace, "'{' to start the enum body")?;
        let mut variants = Vec::new();
        while !matches!(self.peek_tok(), Tok::RBrace | Tok::Eof) {
            let (vname, vspan) = self.ident("a case name")?;
            let mut fields = Vec::new();
            if matches!(self.peek_tok(), Tok::LParen) {
                self.advance(); // (
                if !matches!(self.peek_tok(), Tok::RParen) {
                    loop {
                        fields.push(self.field_def()?);
                        if matches!(self.peek_tok(), Tok::Comma) {
                            self.advance();
                            if matches!(self.peek_tok(), Tok::RParen) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&Tok::RParen, "')' to close the case's values")?;
            }
            variants.push(VariantDef {
                name: vname,
                fields,
                span: vspan,
            });
            if matches!(self.peek_tok(), Tok::Comma) {
                self.advance();
            }
        }
        self.expect(&Tok::RBrace, "'}' to close the enum")?;
        Ok(Stmt::Enum {
            name,
            variants,
            span: start,
        })
    }

    /// Parse one `name: type` field declaration.
    fn field_def(&mut self) -> Result<FieldDef, LuxError> {
        let (name, name_span) = self.ident("a field name")?;
        self.expect(&Tok::Colon, "':' and a type for the field")?;
        let ty = self.parse_type()?;
        let span = name_span.to(ty.span);
        Ok(FieldDef { name, ty, span })
    }

    fn return_stmt(&mut self) -> Result<Stmt, LuxError> {
        let start = self.span();
        self.advance(); // return
        // A bare `return` is one that ends its block; anything else is a value.
        if matches!(self.peek_tok(), Tok::RBrace | Tok::Eof) {
            Ok(Stmt::Return {
                value: None,
                span: start,
            })
        } else {
            let value = self.expression()?;
            let span = start.to(value.span());
            Ok(Stmt::Return {
                value: Some(value),
                span,
            })
        }
    }

    fn for_stmt(&mut self) -> Result<Stmt, LuxError> {
        let start = self.span();
        self.advance(); // for
        let (var, _) = self.ident("a loop variable after 'for'")?;
        self.expect(&Tok::In, "'in' after the loop variable")?;
        let iter = self.expression()?;
        let body = self.block()?;
        Ok(Stmt::For {
            var,
            iter,
            body,
            span: start,
        })
    }

    fn assign_stmt(&mut self) -> Result<Stmt, LuxError> {
        let (name, name_span) = self.ident("a name")?;
        let op = match self.peek_tok() {
            Tok::Eq => AssignOp::Set,
            Tok::PlusEq => AssignOp::Add,
            Tok::MinusEq => AssignOp::Sub,
            _ => unreachable!("assign_ahead guaranteed an assignment operator"),
        };
        self.advance();
        let value = self.expression()?;
        let span = name_span.to(value.span());
        Ok(Stmt::Assign {
            name,
            name_span,
            op,
            value,
            span,
        })
    }

    fn if_stmt(&mut self) -> Result<Stmt, LuxError> {
        let start = self.span();
        self.advance(); // if
        let cond = self.expression()?;
        let then_body = self.block()?;
        let mut else_body = None;
        if matches!(self.peek_tok(), Tok::Else) {
            self.advance();
            if matches!(self.peek_tok(), Tok::If) {
                else_body = Some(vec![self.if_stmt()?]);
            } else {
                else_body = Some(self.block()?);
            }
        }
        Ok(Stmt::If {
            cond,
            then_body,
            else_body,
            span: start,
        })
    }

    fn while_stmt(&mut self) -> Result<Stmt, LuxError> {
        let start = self.span();
        self.advance(); // while
        let cond = self.expression()?;
        let body = self.block()?;
        Ok(Stmt::While {
            cond,
            body,
            span: start,
        })
    }

    fn block(&mut self) -> Result<Vec<Stmt>, LuxError> {
        self.expect(&Tok::LBrace, "'{' to start a block")?;
        let mut stmts = Vec::new();
        while !matches!(self.peek_tok(), Tok::RBrace | Tok::Eof) {
            stmts.push(self.statement()?);
        }
        self.expect(&Tok::RBrace, "'}' to close the block")?;
        Ok(stmts)
    }

    // ----- expressions (precedence ladder, loosest first) -------------------

    fn expression(&mut self) -> Result<Expr, LuxError> {
        self.range()
    }

    /// `a..b` — a half-open range. Loosest precedence, and non-chaining: you
    /// write one `..`, not `a..b..c`.
    fn range(&mut self) -> Result<Expr, LuxError> {
        let lhs = self.or()?;
        if matches!(self.peek_tok(), Tok::DotDot) {
            self.advance();
            let rhs = self.or()?;
            let span = lhs.span().to(rhs.span());
            Ok(Expr::Range {
                start: Box::new(lhs),
                end: Box::new(rhs),
                span,
            })
        } else {
            Ok(lhs)
        }
    }

    fn or(&mut self) -> Result<Expr, LuxError> {
        let mut lhs = self.and()?;
        while matches!(self.peek_tok(), Tok::OrOr) {
            self.advance();
            let rhs = self.and()?;
            lhs = binary(BinOp::Or, lhs, rhs);
        }
        Ok(lhs)
    }

    fn and(&mut self) -> Result<Expr, LuxError> {
        let mut lhs = self.equality()?;
        while matches!(self.peek_tok(), Tok::AndAnd) {
            self.advance();
            let rhs = self.equality()?;
            lhs = binary(BinOp::And, lhs, rhs);
        }
        Ok(lhs)
    }

    fn equality(&mut self) -> Result<Expr, LuxError> {
        let mut lhs = self.comparison()?;
        loop {
            let op = match self.peek_tok() {
                Tok::EqEq => BinOp::Eq,
                Tok::NotEq => BinOp::Ne,
                _ => break,
            };
            self.advance();
            let rhs = self.comparison()?;
            lhs = binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn comparison(&mut self) -> Result<Expr, LuxError> {
        let mut lhs = self.term()?;
        loop {
            let op = match self.peek_tok() {
                Tok::Lt => BinOp::Lt,
                Tok::Gt => BinOp::Gt,
                Tok::Le => BinOp::Le,
                Tok::Ge => BinOp::Ge,
                _ => break,
            };
            self.advance();
            let rhs = self.term()?;
            lhs = binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn term(&mut self) -> Result<Expr, LuxError> {
        let mut lhs = self.factor()?;
        loop {
            let op = match self.peek_tok() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.factor()?;
            lhs = binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn factor(&mut self) -> Result<Expr, LuxError> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek_tok() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let rhs = self.unary()?;
            lhs = binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, LuxError> {
        let t = self.peek().clone();
        match t.tok {
            Tok::Bang => {
                self.advance();
                let rhs = self.unary()?;
                let span = t.span.to(rhs.span());
                Ok(Expr::Unary {
                    op: UnOp::Not,
                    rhs: Box::new(rhs),
                    span,
                })
            }
            Tok::Minus => {
                self.advance();
                let rhs = self.unary()?;
                let span = t.span.to(rhs.span());
                Ok(Expr::Unary {
                    op: UnOp::Neg,
                    rhs: Box::new(rhs),
                    span,
                })
            }
            _ => self.postfix(),
        }
    }

    /// A primary expression followed by any number of `[index]` lookups and
    /// `.field` accesses. A `.case(...)` after a bare name is enum construction.
    fn postfix(&mut self) -> Result<Expr, LuxError> {
        let mut e = self.primary()?;
        loop {
            match self.peek_tok() {
                Tok::LBracket => {
                    self.advance();
                    let index = self.expression()?;
                    let close = self.expect(&Tok::RBracket, "']' to close the index")?;
                    let span = e.span().to(close.span);
                    e = Expr::Index {
                        base: Box::new(e),
                        index: Box::new(index),
                        span,
                    };
                }
                Tok::Dot => {
                    self.advance();
                    let (field, field_span) = self.ident("a field or case name after '.'")?;
                    if matches!(self.peek_tok(), Tok::LParen) {
                        // `Name.case(label: value, ...)` — enum construction. The
                        // thing before the dot must be a bare enum name.
                        let enum_name = match &e {
                            Expr::Ident(n, _) => n.clone(),
                            _ => {
                                return Err(LuxError::new(
                                    "only an enum name can be used like `Name.case(...)`",
                                    e.span(),
                                )
                                .with_learn(
                                    "enums",
                                    "`Shape.circle(...)` builds one case of an enum",
                                ));
                            }
                        };
                        self.advance(); // (
                        let fields = self.label_list()?;
                        let close = self.expect(&Tok::RParen, "')' to close the case's values")?;
                        let span = e.span().to(close.span);
                        e = Expr::EnumLit {
                            enum_name,
                            variant: field,
                            fields,
                            span,
                        };
                    } else {
                        let span = e.span().to(field_span);
                        e = Expr::Field {
                            base: Box::new(e),
                            field,
                            span,
                        };
                    }
                }
                _ => break,
            }
        }
        Ok(e)
    }

    /// Parse a parenthesized list of `name: value` pairs, with the opening `(`
    /// already consumed and the closing `)` left for the caller. Shared by
    /// struct literals and enum construction.
    fn label_list(&mut self) -> Result<Vec<(String, Expr)>, LuxError> {
        let mut fields = Vec::new();
        if !matches!(self.peek_tok(), Tok::RParen) {
            loop {
                let (name, _) = self.ident("a field name")?;
                self.expect(&Tok::Colon, "':' after the field name")?;
                let value = self.expression()?;
                fields.push((name, value));
                if matches!(self.peek_tok(), Tok::Comma) {
                    self.advance();
                    if matches!(self.peek_tok(), Tok::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        Ok(fields)
    }

    /// `match scrutinee { pattern => expr ... }`.
    fn match_expr(&mut self) -> Result<Expr, LuxError> {
        let start = self.span();
        self.advance(); // match
        let scrutinee = self.expression()?;
        self.expect(&Tok::LBrace, "'{' to start the match arms")?;
        let mut arms = Vec::new();
        while !matches!(self.peek_tok(), Tok::RBrace | Tok::Eof) {
            let pattern = self.pattern()?;
            self.expect(&Tok::FatArrow, "'=>' after the pattern")?;
            let body = self.expression()?;
            let span = pattern.span().to(body.span());
            arms.push(MatchArm {
                pattern,
                body,
                span,
            });
        }
        let close = self.expect(&Tok::RBrace, "'}' to close the match")?;
        Ok(Expr::Match {
            scrutinee: Box::new(scrutinee),
            arms,
            span: start.to(close.span),
        })
    }

    /// Parse one match pattern: a literal, an enum case (with optional captured
    /// names), or `_`.
    fn pattern(&mut self) -> Result<Pattern, LuxError> {
        let t = self.peek().clone();
        match t.tok {
            Tok::Int(v) => {
                self.advance();
                Ok(Pattern::Int(v, t.span))
            }
            Tok::Minus => {
                self.advance();
                let nt = self.peek().clone();
                if let Tok::Int(v) = nt.tok {
                    self.advance();
                    Ok(Pattern::Int(-v, t.span.to(nt.span)))
                } else {
                    Err(LuxError::new(
                        "expected a number after '-' in a pattern",
                        nt.span,
                    ))
                }
            }
            Tok::Str(s) => {
                self.advance();
                Ok(Pattern::Str(s, t.span))
            }
            Tok::True => {
                self.advance();
                Ok(Pattern::Bool(true, t.span))
            }
            Tok::False => {
                self.advance();
                Ok(Pattern::Bool(false, t.span))
            }
            Tok::Ident(name) => {
                self.advance();
                if name == "_" {
                    Ok(Pattern::Wildcard(t.span))
                } else if matches!(self.peek_tok(), Tok::LParen) {
                    self.advance(); // (
                    let mut bindings = Vec::new();
                    if !matches!(self.peek_tok(), Tok::RParen) {
                        loop {
                            self.expect(
                                &Tok::Let,
                                "'let' before each captured name, like circle(let r)",
                            )?;
                            let (b, _) = self.ident("a name to bind")?;
                            bindings.push(b);
                            if matches!(self.peek_tok(), Tok::Comma) {
                                self.advance();
                                if matches!(self.peek_tok(), Tok::RParen) {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    }
                    let close = self.expect(&Tok::RParen, "')' to close the pattern")?;
                    Ok(Pattern::Variant {
                        name,
                        bindings,
                        span: t.span.to(close.span),
                    })
                } else {
                    Ok(Pattern::Variant {
                        name,
                        bindings: Vec::new(),
                        span: t.span,
                    })
                }
            }
            _ => Err(LuxError::new("expected a pattern", t.span)
                .with_note("a pattern is a case name, a literal (int, string, true/false), or `_`")
                .with_learn(
                    "match",
                    "each arm starts with a pattern — what it's looking for",
                )),
        }
    }

    fn primary(&mut self) -> Result<Expr, LuxError> {
        let t = self.peek().clone();
        match t.tok {
            Tok::Int(v) => {
                self.advance();
                Ok(Expr::Int(v, t.span))
            }
            Tok::Float(v) => {
                self.advance();
                Ok(Expr::Float(v, t.span))
            }
            Tok::Str(s) => {
                self.advance();
                Ok(Expr::Str(s, t.span))
            }
            Tok::True => {
                self.advance();
                Ok(Expr::Bool(true, t.span))
            }
            Tok::False => {
                self.advance();
                Ok(Expr::Bool(false, t.span))
            }
            Tok::LParen => {
                self.advance();
                let e = self.expression()?;
                self.expect(&Tok::RParen, "')' to close the group")?;
                Ok(e)
            }
            Tok::LBracket => {
                self.advance();
                let mut elems = Vec::new();
                if !matches!(self.peek_tok(), Tok::RBracket) {
                    loop {
                        elems.push(self.expression()?);
                        if matches!(self.peek_tok(), Tok::Comma) {
                            self.advance();
                            // A trailing comma is allowed: stop if the list closed.
                            if matches!(self.peek_tok(), Tok::RBracket) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                let close = self.expect(&Tok::RBracket, "']' to close the array")?;
                Ok(Expr::Array(elems, t.span.to(close.span)))
            }
            Tok::Match => self.match_expr(),
            Tok::Ident(name) => {
                self.advance();
                if matches!(self.peek_tok(), Tok::LParen) {
                    self.advance(); // (
                    // `Name(field: ...)` is a struct literal; `Name(value, ...)`
                    // is a function call. Labelled fields have a `:` after the
                    // first name, which never appears in a positional argument.
                    if matches!(self.peek_tok(), Tok::Ident(_))
                        && matches!(self.next_tok(), Tok::Colon)
                    {
                        let fields = self.label_list()?;
                        let close = self.expect(&Tok::RParen, "')' to close the struct fields")?;
                        Ok(Expr::StructLit {
                            name,
                            fields,
                            span: t.span.to(close.span),
                        })
                    } else {
                        let mut args = Vec::new();
                        if !matches!(self.peek_tok(), Tok::RParen) {
                            loop {
                                args.push(self.expression()?);
                                if matches!(self.peek_tok(), Tok::Comma) {
                                    self.advance();
                                    if matches!(self.peek_tok(), Tok::RParen) {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                        let close = self.expect(&Tok::RParen, "')' to close the call")?;
                        Ok(Expr::Call {
                            name,
                            args,
                            span: t.span.to(close.span),
                        })
                    }
                } else {
                    Ok(Expr::Ident(name, t.span))
                }
            }
            _ => Err(LuxError::new("expected a value", t.span)),
        }
    }
}

/// Build a binary expression spanning both sides.
fn binary(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    let span = lhs.span().to(rhs.span());
    Expr::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span,
    }
}
