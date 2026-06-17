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
            let (name, span) = self.ident("a type name after ':'")?;
            Ok(Some(TypeAnn { name, span }))
        } else {
            Ok(None)
        }
    }

    // ----- statements -------------------------------------------------------

    fn statement(&mut self) -> Result<Stmt, LuxError> {
        match self.peek_tok() {
            Tok::Let => self.let_stmt(),
            Tok::Var => self.var_stmt(),
            Tok::If => self.if_stmt(),
            Tok::While => self.while_stmt(),
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
        self.or()
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
            _ => self.primary(),
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
            Tok::Ident(name) => {
                self.advance();
                if matches!(self.peek_tok(), Tok::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if !matches!(self.peek_tok(), Tok::RParen) {
                        loop {
                            args.push(self.expression()?);
                            if matches!(self.peek_tok(), Tok::Comma) {
                                self.advance();
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
