//! Abstract syntax tree for the MATBOX language.

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    Assign { target: LValue, value: Expr },
    If { cond: Expr, body: Vec<Stmt>, elseif: Vec<(Expr, Vec<Stmt>)>, else_body: Vec<Stmt> },
    For { var: String, range: Expr, body: Vec<Stmt> },
    While { cond: Expr, body: Vec<Stmt> },
    Switch { expr: Expr, cases: Vec<(Expr, Vec<Stmt>)>, otherwise: Vec<Stmt> },
    Try { body: Vec<Stmt>, catch_var: Option<String>, catch_body: Vec<Stmt> },
    Return,
    Break,
    Continue,
    Function {
        outputs: Vec<String>,
        name: String,
        inputs: Vec<String>,
        body: Vec<Stmt>,
    },
    Parfor { var: String, range: Expr, body: Vec<Stmt> },
    Persistent(Vec<String>),
    Global(Vec<String>),
}

#[derive(Debug, Clone)]
pub enum LValue {
    Ident(String),
    Index { name: String, indices: Vec<Expr> },
    BraceIndex { name: String, indices: Vec<Expr> },
    DotAccess { name: String, field: String },
    DynamicDotAccess { name: String, field_expr: Box<Expr> },
    List(Vec<LValue>),
}

impl LValue {
    pub fn ident(&self) -> Option<&String> {
        match self {
            LValue::Ident(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    Num(f64),
    Imag(f64),
    Str(String),
    Ident(String),
    Binary { op: BinOp, left: Box<Expr>, right: Box<Expr> },
    Unary { op: UnOp, expr: Box<Expr> },
    Transpose { expr: Box<Expr>, conj: bool },
    Matrix(Vec<Vec<Expr>>),
    Cell(Vec<Vec<Expr>>),
    ColonRange { start: Box<Expr>, step: Option<Box<Expr>>, end: Box<Expr> },
    Call { name: String, args: Vec<Expr> },
    Index { base: Box<Expr>, indices: Vec<Expr> },
    BraceIndex { base: Box<Expr>, indices: Vec<Expr> },
    DotAccess { base: Box<Expr>, field: String },
    DynamicDotAccess { base: Box<Expr>, field_expr: Box<Expr> },
    Colon,
    End,
    AnonFunc { params: Vec<String>, body: Box<Expr> },
    FuncRef(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add, Sub, Mul, Div, LDiv, Pow,
    ElemMul, ElemDiv, ElemLDiv, ElemPow,
    Lt, Gt, Le, Ge, Eq, Ne,
    And, Or, AndAnd, OrOr,
    Colon,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg, Pos, Not,
}
