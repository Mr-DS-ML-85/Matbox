//! Lexer for the MATBOX (MATLAB-flavored) language.

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Num(f64),
    Imag(f64), // pure-imaginary literal, e.g. 2i or 1.5j
    Ident(String),
    Str(String),
    Plus,
    Minus,
    Star,
    StarStar,
    Slash,
    Backslash,
    Hat,
    DotStar,
    DotSlash,
    DotBackslash,
    DotHat,
    Transpose, // .'
    CTranspose, // '
    Colon,
    Eq,
    Ne,
    Le,
    Ge,
    Lt,
    Gt,
    And,
    Or,
    AndAnd,
    OrOr,
    Not,
    Newline,
    Assign,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Dot,
    Semicolon,
    Comma,
    At,
    Kw(String),
    Eof,
}

pub struct Lexer {
    chars: Vec<char>,
    pos: usize,
    pub tokens: Vec<Tok>,
}

impl Lexer {
    pub fn new(src: &str) -> Self {
        Lexer { chars: src.chars().collect(), pos: 0, tokens: Vec::new() }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    fn peek2(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }
    pub fn lex(&mut self) -> Result<(), String> {
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\t' | '\r' => { self.pos += 1; }
                '\n' => { self.pos += 1; self.tokens.push(Tok::Newline); }
                '%' => {
                    if self.peek2() == Some('{') {
                        // multi-line comment %{ ... %}
                        self.pos += 2;
                        while let Some(ch) = self.peek() {
                            if ch == '%' && self.peek2() == Some('}') { self.pos += 2; break; }
                            self.pos += 1;
                        }
                    } else {
                        while let Some(ch) = self.peek() {
                            if ch == '\n' { break; }
                            self.pos += 1;
                        }
                    }
                }
                '#' => {
                    while let Some(ch) = self.peek() {
                        if ch == '\n' { break; }
                        self.pos += 1;
                    }
                }
                '.' => {
                    if self.peek2() == Some('.') && self.chars.get(self.pos + 2).copied() == Some('.') {
                        while let Some(ch) = self.peek() {
                            if ch == '\n' { break; }
                            self.pos += 1;
                        }
                        continue;
                    }
                    if matches!(self.peek2(), Some('*')) { self.pos += 2; self.tokens.push(Tok::DotStar); }
                    else if matches!(self.peek2(), Some('/')) { self.pos += 2; self.tokens.push(Tok::DotSlash); }
                    else if matches!(self.peek2(), Some('\\')) { self.pos += 2; self.tokens.push(Tok::DotBackslash); }
                    else if matches!(self.peek2(), Some('^')) { self.pos += 2; self.tokens.push(Tok::DotHat); }
                    else if self.peek2().map(|c| c.is_ascii_alphabetic() || c == '_').unwrap_or(false) {
                        self.pos += 1; self.tokens.push(Tok::Dot);
                    }
                    else if matches!(self.peek2(), Some('(')) {
                        self.pos += 1; self.tokens.push(Tok::Dot); // dynamic field access: .(
                    }
                    else if let Some(n) = self.try_number() { self.tokens.push(n); }
                    else { self.pos += 1; self.tokens.push(Tok::Transpose); }
                }
                '+' => { self.pos += 1; self.tokens.push(Tok::Plus); }
                '-' => { self.pos += 1; self.tokens.push(Tok::Minus); }
                '*' => {
                    if self.peek2() == Some('*') { self.pos += 2; self.tokens.push(Tok::StarStar); }
                    else { self.pos += 1; self.tokens.push(Tok::Star); }
                }
                '/' => { self.pos += 1; self.tokens.push(Tok::Slash); }
                '\\' => { self.pos += 1; self.tokens.push(Tok::Backslash); }
                '^' => { self.pos += 1; self.tokens.push(Tok::Hat); }
                '\'' => {
                    // A `'` is a (conjugate) transpose when it directly follows an operand,
                    // otherwise it opens a char literal. `''` after an operand is a double
                    // transpose; standalone it is the empty string. Deciding on the previous
                    // character (not `peek2`) keeps both cases unambiguous.
                    let operand_pos = self.pos > 0 && {
                        let prev = self.chars[self.pos - 1];
                        prev.is_ascii_alphanumeric() || prev == '_' || prev == ')'
                            || prev == ']' || prev == '}' || prev == '.' || prev == '\''
                    };
                    if operand_pos {
                        self.pos += 1; self.tokens.push(Tok::CTranspose);
                    } else if let Some(s) = self.try_char_literal() {
                        self.tokens.push(Tok::Str(s));
                    } else {
                        self.pos += 1; self.tokens.push(Tok::CTranspose);
                    }
                }
                ':' => { self.pos += 1; self.tokens.push(Tok::Colon); }
                '=' => {
                    if self.peek2() == Some('=') { self.pos += 2; self.tokens.push(Tok::Eq); }
                    else { self.pos += 1; self.tokens.push(Tok::Assign); }
                }
                '~' => {
                    if self.peek2() == Some('=') { self.pos += 2; self.tokens.push(Tok::Ne); }
                    else { self.pos += 1; self.tokens.push(Tok::Not); }
                }
                '<' => {
                    if self.peek2() == Some('=') { self.pos += 2; self.tokens.push(Tok::Le); }
                    else { self.pos += 1; self.tokens.push(Tok::Lt); }
                }
                '>' => {
                    if self.peek2() == Some('=') { self.pos += 2; self.tokens.push(Tok::Ge); }
                    else { self.pos += 1; self.tokens.push(Tok::Gt); }
                }
                '&' => {
                    if self.peek2() == Some('&') { self.pos += 2; self.tokens.push(Tok::AndAnd); }
                    else { self.pos += 1; self.tokens.push(Tok::And); }
                }
                '|' => {
                    if self.peek2() == Some('|') { self.pos += 2; self.tokens.push(Tok::OrOr); }
                    else { self.pos += 1; self.tokens.push(Tok::Or); }
                }
                '(' => { self.pos += 1; self.tokens.push(Tok::LParen); }
                ')' => { self.pos += 1; self.tokens.push(Tok::RParen); }
                '[' => { self.pos += 1; self.tokens.push(Tok::LBracket); }
                ']' => { self.pos += 1; self.tokens.push(Tok::RBracket); }
                '{' => { self.pos += 1; self.tokens.push(Tok::LBrace); }
                '}' => { self.pos += 1; self.tokens.push(Tok::RBrace); }
                ';' => { self.pos += 1; self.tokens.push(Tok::Semicolon); }
                ',' => { self.pos += 1; self.tokens.push(Tok::Comma); }
                '"' => {
                    self.pos += 1;
                    let mut s = String::new();
                    while let Some(ch) = self.peek() {
                        if ch == '"' { self.pos += 1; break; }
                        if ch == '\\' { self.pos += 1; if let Some(e) = self.peek() { s.push(e); self.pos += 1; } }
                        else { s.push(ch); self.pos += 1; }
                    }
                    self.tokens.push(Tok::Str(s));
                }
                '@' => { self.pos += 1; self.tokens.push(Tok::At); }
                _ => {
                    if c.is_ascii_digit() || (c == '.' && self.peek2().map(|x| x.is_ascii_digit()).unwrap_or(false)) {
                        if let Some(n) = self.try_number() { self.tokens.push(n); }
                        else { self.pos += 1; }
                    } else if c.is_ascii_alphabetic() || c == '_' {
                        let mut id = String::new();
                        while let Some(ch) = self.peek() {
                            if ch.is_ascii_alphanumeric() || ch == '_' { id.push(ch); self.pos += 1; }
                            else { break; }
                        }
                        self.tokens.push(keyword_or_ident(id));
                    } else {
                        return Err(format!("unexpected character '{}' at position {}", c, self.pos));
                    }
                }
            }
        }
        self.tokens.push(Tok::Eof);
        Ok(())
    }

    fn try_char_literal(&mut self) -> Option<String> {
        // current char is the opening '
        self.pos += 1;
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if ch == '\'' {
                if self.peek2() == Some('\'') { s.push('\''); self.pos += 2; continue; }
                self.pos += 1;
                return Some(s);
            }
            s.push(ch);
            self.pos += 1;
        }
        None
    }

    /// Try to parse a number (int/float/scientific) optionally followed by i/j imaginary suffix.
    fn try_number(&mut self) -> Option<Tok> {
        let start = self.pos;
        let mut has_dot = false;
        let mut has_exp = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() { self.pos += 1; }
            else if c == '.' && !has_dot && !has_exp { has_dot = true; self.pos += 1; }
            else if (c == 'e' || c == 'E') && !has_exp {
                has_exp = true; self.pos += 1;
                if matches!(self.peek(), Some('+') | Some('-')) { self.pos += 1; }
            }
            else { break; }
        }
        if self.pos == start { return None; }
        let s: String = self.chars[start..self.pos].iter().collect();
        let val: f64 = s.parse().ok()?;
        if let Some(nx) = self.peek() {
            if nx == 'i' || nx == 'j' { self.pos += 1; return Some(Tok::Imag(val)); }
        }
        Some(Tok::Num(val))
    }
}

fn keyword_or_ident(s: String) -> Tok {
    match s.as_str() {
        "if" | "else" | "elseif" | "for" | "while" | "end" | "function" | "return"
        | "break" | "continue" | "try" | "catch" | "switch" | "case" | "otherwise"
        | "parfor" | "persistent" | "global" | "endfunction" => Tok::Kw(s),
        _ => Tok::Ident(s),
    }
}
