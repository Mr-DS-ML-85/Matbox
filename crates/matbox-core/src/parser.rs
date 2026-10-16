//! Recursive-descent + Pratt parser for the MATBOX language.
//! Emulates MATLAB operator precedence: unary > ^ > */\ > +/- > : >
//! relational > & > | > && > ||.

use crate::ast::*;
use crate::lexer::{Lexer, Tok};

pub struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    pub fn new(src: &str) -> Result<Self, String> {
        let mut lex = Lexer::new(src);
        lex.lex()?;
        Ok(Parser { toks: lex.tokens, pos: 0 })
    }

    fn peek(&self) -> &Tok {
        self.toks.get(self.pos).unwrap_or(&Tok::Eof)
    }

    fn bump(&mut self) -> Tok {
        let t = self.peek().clone();
        if !matches!(t, Tok::Eof) { self.pos += 1; }
        t
    }

    fn skip_nl(&mut self) {
        while matches!(self.peek(), Tok::Newline) { self.pos += 1; }
    }

    pub fn parse_program(&mut self) -> Result<Vec<Stmt>, String> {
        let mut stmts = Vec::new();
        loop {
            self.skip_nl();
            while matches!(self.peek(), Tok::Semicolon | Tok::Comma | Tok::Newline) {
                self.pos += 1;
            }
            if matches!(self.peek(), Tok::Eof) { break; }
            let s = self.parse_statement()?;
            stmts.push(s);
        }
        Ok(stmts)
    }

    fn parse_statement(&mut self) -> Result<Stmt, String> {
        self.skip_nl();
        match self.peek() {
            Tok::Kw(k) if k == "if" => self.parse_if(),
            Tok::Kw(k) if k == "for" => self.parse_for(),
            Tok::Kw(k) if k == "while" => self.parse_while(),
            Tok::Kw(k) if k == "switch" => self.parse_switch(),
            Tok::Kw(k) if k == "try" => self.parse_try(),
            Tok::Kw(k) if k == "return" => { self.pos += 1; Ok(Stmt::Return) }
            Tok::Kw(k) if k == "break" => { self.pos += 1; Ok(Stmt::Break) }
            Tok::Kw(k) if k == "continue" => { self.pos += 1; Ok(Stmt::Continue) }
            Tok::Kw(k) if k == "function" => self.parse_function(),
            Tok::Kw(k) if k == "parfor" => self.parse_parfor(),
            Tok::Kw(k) if k == "persistent" => self.parse_persistent(),
            Tok::Kw(k) if k == "global" => self.parse_global(),
            _ => {
                // Command syntax: `clear a b`, `format long`, `hold on`. A known
                // command builtin directly followed (on the same line) by a bareword
                // or string argument is invoked with those words as string arguments.
                if let Tok::Ident(name) = self.peek() {
                    let name = name.clone();
                    if is_command_builtin(&name)
                        && matches!(self.toks.get(self.pos + 1), Some(Tok::Ident(_)) | Some(Tok::Str(_)) | Some(Tok::DStr(_)))
                    {
                        return self.parse_command(name);
                    }
                }
                let lhs = self.parse_expr(0)?;
                if matches!(self.peek(), Tok::Assign) {
                    self.pos += 1;
                    let rhs = self.parse_expr(0)?;
                    let target = to_lvalue(lhs)?;
                    Ok(Stmt::Assign { target, value: rhs })
                } else {
                    Ok(Stmt::Expr(lhs))
                }
            }
        }
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // if
        let cond = self.parse_expr(0)?;
        let mut body = Vec::new();
        let mut elseif = Vec::new();
        let mut else_body = Vec::new();
        // skip semicolons/comma after condition (MATLAB allows `if cond; ...`)
        while matches!(self.peek(), Tok::Semicolon | Tok::Comma | Tok::Newline) { self.pos += 1; }
        // parse body until elseif/else/end
        loop {
            self.skip_nl();
            while matches!(self.peek(), Tok::Semicolon | Tok::Comma) { self.pos += 1; }
            self.skip_nl();
            match self.peek() {
                Tok::Kw(k) if k == "elseif" => {
                    self.pos += 1;
                    let c = self.parse_expr(0)?;
                    let b = self.parse_block_until(vec!["elseif", "else", "end"])?;
                    elseif.push((c, b));
                }
                Tok::Kw(k) if k == "else" => {
                    self.pos += 1;
                    else_body = self.parse_block_until(vec!["end"])?;
                }
                Tok::Kw(k) if k == "end" => { self.pos += 1; break; }
                Tok::Eof => return Err("unexpected EOF in if-statement".into()),
                _ => body.push(self.parse_statement()?),
            }
        }
        Ok(Stmt::If { cond, body, elseif, else_body })
    }

    fn parse_block_until(&mut self, stops: Vec<&str>) -> Result<Vec<Stmt>, String> {
        let mut body = Vec::new();
        loop {
            self.skip_nl();
            while matches!(self.peek(), Tok::Semicolon | Tok::Comma) { self.pos += 1; }
            self.skip_nl();
            if matches!(self.peek(), Tok::Eof) { return Err("unexpected EOF in block".into()); }
            if let Tok::Kw(k) = self.peek() {
                if stops.contains(&k.as_str()) { break; }
            }
            body.push(self.parse_statement()?);
        }
        Ok(body)
    }

    fn parse_for(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // for
        let var = match self.bump() {
            Tok::Ident(s) => s,
            _ => return Err("expected loop variable after 'for'".into()),
        };
        if !matches!(self.peek(), Tok::Assign) { return Err("expected '=' in for-loop".into()); }
        self.pos += 1;
        let range = self.parse_expr(0)?;
        while matches!(self.peek(), Tok::Semicolon | Tok::Comma | Tok::Newline) { self.pos += 1; }
        let body = self.parse_block_until(vec!["end"])?;
        Ok(Stmt::For { var, range, body })
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // while
        let cond = self.parse_expr(0)?;
        while matches!(self.peek(), Tok::Semicolon | Tok::Comma | Tok::Newline) { self.pos += 1; }
        let body = self.parse_block_until(vec!["end"])?;
        Ok(Stmt::While { cond, body })
    }

    fn parse_parfor(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // parfor
        let var = match self.bump() {
            Tok::Ident(s) => s,
            t => return Err(format!("expected identifier after parfor, got {:?}", t)),
        };
        if !matches!(self.bump(), Tok::Assign) {
            return Err("expected '=' after parfor variable".into());
        }
        let range = self.parse_expr(0)?;
        while matches!(self.peek(), Tok::Semicolon | Tok::Comma | Tok::Newline) { self.pos += 1; }
        let body = self.parse_block_until(vec!["end"])?;
        Ok(Stmt::Parfor { var, range, body })
    }

    fn parse_persistent(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // persistent
        let vars = self.parse_decl_names("persistent")?;
        Ok(Stmt::Persistent(vars))
    }

    fn parse_global(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // global
        let vars = self.parse_decl_names("global")?;
        Ok(Stmt::Global(vars))
    }

    /// Parse the variable list of a `persistent`/`global` declaration: one or more
    /// names separated by spaces or commas, terminated by a newline/semicolon.
    fn parse_decl_names(&mut self, kw: &str) -> Result<Vec<String>, String> {
        let mut vars = Vec::new();
        loop {
            match self.peek() {
                Tok::Ident(s) => { vars.push(s.clone()); self.pos += 1; }
                Tok::Comma => { self.pos += 1; } // optional separator between names
                _ => break, // newline, semicolon, eof, etc. end the declaration
            }
        }
        if vars.is_empty() {
            return Err(format!("expected identifier after {}, got {:?}", kw, self.peek()));
        }
        Ok(vars)
    }

    fn parse_switch(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // switch
        let expr = self.parse_expr(0)?;
        let mut cases = Vec::new();
        let mut otherwise = Vec::new();
        let mut done = false;
        while !done {
            self.skip_nl();
            while matches!(self.peek(), Tok::Semicolon | Tok::Comma) { self.pos += 1; }
            self.skip_nl();
            match self.peek() {
                Tok::Kw(k) if k == "case" => {
                    self.pos += 1;
                    while matches!(self.peek(), Tok::Comma | Tok::Semicolon | Tok::Newline) {
                        self.pos += 1;
                    }
                    let cexpr = self.parse_expr(0)?;
                    let body = self.parse_block_until(vec!["case", "otherwise", "end"])?;
                    cases.push((cexpr, body));
                }
                Tok::Kw(k) if k == "otherwise" => {
                    self.pos += 1;
                    otherwise = self.parse_block_until(vec!["end"])?;
                }
                Tok::Kw(k) if k == "end" => {
                    self.pos += 1;
                    done = true;
                }
                Tok::Eof => return Err("unexpected EOF in switch".into()),
                _ => return Err(format!("expected case/otherwise/end in switch, found {:?}", self.peek())),
            }
        }
        Ok(Stmt::Switch { expr, cases, otherwise })
    }

    fn parse_function(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // function
        let outputs: Vec<String>;
        let name: String;
        if matches!(self.peek(), Tok::LBracket) {
            // function [a, b] = name(...)
            self.pos += 1;
            let mut list = Vec::new();
            loop {
                match self.bump() {
                    Tok::Ident(s) => list.push(s),
                    Tok::Comma => { continue; }
                    Tok::RBracket => break,
                    other => return Err(format!("expected output name or ']', found {:?}", other)),
                }
            }
            outputs = list;
            if !matches!(self.bump(), Tok::Assign) { return Err("expected '=' after output list".into()); }
            name = match self.bump() {
                Tok::Ident(s) => s,
                other => return Err(format!("expected function name, found {:?}", other)),
            };
        } else {
            // function [name](...) or function name = ... or function name(...)
            let first = match self.bump() {
                Tok::Ident(s) => s,
                other => return Err(format!("expected identifier or '[', found {:?}", other)),
            };
            match self.peek() {
                Tok::Assign => {
                    // function y = name(...)
                    outputs = vec![first];
                    self.pos += 1;
                    name = match self.bump() {
                        Tok::Ident(s) => s,
                        other => return Err(format!("expected function name, found {:?}", other)),
                    };
                }
                Tok::LParen | Tok::Newline | Tok::Semicolon => {
                    // function name(...)  — no outputs
                    outputs = Vec::new();
                    name = first;
                }
                _ => return Err(format!("expected '=' or '(' after identifier in function, found {:?}", self.peek())),
            }
        }
        // Parse input arguments
        let inputs = if matches!(self.peek(), Tok::LParen) {
            self.pos += 1;
            let mut list = Vec::new();
            if !matches!(self.peek(), Tok::RParen) {
                loop {
                    match self.bump() {
                        Tok::Ident(s) => list.push(s),
                        Tok::Comma => continue,
                        Tok::RParen => break,
                        other => return Err(format!("expected input name or ')', found {:?}", other)),
                    }
                }
            } else { self.pos += 1; }
            list
        } else { Vec::new() };
        // skip separators after signature
        while matches!(self.peek(), Tok::Newline | Tok::Semicolon | Tok::Comma) { self.pos += 1; }
        let body = self.parse_block_until(vec!["end", "endfunction"])?;
        match self.peek() {
            Tok::Kw(k) if k == "end" || k == "endfunction" => { self.pos += 1; }
            _ => return Err(format!("expected 'end' or 'endfunction' to close function, found {:?}", self.peek())),
        }
        Ok(Stmt::Function { outputs, name, inputs, body })
    }

    fn parse_try(&mut self) -> Result<Stmt, String> {
        self.pos += 1; // try
        let body = self.parse_block_until(vec!["catch", "end"])?;
        let mut catch_var = None;
        let mut catch_body = Vec::new();
        if let Tok::Kw(k) = self.peek() {
            if k == "catch" {
                self.pos += 1;
                if let Tok::Ident(s) = self.peek() {
                    catch_var = Some(s.clone());
                    self.pos += 1;
                }
                catch_body = self.parse_block_until(vec!["end"])?;
            }
        }
        self.expect_keyword("end")?;
        Ok(Stmt::Try { body, catch_var, catch_body })
    }

    fn expect_keyword(&mut self, kw: &str) -> Result<(), String> {
        match self.peek() {
            Tok::Kw(k) if k == kw => { self.pos += 1; Ok(()) }
            other => Err(format!("expected '{}', found {:?}", kw, other)),
        }
    }

    // ---- expressions ----

    /// Parse an expression that does NOT handle colon operator (for step/end in colon ranges).
    fn parse_expr_step(&mut self) -> Result<Expr, String> {
        self.skip_nl();
        let mut lhs = self.parse_unary()?;
        loop {
            self.skip_nl();
            if matches!(self.peek(), Tok::Colon) { break; }
            let (op, bp, right) = match binop(self.peek()) {
                Some(x) => x,
                None => break,
            };
            self.pos += 1;
            let next_min = if right { bp } else { bp + 1 };
            let rhs = self.parse_expr(next_min)?;
            lhs = Expr::Binary { op, left: Box::new(lhs), right: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_expr(&mut self, min_bp: u8) -> Result<Expr, String> {
        self.skip_nl();
        let mut lhs = self.parse_unary()?;
        loop {
            self.skip_nl();
            // colon has very low precedence (10) — lower than +/-
            if matches!(self.peek(), Tok::Colon) {
                if 10 < min_bp { break; }
                self.pos += 1;
                // Parse step/end WITHOUT consuming a second colon in the middle.
                let middle = self.parse_expr_step()?;
                if matches!(self.peek(), Tok::Colon) {
                    self.pos += 1;
                    let end = self.parse_expr(0)?;
                    lhs = Expr::ColonRange { start: Box::new(lhs), step: Some(Box::new(middle)), end: Box::new(end) };
                } else {
                    lhs = Expr::ColonRange { start: Box::new(lhs), step: None, end: Box::new(middle) };
                }
                continue;
            }
            let (op, bp, right) = match binop(self.peek()) {
                Some(x) => x,
                None => break,
            };
            if bp < min_bp { break; }
            self.pos += 1;
            let next_min = if right { bp } else { bp + 1 };
            let rhs = self.parse_expr(next_min)?;
            lhs = Expr::Binary { op, left: Box::new(lhs), right: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        self.skip_nl();
        match self.peek() {
            // A leading sign binds LOOSER than `^` but TIGHTER than `*`: `-2^2`
            // groups as `-(2^2)`, not `(-2)^2`. So the operand is parsed as a
            // full expression at the sign's own precedence level instead of
            // recursing straight into another unary — `parse_expr(111)` absorbs
            // `^` (120) but stops before `*` (110), and leaves `:` alone so
            // `-a:b` keeps its existing grouping.
            Tok::Minus => { self.pos += 1; Ok(Expr::Unary { op: UnOp::Neg, expr: Box::new(self.parse_expr(UNARY_SIGN_BP)?) }) }
            Tok::Plus => { self.pos += 1; Ok(Expr::Unary { op: UnOp::Pos, expr: Box::new(self.parse_expr(UNARY_SIGN_BP)?) }) }
            Tok::Not => { self.pos += 1; Ok(Expr::Unary { op: UnOp::Not, expr: Box::new(self.parse_unary()?) }) }
            Tok::At => self.parse_anon_func(),
            _ => self.parse_postfix(),
        }
    }

    fn parse_anon_func(&mut self) -> Result<Expr, String> {
        self.pos += 1; // @
        if matches!(self.peek(), Tok::LParen) {
            // @(params) body
            self.pos += 1;
            let mut params = Vec::new();
            if !matches!(self.peek(), Tok::RParen) {
                loop {
                    match self.bump() {
                        Tok::Ident(s) => params.push(s),
                        Tok::Comma => continue,
                        Tok::RParen => break,
                        other => return Err(format!("expected parameter name or ')', found {:?}", other)),
                    }
                }
            } else { self.pos += 1; }
            let body = self.parse_expr(0)?;
            Ok(Expr::AnonFunc { params, body: Box::new(body) })
        } else if let Tok::Ident(name) = self.bump() {
            // @name — function handle reference
            Ok(Expr::FuncRef(name))
        } else {
            Err("expected '(' or function name after '@'".into())
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        self.skip_nl();
        let mut expr = self.parse_primary()?;
        loop {
            self.skip_nl();
            match self.peek() {
                Tok::LParen => {
                    self.pos += 1;
                    let args = self.parse_arg_list(Tok::RParen)?;
                    if let Expr::Ident(name) = &expr {
                        expr = Expr::Call { name: name.clone(), args };
                    } else {
                        expr = Expr::Index { base: Box::new(expr), indices: args };
                    }
                }
                Tok::LBrace => {
                    self.pos += 1;
                    let args = self.parse_arg_list(Tok::RBrace)?;
                    expr = Expr::BraceIndex { base: Box::new(expr), indices: args };
                }
                Tok::CTranspose => { self.pos += 1; expr = Expr::Transpose { expr: Box::new(expr), conj: true }; }
                Tok::Transpose => { self.pos += 1; expr = Expr::Transpose { expr: Box::new(expr), conj: false }; }
                Tok::Dot => {
                    self.pos += 1;
                    if matches!(self.peek(), Tok::LParen) {
                        // dynamic field access: s.(expr)
                        self.pos += 1;
                        let field_expr = self.parse_expr(0)?;
                        if !matches!(self.bump(), Tok::RParen) {
                            return Err("expected ')' after dynamic field name".into());
                        }
                        expr = Expr::DynamicDotAccess { base: Box::new(expr), field_expr: Box::new(field_expr) };
                    } else {
                        let field = match self.bump() {
                            Tok::Ident(s) => s,
                            other => return Err(format!("expected field name after '.', found {:?}", other)),
                        };
                        expr = Expr::DotAccess { base: Box::new(expr), field };
                    }
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_arg_list(&mut self, close: Tok) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if matches!(self.peek(), x if *x == close) { self.pos += 1; return Ok(args); }
        loop {
            let a = self.parse_expr(0)?;
            args.push(a);
            match self.peek() {
                x if *x == close => { self.pos += 1; break; }
                Tok::Comma => { self.pos += 1; if matches!(self.peek(), x if *x == close) { self.pos += 1; break; } }
                _ => return Err(format!("expected ',' or ')' in argument list, found {:?}", self.peek())),
            }
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        self.skip_nl();
        let t = self.peek().clone();
        match t {
            Tok::Num(f) => { self.pos += 1; Ok(Expr::Num(f)) }
            Tok::Imag(f) => { self.pos += 1; Ok(Expr::Imag(f)) }
            Tok::Str(s) => { self.pos += 1; Ok(Expr::Str(s)) }
            Tok::DStr(s) => { self.pos += 1; Ok(Expr::DStr(s)) }
            Tok::Ident(s) => { self.pos += 1; Ok(Expr::Ident(s)) }
            Tok::Colon => { self.pos += 1; Ok(Expr::Colon) }
            Tok::Kw(k) if k == "end" => { self.pos += 1; Ok(Expr::End) }
            Tok::LParen => {
                self.pos += 1;
                let e = self.parse_expr(0)?;
                if !matches!(self.peek(), Tok::RParen) { return Err(format!("expected ')', found {:?}", self.peek())); }
                self.pos += 1;
                Ok(e)
            }
            Tok::LBracket => self.parse_matrix(),
            Tok::LBrace => self.parse_cell(),
            _ => Err(format!("unexpected token in expression: {:?}", t)),
        }
    }

    /// Parse MATLAB command syntax: `cmd word1 word2 ...`, where each bareword or
    /// string token becomes a string argument until the end of the statement.
    fn parse_command(&mut self, name: String) -> Result<Stmt, String> {
        self.pos += 1; // command name
        let mut args = Vec::new();
        loop {
            match self.peek() {
                Tok::Ident(s) => { args.push(Expr::Str(s.clone())); self.pos += 1; }
                Tok::Str(s) => { args.push(Expr::Str(s.clone())); self.pos += 1; }
                _ => break,
            }
        }
        Ok(Stmt::Expr(Expr::Call { name, args }))
    }

    fn parse_matrix(&mut self) -> Result<Expr, String> {
        self.pos += 1; // [
        let mut rows: Vec<Vec<Expr>> = Vec::new();
        if matches!(self.peek(), Tok::RBracket) { self.pos += 1; return Ok(Expr::Matrix(rows)); }
        loop {
            let mut row = Vec::new();
            loop {
                self.skip_nl();
                if !is_expr_start(self.peek()) { break; }
                let e = self.parse_expr(0)?;
                row.push(e);
                match self.peek() {
                    Tok::Comma => { self.pos += 1; }
                    Tok::Semicolon | Tok::RBracket | Tok::Newline => { break; }
                    _ if is_expr_start(self.peek()) => { /* space separator, continue */ }
                    _ => { break; }
                }
            }
            rows.push(row);
            match self.peek() {
                Tok::Semicolon => { self.pos += 1; }
                Tok::RBracket => { self.pos += 1; break; }
                Tok::Newline => { self.pos += 1; }
                other => return Err(format!("expected ';' or ']' in matrix literal, found {:?}", other)),
            }
        }
        Ok(Expr::Matrix(rows))
    }

    fn parse_cell(&mut self) -> Result<Expr, String> {
        self.pos += 1; // {
        let mut rows: Vec<Vec<Expr>> = Vec::new();
        if matches!(self.peek(), Tok::RBrace) { self.pos += 1; return Ok(Expr::Cell(rows)); }
        loop {
            let mut row = Vec::new();
            loop {
                self.skip_nl();
                if !is_expr_start(self.peek()) { break; }
                let e = self.parse_expr(0)?;
                row.push(e);
                match self.peek() {
                    Tok::Comma => { self.pos += 1; }
                    Tok::Semicolon | Tok::RBrace | Tok::Newline => { break; }
                    _ if is_expr_start(self.peek()) => { }
                    _ => { break; }
                }
            }
            rows.push(row);
            match self.peek() {
                Tok::Semicolon => { self.pos += 1; }
                Tok::RBrace => { self.pos += 1; break; }
                Tok::Newline => { self.pos += 1; }
                other => return Err(format!("expected ';' or '}}' in cell literal, found {:?}", other)),
            }
        }
        Ok(Expr::Cell(rows))
    }
}

/// Builtins conventionally invoked with command syntax (bareword string args).
fn is_command_builtin(name: &str) -> bool {
    matches!(name,
        "clear" | "format" | "hold" | "grid" | "axis" | "who" | "clc")
}

fn is_expr_start(t: &Tok) -> bool {
    matches!(t,
        Tok::Num(_) | Tok::Imag(_) | Tok::Str(_) | Tok::DStr(_) | Tok::Ident(_) | Tok::LParen | Tok::LBracket | Tok::LBrace
        | Tok::Minus | Tok::Plus | Tok::Not | Tok::Colon | Tok::At)
        || matches!(t, Tok::Kw(k) if k == "end")
}

/// Precedence floor for a leading `+`/`-` sign. Sits above `*` (110) and below
/// `^` (120) so the sign absorbs exponentiation into its operand — making
/// `-2^2` evaluate as `-(2^2) == -4` rather than `(-2)^2 == 4`.
const UNARY_SIGN_BP: u8 = 111;

fn binop(t: &Tok) -> Option<(BinOp, u8, bool)> {
    let r = match t {
        Tok::Plus => (BinOp::Add, 100, false),
        Tok::Minus => (BinOp::Sub, 100, false),
        Tok::Star => (BinOp::Mul, 110, false),
        Tok::Slash => (BinOp::Div, 110, false),
        Tok::Backslash => (BinOp::LDiv, 110, false),
        Tok::Hat => (BinOp::Pow, 120, true),
        Tok::StarStar => (BinOp::Pow, 120, true),
        Tok::DotStar => (BinOp::ElemMul, 110, false),
        Tok::DotSlash => (BinOp::ElemDiv, 110, false),
        Tok::DotBackslash => (BinOp::ElemLDiv, 110, false),
        Tok::DotHat => (BinOp::ElemPow, 120, true),
        Tok::Lt => (BinOp::Lt, 80, false),
        Tok::Gt => (BinOp::Gt, 80, false),
        Tok::Le => (BinOp::Le, 80, false),
        Tok::Ge => (BinOp::Ge, 80, false),
        Tok::Eq => (BinOp::Eq, 80, false),
        Tok::Ne => (BinOp::Ne, 80, false),
        Tok::And => (BinOp::And, 70, false),
        Tok::Or => (BinOp::Or, 60, false),
        Tok::AndAnd => (BinOp::AndAnd, 50, false),
        Tok::OrOr => (BinOp::OrOr, 50, false),
        _ => return None,
    };
    Some(r)
}

fn to_lvalue(expr: Expr) -> Result<LValue, String> {
    match expr {
        Expr::Ident(s) => Ok(LValue::Ident(s)),
        Expr::Index { base, indices } => {
            if let Expr::Ident(name) = *base {
                Ok(LValue::Index { name, indices })
            } else {
                Err("invalid assignment target".into())
            }
        }
        Expr::Call { name, args } => {
            // A(4) = 99 → treat as indexed assignment when used as lvalue
            Ok(LValue::Index { name, indices: args })
        }
        Expr::BraceIndex { base, indices } => {
            if let Expr::Ident(name) = *base {
                Ok(LValue::BraceIndex { name, indices })
            } else {
                Err("invalid assignment target".into())
            }
        }
        Expr::DotAccess { base, field } => {
            if let Expr::Ident(name) = *base {
                Ok(LValue::DotAccess { name, field })
            } else {
                Err("invalid assignment target".into())
            }
        }
        Expr::DynamicDotAccess { base, field_expr } => {
            if let Expr::Ident(name) = *base {
                Ok(LValue::DynamicDotAccess { name, field_expr })
            } else {
                Err("invalid assignment target".into())
            }
        }
        Expr::Matrix(rows) => {
            if rows.len() != 1 { return Err("invalid assignment target".into()); }
            let list = rows.into_iter().next().unwrap()
                .into_iter().map(|e| -> Result<LValue, String> {
                    match e {
                        Expr::Ident(s) => Ok(LValue::Ident(s)),
                        _ => Err("multi-output targets must be identifiers".into()),
                    }
                }).collect::<Result<Vec<_>, _>>()?;
            Ok(LValue::List(list))
        }
        _ => Err("invalid assignment target".into()),
    }
}
