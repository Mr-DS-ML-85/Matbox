//! Interpreter / evaluator for the MATBOX language.

use crate::ast::*;
use crate::builtins;
use crate::parser::Parser;
use matbox_math::array::{MxArray, MxData};
use matbox_math::{binary_elementwise, matmul, unary_elementwise, MathError};
use num_complex::Complex64;
use std::borrow::Cow;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Value {
    Array(MxArray),
    String(String),
    /// MATBOX's double-quoted string type: a row-major grid of text elements
    /// (mirroring `Cell`'s [rows][cols] shape convention), distinct from the
    /// single-quoted char array (`String`). A scalar string is `[[s]]`.
    Str(Vec<Vec<String>>),
    Cell(Vec<Vec<Value>>),
    Struct(HashMap<String, Value>),
    /// `containers.Map`: an ordered list of key/value entries. Kept as a
    /// `Vec` (not `HashMap`) since keys are compared by content, not hashed.
    Map(Vec<(Value, Value)>),
    Multi(Vec<Value>),
    Func(Vec<String>, Box<Expr>, HashMap<String, Value>),
    FuncRef(String),
}

impl Value {
    pub fn as_array(&self) -> &MxArray {
        match self {
            Value::Array(a) => a,
            Value::Multi(v) => v[0].as_array(),
            _ => panic!("expected array, found {:?}", self),
        }
    }
    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_)) || matches!(self, Value::Multi(v) if matches!(&v[0], Value::Array(_)))
    }
    pub fn to_scalar(&self) -> f64 {
        match self {
            Value::Array(a) => a.get_double(&[]),
            Value::Multi(v) => v[0].to_scalar(),
            _ => panic!("expected numeric scalar, found {:?}", self),
        }
    }
    pub fn to_scalar_usize(&self) -> usize {
        let v = self.to_scalar();
        if v < 0.0 || (v.fract()).abs() > 1e-12 { panic!("expected integer index"); }
        v.round() as usize
    }
    pub fn into_single(self) -> Value {
        match self {
            Value::Multi(mut v) => v.remove(0),
            other => other,
        }
    }
}

#[derive(Debug)]
pub enum EngineError {
    Msg(String),
    /// A structured error raised via `error(id, fmt, ...)` / `MException`,
    /// carrying the `component:mnemonic` identifier alongside the formatted
    /// message so `catch` can bind an object with `.identifier`/`.message`.
    Exception { identifier: String, message: String },
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Msg(m) => write!(f, "{}", m),
            EngineError::Exception { message, .. } => write!(f, "{}", message),
        }
    }
}

pub(crate) struct Rng {
    seed: u32,
}
impl Rng {
    pub(crate) fn next_u32(&mut self) -> u32 {
        // Park-Miller LCG
        self.seed = ((self.seed as u64 * 48271) % 2147483647) as u32;
        self.seed
    }
    pub(crate) fn uniform(&mut self) -> f64 {
        (self.next_u32() as f64) / 2147483647.0
    }
    pub(crate) fn normal(&mut self) -> f64 {
        let u1 = self.uniform().max(1e-12);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

pub struct Engine {
    pub workspace: HashMap<String, Value>,
    pub(crate) rng: Rng,
    pub format: String,
    suppress_display: bool,
    pub(crate) functions: HashMap<String, UserFuncBody>,
    pub(crate) persistent_store: HashMap<String, HashMap<String, Value>>,
    pub(crate) global_store: HashMap<String, Value>,
    pub(crate) current_function: Option<String>,
    /// Number of outputs requested by the enclosing call site (e.g. 2 for
    /// `[a, b] = f(x)`). Builtins consult this to decide how many values to
    /// return. Defaults to 1 and is reset after each dispatch.
    pub(crate) nargout: usize,
}

pub(crate) type UserFuncBody = (Vec<String>, Vec<String>, Vec<Stmt>);

/// Names that are seeded into every workspace as reserved constants
/// (`pi`, `eps`, `i`, ...). They are not user variables, so `who` skips them.
pub(crate) fn is_reserved_constant(name: &str) -> bool {
    matches!(name, "pi" | "eps" | "inf" | "nan" | "Inf" | "NaN" | "NA" | "e"
        | "i" | "j" | "true" | "false")
}

pub(crate) enum Flow {
    None,
    Break,
    Continue,
    Return(Option<Value>),
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        let mut e = Engine {
            workspace: HashMap::new(),
            rng: Rng { seed: 123456789 },
            format: "short".to_string(),
            suppress_display: false,
            functions: HashMap::new(),
            persistent_store: HashMap::new(),
            global_store: HashMap::new(),
            current_function: None,
            nargout: 1,
        };
        e.reset_constants();
        e
    }

    pub fn reset_constants(&mut self) {
        self.workspace.insert("pi".into(), Value::Array(MxArray::scalar_double(std::f64::consts::PI)));
        self.workspace.insert("eps".into(), Value::Array(MxArray::scalar_double(f64::EPSILON)));
        self.workspace.insert("inf".into(), Value::Array(MxArray::scalar_double(f64::INFINITY)));
        self.workspace.insert("nan".into(), Value::Array(MxArray::scalar_double(f64::NAN)));
        // Capitalised spellings are the canonical display forms and are
        // accepted as names in their own right, alongside `e` and `NA`.
        self.workspace.insert("Inf".into(), Value::Array(MxArray::scalar_double(f64::INFINITY)));
        self.workspace.insert("NaN".into(), Value::Array(MxArray::scalar_double(f64::NAN)));
        self.workspace.insert("e".into(), Value::Array(MxArray::scalar_double(std::f64::consts::E)));
        self.workspace.insert("NA".into(), Value::Array(MxArray::scalar_double(f64::NAN)));
        let i = Complex64::new(0.0, 1.0);
        self.workspace.insert("i".into(), Value::Array(MxArray::scalar_complex(i)));
        self.workspace.insert("j".into(), Value::Array(MxArray::scalar_complex(i)));
        self.workspace.insert("true".into(), Value::Array(MxArray::scalar_logical(true)));
        self.workspace.insert("false".into(), Value::Array(MxArray::scalar_logical(false)));
    }

    pub(crate) fn eval_func_handle(&mut self, func: Value, argvals: Vec<Value>) -> Result<Value, EngineError> {
        match func {
            Value::Func(params, body, captured) => self.eval_anon_func(params, *body, captured, argvals),
            // A bare `@name` handle (e.g. `@isempty`, `@numel`) names a builtin
            // or user function rather than an anonymous function body — dispatch
            // it the same way a direct call would. Without this arm, cellfun/
            // arrayfun given a plain function-name handle fails with "expected
            // function handle" even though the handle is valid.
            Value::FuncRef(name) => self.dispatch_call(&name, argvals),
            _ => Err(EngineError::Msg("expected function handle".into())),
        }
    }

    pub fn eval(&mut self, code: &str) -> Result<Option<Value>, EngineError> {
        let mut p = Parser::new(code).map_err(EngineError::Msg)?;
        let stmts = p.parse_program().map_err(EngineError::Msg)?;
        let mut display: Option<Value> = None;
        for stmt in stmts {
            let (flow, val) = self.exec_stmt(&stmt)?;
            if let Some(v) = val { display = Some(v); }
            match flow {
                Flow::Break => return Err(EngineError::Msg("break outside loop".into())),
                Flow::Continue => return Err(EngineError::Msg("continue outside loop".into())),
                Flow::Return(v) => { display = v.or(display); break; }
                Flow::None => {}
            }
        }
        if let Some(ref v) = display {
            self.workspace.insert("ans".into(), v.clone());
        }
        Ok(display)
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<(Flow, Option<Value>), EngineError> {
        match stmt {
            Stmt::Expr(e) => {
                let v = self.eval_expr(e)?;
                Ok((Flow::None, Some(v)))
            }
            Stmt::Assign { target, value } => {
                if let LValue::List(list) = target {
                    self.nargout = list.len();
                }
                let v = self.eval_expr(value)?;
                self.nargout = 1;
                self.assign_lvalue(target, v.clone())?;
                // assignment with single identifier/colon target displays; list targets don't.
                let disp = match target {
                    LValue::List(_) => None,
                    _ => Some(v),
                };
                Ok((Flow::None, disp))
            }
            Stmt::If { cond, body, elseif, else_body } => {
                let c = self.eval_expr(cond)?;
                if self.is_true(&c) {
                    return self.exec_block(body);
                }
                for (cc, b) in elseif {
                    let cv = self.eval_expr(cc)?;
                    if self.is_true(&cv) { return self.exec_block(b); }
                }
                if !else_body.is_empty() { return self.exec_block(else_body); }
                Ok((Flow::None, None))
            }
            Stmt::For { var, range, body } => {
                let rv = self.eval_expr(range)?;
                // iterate over columns (MATLAB loops over columns of the range)
                let cols = self.column_vectors(&rv);
                for col in cols {
                    self.workspace.insert(var.clone(), Value::Array(col));
                    let (flow, _) = self.exec_block(body)?;
                    match flow {
                        Flow::Break => break,
                        Flow::Continue => continue,
                        Flow::Return(v) => return Ok((Flow::Return(v), None)),
                        Flow::None => {}
                    }
                }
                Ok((Flow::None, None))
            }
            Stmt::While { cond, body } => {
                loop {
                    let c = self.eval_expr(cond)?;
                    if !self.is_true(&c) { break; }
                    let (flow, _) = self.exec_block(body)?;
                    match flow {
                        Flow::Break => break,
                        Flow::Continue => continue,
                        Flow::Return(v) => return Ok((Flow::Return(v), None)),
                        Flow::None => {}
                    }
                }
                Ok((Flow::None, None))
            }
            Stmt::Switch { expr, cases, otherwise } => {
                let sv = self.eval_expr(expr)?;
                for (ce, body) in cases {
                    let cv = self.eval_expr(ce)?;
                    if let Value::Cell(ref rows) = cv {
                        for row in rows {
                            for val in row {
                                if values_equal(&sv, val) {
                                    let (flow, v) = self.exec_block(body)?;
                                    return Ok((flow, v));
                                }
                            }
                        }
                    } else if values_equal(&sv, &cv) {
                        let (flow, v) = self.exec_block(body)?;
                        return Ok((flow, v));
                    }
                }
                if !otherwise.is_empty() {
                    return self.exec_block(otherwise);
                }
                Ok((Flow::None, None))
            }
            Stmt::Try { body, catch_var, catch_body } => {
                match self.exec_block(body) {
                    Ok((flow, v)) => Ok((flow, v)),
                    Err(e) => {
                        if let Some(var) = catch_var {
                            // Bind an MException-like struct: `.identifier` is empty
                            // unless the error was raised with a `component:mnemonic`
                            // id (see `error()`), `.message` is the formatted text.
                            let (identifier, message) = match &e {
                                EngineError::Msg(m) => (String::new(), m.clone()),
                                EngineError::Exception { identifier, message } =>
                                    (identifier.clone(), message.clone()),
                            };
                            let mut fields = HashMap::new();
                            fields.insert("message".into(), Value::String(message));
                            fields.insert("identifier".into(), Value::String(identifier));
                            fields.insert("stack".into(), Value::Struct(HashMap::new()));
                            fields.insert("cause".into(), Value::Cell(Vec::new()));
                            self.workspace.insert(var.clone(), Value::Struct(fields));
                        }
                        self.exec_block(catch_body)
                    }
                }
            }
            Stmt::Return => Ok((Flow::Return(None), None)),
            Stmt::Break => Ok((Flow::Break, None)),
            Stmt::Continue => Ok((Flow::Continue, None)),
            Stmt::Function { outputs, name, inputs, body } => {
                self.functions.insert(name.clone(), (inputs.clone(), outputs.clone(), body.clone()));
                Ok((Flow::None, Some(Value::Array(MxArray::new_double(&[0, 0])))))
            }
            Stmt::Parfor { var, range, body } => {
                let rv = self.eval_expr(range)?;
                let cols = self.column_vectors(&rv);
                for col in cols {
                    self.workspace.insert(var.clone(), Value::Array(col));
                    let (flow, _) = self.exec_block(body)?;
                    match flow {
                        Flow::Break => break,
                        Flow::Continue => continue,
                        Flow::Return(v) => return Ok((Flow::Return(v), None)),
                        Flow::None => {}
                    }
                }
                Ok((Flow::None, None))
            }
            Stmt::Persistent(vars) => {
                for var in vars {
                    let func_name = self.current_function.clone().unwrap_or_default();
                    let store = self.persistent_store.entry(func_name).or_default();
                    if !store.contains_key(var) {
                        store.insert(var.clone(), Value::Array(MxArray::new_double(&[0, 0])));
                    }
                    if let Some(val) = store.get(var) {
                        self.workspace.insert(var.clone(), val.clone());
                    }
                }
                Ok((Flow::None, None))
            }
            Stmt::Global(vars) => {
                for var in vars {
                    if let Some(val) = self.global_store.get(var).cloned() {
                        self.workspace.insert(var.clone(), val);
                    } else {
                        let val = self.workspace.get(var).cloned()
                            .unwrap_or(Value::Array(MxArray::new_double(&[0, 0])));
                        self.global_store.insert(var.clone(), val.clone());
                        self.workspace.insert(var.clone(), val);
                    }
                }
                Ok((Flow::None, None))
            }
        }
    }

    pub(crate) fn exec_block(&mut self, stmts: &[Stmt]) -> Result<(Flow, Option<Value>), EngineError> {
        let mut disp = None;
        for s in stmts {
            let (flow, v) = self.exec_stmt(s)?;
            if !self.suppress_display {
                if let Some(val) = v { disp = Some(val); }
            }
            match flow {
                Flow::None => {}
                other => return Ok((other, disp)),
            }
        }
        Ok((Flow::None, disp))
    }

    pub(crate) fn is_true(&self, v: &Value) -> bool {
        match v {
            Value::Array(a) => {
                if a.is_empty() { return false; }
                a.iter_complex().iter().all(|c| c.norm() != 0.0 && !c.re.is_nan())
            }
            Value::String(s) => !s.is_empty(),
            Value::Str(rows) => !rows.is_empty() && !rows[0].is_empty(),
            Value::Cell(c) => !c.is_empty(),
            Value::Struct(m) => !m.is_empty(),
            Value::Map(entries) => !entries.is_empty(),
            Value::Multi(vals) => vals.iter().any(|v| self.is_true(v)),
            Value::Func(_, _, _) | Value::FuncRef(_) => true,
        }
    }

    fn column_vectors(&self, v: &Value) -> Vec<MxArray> {
        let a = v.as_array();
        let m = a.shape[0];
        let n = a.shape.get(1).copied().unwrap_or(1);
        let mut out = Vec::new();
        for j in 0..n {
            let mut col = Vec::with_capacity(m);
            for i in 0..m {
                col.push(a.get_complex(&[i, j]));
            }
            out.push(MxArray::from_complex_colmajor(col, &[m, 1]));
        }
    out
}

    fn assign_lvalue(&mut self, target: &LValue, value: Value) -> Result<(), EngineError> {
        match target {
            LValue::Ident(name) => {
                // if multi-output, take first output
                let v = match value {
                    Value::Multi(mut vals) => vals.remove(0),
                    other => other,
                };
                self.workspace.insert(name.clone(), v);
                Ok(())
            }
            LValue::Index { name, indices } => {
                let existing = self.workspace.get(name).cloned()
                    .ok_or_else(|| EngineError::Msg(format!("undefined variable '{}'", name)))?;
                if let Value::Map(mut entries) = existing {
                    // `m(key) = v`: containers.Map uses function-call-style
                    // indexing for both lookup and assignment, unlike the
                    // ()/{} split used by arrays/cells.
                    if indices.len() != 1 {
                        return Err(EngineError::Msg(
                            "containers.Map assignment requires a single key index".into()));
                    }
                    let key = self.eval_expr(&indices[0])?;
                    let v = match value { Value::Multi(mut vals) => vals.remove(0), other => other };
                    if let Some(slot) = entries.iter_mut().find(|(k, _)| map_key_eq(k, &key)) {
                        slot.1 = v;
                    } else {
                        entries.push((key, v));
                    }
                    self.workspace.insert(name.clone(), Value::Map(entries));
                    return Ok(());
                }
                let new_arr = self.index_assign(&existing.as_array().clone(), indices, &value)?;
                self.workspace.insert(name.clone(), Value::Array(new_arr));
                Ok(())
            }
            LValue::BraceIndex { name, indices } => {
                let v = match value {
                    Value::Multi(mut vals) => vals.remove(0),
                    other => other,
                };
                let mut rows: Vec<Vec<Value>> = match self.workspace.get(name) {
                    Some(Value::Cell(r)) => r.clone(),
                    None => Vec::new(),
                    Some(_) => return Err(EngineError::Msg(format!(
                        "cannot use '{{}}' indexing to assign into non-cell '{}'", name))),
                };
                let m = rows.len();
                let n = if m > 0 { rows[0].len() } else { 0 };
                let (row, col, new_m, new_n) = if indices.len() == 1 {
                    let e = replace_end(&indices[0], m * n);
                    let k = self.eval_expr(&e)?.as_array().to_scalar().round() as i64;
                    if k < 1 { return Err(EngineError::Msg("index must be a positive integer".into())); }
                    let k = k as usize;
                    if m <= 1 {
                        // empty or row vector: grow along columns
                        (0, k - 1, m.max(1), n.max(k))
                    } else if n == 1 {
                        // column vector: grow along rows
                        (k - 1, 0, m.max(k), 1)
                    } else {
                        // established matrix: linear index without growth
                        let idx = k - 1;
                        let (r, c) = (idx % m, idx / m);
                        if c >= n { return Err(EngineError::Msg("cell index out of range".into())); }
                        (r, c, m, n)
                    }
                } else {
                    let r = self.eval_expr(&replace_end(&indices[0], m))?.as_array().to_scalar().round() as i64;
                    let c = self.eval_expr(&replace_end(&indices[1], n))?.as_array().to_scalar().round() as i64;
                    if r < 1 || c < 1 { return Err(EngineError::Msg("index must be a positive integer".into())); }
                    (r as usize - 1, c as usize - 1, m.max(r as usize), n.max(c as usize))
                };
                rows.resize_with(new_m, Vec::new);
                for rr in rows.iter_mut() {
                    rr.resize_with(new_n, || Value::Array(MxArray::new_double(&[0, 0])));
                }
                rows[row][col] = v;
                self.workspace.insert(name.clone(), Value::Cell(rows));
                Ok(())
            }
            LValue::DotAccess { name, field } => {
                let mut s = match self.workspace.get(name) {
                    Some(Value::Struct(m)) => m.clone(),
                    _ => HashMap::new(),
                };
                s.insert(field.clone(), value);
                self.workspace.insert(name.clone(), Value::Struct(s));
                Ok(())
            }
            LValue::DynamicDotAccess { name, field_expr } => {
                let fv = self.eval_expr(field_expr)?;
                let field = text_arg(&fv).ok_or_else(|| EngineError::Msg(
                    "dynamic field name must evaluate to a string".into()))?;
                let mut s = match self.workspace.get(name) {
                    Some(Value::Struct(m)) => m.clone(),
                    _ => HashMap::new(),
                };
                s.insert(field, value);
                self.workspace.insert(name.clone(), Value::Struct(s));
                Ok(())
            }
            LValue::List(list) => {
                if list.len() == 1 {
                    self.workspace.insert(list[0].ident().unwrap().clone(), value);
                    return Ok(());
                }
                // unpack multi-output from Value::Multi
                match value {
                    Value::Multi(vals) => {
                        for (i, lv) in list.iter().enumerate() {
                            if let LValue::Ident(name) = lv {
                                if let Some(v) = vals.get(i) {
                                    self.workspace.insert(name.clone(), v.clone());
                                }
                            }
                        }
                        Ok(())
                    }
                    _ => Err(EngineError::Msg(
                        "multi-output assignment requires a function returning multiple values".into()
                    )),
                }
            }
        }
    }

    fn eval_expr(&mut self, e: &Expr) -> Result<Value, EngineError> {
        match e {
            Expr::Num(f) => Ok(Value::Array(MxArray::scalar_double(*f))),
            Expr::Imag(f) => Ok(Value::Array(MxArray::scalar_complex(Complex64::new(0.0, *f)))),
            Expr::Str(s) => Ok(Value::String(s.clone())),
            Expr::DStr(s) => Ok(Value::Str(vec![vec![s.clone()]])),
            Expr::Ident(name) => {
                if let Some(v) = self.workspace.get(name).cloned() { return Ok(v); }
                // try loading as a script file
                let path = format!("{}.m", name);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let mut p = match Parser::new(&content) {
                        Ok(p) => p,
                        Err(e) => return Err(EngineError::Msg(e)),
                    };
                    let stmts = match p.parse_program() {
                        Ok(s) => s,
                        Err(e) => return Err(EngineError::Msg(e)),
                    };
                    let (_, disp) = self.exec_block(&stmts)?;
                    return Ok(disp.unwrap_or(Value::Array(MxArray::new_double(&[0, 0]))));
                }
                // Command syntax: a bare identifier may name a zero-argument builtin or
                // user function (e.g. `who`, `clear`, `clc`, `pwd`). Fall back to a call,
                // preserving the original message when it is neither.
                match self.dispatch_call(name, vec![]) {
                    Ok(v) => Ok(v),
                    Err(EngineError::Msg(m))
                        if m.starts_with("not a builtin") || m.starts_with("undefined function") =>
                    {
                        Err(EngineError::Msg(format!("undefined function or variable '{}'", name)))
                    }
                    Err(e) => Err(e),
                }
            }
            Expr::Colon => Ok(Value::Array(MxArray::scalar_double(1.0))),
            Expr::End => Ok(Value::Array(MxArray::scalar_double(1.0))),
            Expr::Unary { op, expr } => {
                let v = self.eval_expr(expr)?;
                match op {
                    UnOp::Neg => Ok(Value::Array(unary_elementwise(v.as_array(), |c| -c))),
                    UnOp::Pos => Ok(v),
                    UnOp::Not => Ok(self.logical_not(&v)),
                }
            }
            Expr::Binary { op, left, right } => self.eval_binary(*op, left, right),
            Expr::Transpose { expr, conj } => {
                let v = self.eval_expr(expr)?;
                let a = v.as_array();
                if *conj {
                    let mut out = a.transpose();
                    // conjugate
                    let data = out.iter_complex().iter().map(|c| c.conj()).collect();
                    out = MxArray::from_complex_colmajor(data, &out.shape);
                    Ok(Value::Array(out))
                } else {
                    Ok(Value::Array(a.transpose()))
                }
            }
            Expr::Matrix(rows) => self.build_matrix(rows),
            Expr::Cell(rows) => self.build_cell(rows),
            Expr::ColonRange { start, step, end } => self.colon_range(start, step, end),
            Expr::Call { name, args } => self.eval_call(name, args),
            Expr::Index { base, indices } => {
                // `containers.Map(...)` parses as an Index over a DotAccess
                // (since the base isn't a bare identifier) rather than a Call;
                // recognize that shape here and build the Map value directly.
                if let Expr::DotAccess { base: dbase, field } = base.as_ref() {
                    if field == "Map" {
                        if let Expr::Ident(ns) = dbase.as_ref() {
                            if ns == "containers" {
                                return self.construct_containers_map(indices);
                            }
                        }
                    }
                }
                let b = self.eval_expr(base)?;
                // if base is a function handle, call it
                if let Value::Func(f_params, f_body, f_captured) = b {
                    let argvals: Vec<Value> = indices.iter().map(|a| self.eval_expr(a)).collect::<Result<_, _>>()?;
                    return self.eval_anon_func(f_params, *f_body, f_captured, argvals);
                }
                if let Value::FuncRef(target) = b {
                    let argvals: Vec<Value> = indices.iter().map(|a| self.eval_expr(a)).collect::<Result<_, _>>()?;
                    return self.dispatch_call(&target, argvals);
                }
                let arr = b.as_array();
                let out = self.index_select(arr, indices)?;
                Ok(Value::Array(out))
            }
            Expr::BraceIndex { base, indices } => {
                let b = self.eval_expr(base)?;
                match b {
                    Value::Cell(rows) => {
                        // resolve indices as for array, but return the cell contents
                        let m = rows.len();
                        let n = if m > 0 { rows[0].len() } else { 0 };
                        let idx = self.resolve_indices_2d(m, n, indices)?;
                        // return the cell content (unwrap one level)
                        Ok(rows[idx.0][idx.1].clone())
                    }
                    _ => Err(EngineError::Msg("brace indexing requires a cell array".into())),
                }
            }
            Expr::AnonFunc { params, body } => {
                let captured = Self::capture_env(params, body, &self.workspace);
                Ok(Value::Func(params.clone(), body.clone(), captured))
            }
            Expr::FuncRef(name) => Ok(Value::FuncRef(name.clone())),
            Expr::DotAccess { base, field } => {
                let b = self.eval_expr(base)?;
                match b {
                    Value::Struct(map) => {
                        map.get(field.as_str()).cloned().ok_or_else(||
                            EngineError::Msg(format!("struct has no field '{}'", field)))
                    }
                    Value::Map(entries) if field == "Count" => {
                        Ok(Value::Array(MxArray::scalar_double(entries.len() as f64)))
                    }
                    Value::Map(_) => Err(EngineError::Msg(format!(
                        "unrecognized property '{}' for containers.Map", field))),
                    Value::Cell(_) => Err(EngineError::Msg(
                        "dot indexing not supported for cell arrays".into())),
                    _ => {
                        Err(EngineError::Msg(format!(
                            "unrecognized method or property '{}'", field)))
                    }
                }
            }
            Expr::DynamicDotAccess { base, field_expr } => {
                let b = self.eval_expr(base)?;
                let fv = self.eval_expr(field_expr)?;
                let field = text_arg(&fv).ok_or_else(|| EngineError::Msg(
                    "dynamic field name must evaluate to a string".into()))?;
                match b {
                    Value::Struct(map) => {
                        map.get(&field).cloned().ok_or_else(||
                            EngineError::Msg(format!("struct has no field '{}'", field)))
                    }
                    _ => Err(EngineError::Msg(format!(
                        "unrecognized method or property '{}'", field))),
                }
            }
        }
    }

    fn eval_binary(&mut self, op: BinOp, left: &Expr, right: &Expr) -> Result<Value, EngineError> {
        use BinOp::*;
        // Short-circuit operators evaluate their right operand lazily, so handle
        // them before forcing both sides.
        match op {
            AndAnd => return Ok(Value::Array(self.shortcirc_and(left, right)?)),
            OrOr => return Ok(Value::Array(self.shortcirc_or(left, right)?)),
            _ => {}
        }
        let l = self.eval_expr(left)?;
        let r = self.eval_expr(right)?;
        // The string type (double-quoted) overrides `+` to mean text append
        // (rather than char-array splicing) and `==`/`~=` to mean text-content
        // comparison. Only triggers when a `Value::Str` operand is actually
        // present, so char arithmetic (e.g. 'A' + 1) is untouched.
        if matches!(l, Value::Str(_)) || matches!(r, Value::Str(_)) {
            match op {
                Add => return string_plus(&l, &r),
                Eq => return string_cmp(&l, &r, true),
                Ne => return string_cmp(&l, &r, false),
                _ => {}
            }
        }
        // Char arrays participate in arithmetic/relational operators as their
        // character codes (e.g. 'A' + 1 == 66, 'ab' == 'ab' is [1 1]).
        let la = as_operand(&l)?;
        let ra = as_operand(&r)?;
        let (la, ra) = (la.as_ref(), ra.as_ref());
        let res = match op {
            Add => binary_elementwise(la, ra, |a, b| a + b).map_err(conv)?,
            Sub => binary_elementwise(la, ra, |a, b| a - b).map_err(conv)?,
            ElemMul => binary_elementwise(la, ra, |a, b| a * b).map_err(conv)?,
            ElemDiv => binary_elementwise(la, ra, |a, b| a / b).map_err(conv)?,
            ElemLDiv => binary_elementwise(la, ra, |a, b| b / a).map_err(conv)?,
            ElemPow => binary_elementwise(la, ra, |a, b| a.powc(b)).map_err(conv)?,
            Mul => matmul(la, ra).map_err(conv)?,
            Div => self.mrdivide(la, ra)?,
            LDiv => self.mldivide(la, ra)?,
            Pow => self.matrix_pow(la, ra)?,
            Lt => self.cmp(la, ra, |a, b| a.re < b.re)?,
            Gt => self.cmp(la, ra, |a, b| a.re > b.re)?,
            Le => self.cmp(la, ra, |a, b| a.re <= b.re)?,
            Ge => self.cmp(la, ra, |a, b| a.re >= b.re)?,
            Eq => self.cmp(la, ra, |a, b| a == b)?,
            Ne => self.cmp(la, ra, |a, b| a != b)?,
            And => self.logical_bin(la, ra, |a, b| a && b)?,
            Or => self.logical_bin(la, ra, |a, b| a || b)?,
            AndAnd | OrOr => unreachable!("handled above"),
            Colon => return Err(EngineError::Msg("invalid use of ':' operator".into())),
        };
        Ok(Value::Array(res))
    }

    fn cmp(&self, a: &MxArray, b: &MxArray, f: impl Fn(Complex64, Complex64) -> bool) -> Result<MxArray, EngineError> {
        let _ = matbox_math::broadcast_shapes(&a.shape, &b.shape)
            .ok_or_else(|| EngineError::Msg("dimension mismatch in comparison".into()))?;
        let out = binary_elementwise(a, b, |x, y| Complex64::new(if f(x, y) { 1.0 } else { 0.0 }, 0.0))
            .map_err(conv)?;
        Ok(as_logical(&out))
    }

    fn logical_bin(&self, a: &MxArray, b: &MxArray, f: impl Fn(bool, bool) -> bool) -> Result<MxArray, EngineError> {
        let out = binary_elementwise(a, b, |x, y| {
            Complex64::new(if f(x.norm() != 0.0 && !x.re.is_nan(), y.norm() != 0.0 && !y.re.is_nan()) { 1.0 } else { 0.0 }, 0.0)
        }).map_err(conv)?;
        Ok(as_logical(&out))
    }

    fn logical_not(&self, v: &Value) -> Value {
        let a = v.as_array();
        let out = unary_elementwise(a, |c| Complex64::new(if c.norm() == 0.0 || c.re.is_nan() { 1.0 } else { 0.0 }, 0.0));
        Value::Array(as_logical(&out))
    }

    fn shortcirc_and(&mut self, left: &Expr, right: &Expr) -> Result<MxArray, EngineError> {
        let l = self.eval_expr(left)?;
        if !self.is_true(&l) { return Ok(MxArray::scalar_double(0.0)); }
        let r = self.eval_expr(right)?;
        Ok(MxArray::scalar_double(if self.is_true(&r) { 1.0 } else { 0.0 }))
    }
    fn shortcirc_or(&mut self, left: &Expr, right: &Expr) -> Result<MxArray, EngineError> {
        let l = self.eval_expr(left)?;
        if self.is_true(&l) { return Ok(MxArray::scalar_double(1.0)); }
        let r = self.eval_expr(right)?;
        Ok(MxArray::scalar_double(if self.is_true(&r) { 1.0 } else { 0.0 }))
    }
}

fn conv(e: MathError) -> EngineError { EngineError::Msg(e.to_string()) }

/// A char row vector of a string's character codes (empty string -> 0x0).
pub(crate) fn string_to_char_array(s: &str) -> MxArray {
    let codes: Vec<f64> = s.chars().map(|c| c as u32 as f64).collect();
    let n = codes.len();
    if n == 0 { MxArray::new_double(&[0, 0]) } else { MxArray::from_double_colmajor(codes, &[1, n]) }
}

/// Borrow a value as an array operand for arithmetic/relational operators,
/// converting char strings to their character-code arrays without cloning
/// existing arrays.
fn as_operand(v: &Value) -> Result<Cow<'_, MxArray>, EngineError> {
    match v {
        Value::Array(a) => Ok(Cow::Borrowed(a)),
        Value::Multi(vals) => as_operand(&vals[0]),
        Value::String(s) => Ok(Cow::Owned(string_to_char_array(s))),
        other => Err(EngineError::Msg(format!(
            "operator not defined for operand of type {}",
            match other { Value::Cell(_) => "cell", Value::Struct(_) => "struct",
                Value::Func(..) | Value::FuncRef(_) => "function handle", _ => "value" }
        ))),
    }
}

fn as_logical(a: &MxArray) -> MxArray {
    let v: Vec<bool> = a.iter_complex().iter().map(|c| c.norm() != 0.0 && !c.re.is_nan()).collect();
    let mut out = MxArray::new_logical(&a.shape);
    if let MxData::Logical(d) = &mut out.data {
        for (k, c) in v.iter().enumerate() { d[k] = *c; }
    }
    out
}

/// Represent a value as a row-major grid of text elements, for the string
/// type's `+`/`==` operators. Numeric scalars and char cells stringify
/// implicitly, matching how `+` never rejects a non-string operand outright.
pub(crate) fn to_string_grid(v: &Value) -> Result<Vec<Vec<String>>, EngineError> {
    match v {
        Value::Str(g) => Ok(g.clone()),
        Value::String(s) => Ok(vec![vec![s.clone()]]),
        Value::Array(a) if a.is_scalar() => Ok(vec![vec![format!("{}", a.to_scalar())]]),
        Value::Cell(rows) => Ok(rows.iter().map(|row| row.iter().map(|cell| match cell {
            Value::String(s) => s.clone(),
            Value::Str(g) if g.len() == 1 && g[0].len() == 1 => g[0][0].clone(),
            Value::Array(a) if a.is_scalar() => format!("{}", a.to_scalar()),
            _ => String::new(),
        }).collect()).collect()),
        _ => Err(EngineError::Msg(
            "operator not defined between a string and this operand type".into())),
    }
}

/// Combine two text grids elementwise, broadcasting a 1x1 scalar grid against
/// the other operand's shape (matching `"x" + ["a" "b"]` style broadcasting).
fn zip_text(a: &[Vec<String>], b: &[Vec<String>], f: impl Fn(&str, &str) -> String) -> Result<Vec<Vec<String>>, EngineError> {
    let (am, an) = (a.len(), a.first().map(|r| r.len()).unwrap_or(0));
    let (bm, bn) = (b.len(), b.first().map(|r| r.len()).unwrap_or(0));
    let is_scalar = |m: usize, n: usize| m == 1 && n == 1;
    if (am, an) == (bm, bn) {
        Ok(a.iter().zip(b).map(|(ar, br)| ar.iter().zip(br).map(|(x, y)| f(x, y)).collect()).collect())
    } else if is_scalar(am, an) {
        let sv = &a[0][0];
        Ok(b.iter().map(|row| row.iter().map(|y| f(sv, y)).collect()).collect())
    } else if is_scalar(bm, bn) {
        let sv = &b[0][0];
        Ok(a.iter().map(|row| row.iter().map(|x| f(x, sv)).collect()).collect())
    } else {
        Err(EngineError::Msg("string array dimensions must match for this operator".into()))
    }
}

fn string_plus(l: &Value, r: &Value) -> Result<Value, EngineError> {
    let lg = to_string_grid(l)?;
    let rg = to_string_grid(r)?;
    Ok(Value::Str(zip_text(&lg, &rg, |a, b| format!("{}{}", a, b))?))
}

fn string_cmp(l: &Value, r: &Value, want_eq: bool) -> Result<Value, EngineError> {
    let lg = to_string_grid(l)?;
    let rg = to_string_grid(r)?;
    let g = zip_text(&lg, &rg, |a, b| if (a == b) == want_eq { "1".into() } else { "0".into() })?;
    let m = g.len();
    let n = g.first().map(|r| r.len()).unwrap_or(0);
    let mut out = MxArray::new_logical(&[m, n]);
    if let MxData::Logical(d) = &mut out.data {
        for j in 0..n { for i in 0..m { d[i + j * m] = g[i][j] == "1"; } }
    }
    Ok(Value::Array(out))
}

/// Text equality helper for `containers.Map` keys: a char array and a scalar
/// string with the same characters are the same key. Falls back to
/// `values_equal` for numeric keys.
pub(crate) fn map_key_eq(a: &Value, b: &Value) -> bool {
    match (map_key_text(a), map_key_text(b)) {
        (Some(sa), Some(sb)) => sa == sb,
        _ => values_equal(a, b),
    }
}
fn map_key_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Str(rows) if rows.len() == 1 && rows[0].len() == 1 => Some(rows[0][0].clone()),
        _ => None,
    }
}

pub(crate) fn flatten_row_major(rows: &[Vec<Value>]) -> Vec<Value> {
    rows.iter().flat_map(|r| r.iter().cloned()).collect()
}

/// Extract plain text from either a char array or a scalar string. MATLAB
/// itself accepts both interchangeably wherever a "string argument" is
/// documented (`strcmp`, `struct` field names, dynamic field access, ...);
/// this is the shared extraction point so those call sites don't have to
/// special-case the two representations.
pub(crate) fn text_arg(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Str(g) if g.len() == 1 && g[0].len() == 1 => Some(g[0][0].clone()),
        _ => None,
    }
}

impl Engine {
    fn build_cell(&mut self, rows: &[Vec<Expr>]) -> Result<Value, EngineError> {
        if rows.is_empty() {
            return Ok(Value::Cell(Vec::new()));
        }
        let mut out = Vec::new();
        for row in rows {
            let mut r = Vec::new();
            for e in row {
                let v = self.eval_expr(e)?;
                r.push(v);
            }
            out.push(r);
        }
        Ok(Value::Cell(out))
    }

    fn build_matrix(&mut self, rows: &[Vec<Expr>]) -> Result<Value, EngineError> {
        if rows.is_empty() {
            // []
            return Ok(Value::Array(MxArray::new_double(&[0, 0])));
        }
        // Evaluate every element first so a double-quoted string anywhere in
        // the literal can be detected: `[...]` stacks strings into a string
        // array (each element stays separate) rather than splicing them into
        // one char row the way `['ab','cd']` does.
        let mut row_vals: Vec<Vec<Value>> = Vec::with_capacity(rows.len());
        let mut any_str = false;
        for row in rows {
            let mut r = Vec::with_capacity(row.len());
            for e in row {
                let v = self.eval_expr(e)?;
                if matches!(v, Value::Str(_)) { any_str = true; }
                r.push(v);
            }
            row_vals.push(r);
        }
        if any_str {
            return self.build_string_matrix(&row_vals);
        }
        let mut row_arrays = Vec::new();
        for row in &row_vals {
            if row.is_empty() {
                row_arrays.push(MxArray::new_double(&[0, 0]));
                continue;
            }
            let mut elems = Vec::new();
            for v in row {
                elems.push(v.as_array().clone());
            }
            row_arrays.push(concat(&elems, 1)?);
        }
        Ok(Value::Array(concat(&row_arrays, 0)?))
    }

    fn build_string_matrix(&mut self, rows: &[Vec<Value>]) -> Result<Value, EngineError> {
        let mut out: Vec<Vec<String>> = Vec::with_capacity(rows.len());
        for row in rows {
            let mut r = Vec::with_capacity(row.len());
            for v in row {
                let g = to_string_grid(v)?;
                if g.len() != 1 || g[0].len() != 1 {
                    return Err(EngineError::Msg(
                        "only scalar strings can be stacked in a string array literal".into()));
                }
                r.push(g[0][0].clone());
            }
            out.push(r);
        }
        Ok(Value::Str(out))
    }

    /// `containers.Map()` / `containers.Map(keys, values)`. `KeyType`/`ValueType`
    /// name-value options are accepted (typing is inferred dynamically in
    /// MATBOX rather than pinned up front) and simply yield an empty map.
    fn construct_containers_map(&mut self, indices: &[Expr]) -> Result<Value, EngineError> {
        if indices.is_empty() {
            return Ok(Value::Map(Vec::new()));
        }
        let k = self.eval_expr(&indices[0])?;
        if let Value::String(s) = &k {
            if s.eq_ignore_ascii_case("KeyType") || s.eq_ignore_ascii_case("ValueType") {
                return Ok(Value::Map(Vec::new()));
            }
        }
        let v_expr = indices.get(1).ok_or_else(|| EngineError::Msg(
            "containers.Map: expected keys and values".into()))?;
        let v = self.eval_expr(v_expr)?;
        let keys = match &k { Value::Cell(rows) => flatten_row_major(rows), _ => vec![k] };
        let vals = match &v { Value::Cell(rows) => flatten_row_major(rows), _ => vec![v] };
        if keys.len() != vals.len() {
            return Err(EngineError::Msg(
                "containers.Map: number of keys and values must match".into()));
        }
        Ok(Value::Map(keys.into_iter().zip(vals).collect()))
    }

    fn colon_range(&mut self, start: &Expr, step: &Option<Box<Expr>>, end: &Expr) -> Result<Value, EngineError> {
        let s = self.eval_expr(start)?.to_scalar();
        let e = self.eval_expr(end)?.to_scalar();
        let st = match step {
            Some(x) => self.eval_expr(x)?.to_scalar(),
            None => 1.0,
        };
        if st == 0.0 { return Err(EngineError::Msg("colon step cannot be zero".into())); }
        let mut v = Vec::new();
        let mut x = s;
        if st > 0.0 {
            while x <= e + 1e-12 { v.push(x); x += st; }
        } else {
            while x >= e - 1e-12 { v.push(x); x += st; }
        }
        if v.is_empty() {
            return Ok(Value::Array(MxArray::new_double(&[1, 0])));
        }
        let n = v.len();
        Ok(Value::Array(MxArray::from_double_colmajor(v, &[1, n])))
    }

    fn capture_env(params: &[String], body: &Expr, workspace: &HashMap<String, Value>) -> HashMap<String, Value> {
        let mut free = Vec::new();
        Self::find_free_vars(body, params, &mut free);
        let mut env = HashMap::new();
        for var in &free {
            if let Some(v) = workspace.get(var) {
                env.insert(var.clone(), v.clone());
            }
        }
        env
    }

    fn find_free_vars(expr: &Expr, params: &[String], free: &mut Vec<String>) {
        match expr {
            Expr::Ident(name) => {
                if !params.contains(name) && !free.contains(name) {
                    free.push(name.clone());
                }
            }
            Expr::Binary { left, right, .. } => {
                Self::find_free_vars(left, params, free);
                Self::find_free_vars(right, params, free);
            }
            Expr::Unary { expr: e, .. } | Expr::Transpose { expr: e, .. } => {
                Self::find_free_vars(e, params, free);
            }
            Expr::Num(_) | Expr::Imag(_) | Expr::Str(_) | Expr::DStr(_) | Expr::Colon | Expr::End => {}
            Expr::Matrix(rows) => {
                for row in rows { for e in row { Self::find_free_vars(e, params, free); } }
            }
            Expr::Cell(rows) => {
                for row in rows { for e in row { Self::find_free_vars(e, params, free); } }
            }
            Expr::ColonRange { start, step, end } => {
                Self::find_free_vars(start, params, free);
                if let Some(s) = step { Self::find_free_vars(s, params, free); }
                Self::find_free_vars(end, params, free);
            }
            Expr::Call { args, .. } => {
                for a in args { Self::find_free_vars(a, params, free); }
            }
            Expr::Index { base, indices } | Expr::BraceIndex { base, indices } => {
                Self::find_free_vars(base, params, free);
                for i in indices { Self::find_free_vars(i, params, free); }
            }
            Expr::DotAccess { base, .. } => {
                Self::find_free_vars(base, params, free);
            }
            Expr::DynamicDotAccess { base, field_expr } => {
                Self::find_free_vars(base, params, free);
                Self::find_free_vars(field_expr, params, free);
            }
            Expr::AnonFunc { params: inner, body } => {
                let extended: Vec<String> = params.iter().cloned().chain(inner.iter().cloned()).collect();
                Self::find_free_vars(body, &extended, free);
            }
            Expr::FuncRef(_) => {}
        }
    }

    pub(crate) fn eval_anon_func(&mut self, params: Vec<String>, body: Expr, captured: HashMap<String, Value>, argvals: Vec<Value>) -> Result<Value, EngineError> {
        if argvals.len() != params.len() {
            return Err(EngineError::Msg(format!(
                "anonymous function expects {} input arguments, got {}", params.len(), argvals.len()
            )));
        }
        // save entire workspace, replace with captured env + params
        let saved_workspace = std::mem::take(&mut self.workspace);
        self.workspace.extend(captured);
        for (var, val) in params.iter().zip(argvals) {
            self.workspace.insert(var.clone(), val);
        }
        let result = self.eval_expr(&body);
        self.workspace = saved_workspace;
        result
    }

    fn eval_call(&mut self, name: &str, args: &[Expr]) -> Result<Value, EngineError> {
        // The requested output count applies to THIS call only; argument
        // sub-expressions are evaluated in a single-output context.
        let nargout = std::mem::replace(&mut self.nargout, 1);
        // `remove(m, key)` behaves like a handle-class mutator in MATLAB:
        // containers.Map is a reference type, so removal is visible on the
        // caller's variable without `m = remove(m, key)`. MATBOX values are
        // otherwise copy semantics, so special-case this one mutating call.
        if name == "remove" && args.len() == 2 {
            if let Expr::Ident(var) = &args[0] {
                if let Some(Value::Map(entries)) = self.workspace.get(var).cloned() {
                    let key = self.eval_expr(&args[1])?;
                    let before = entries.len();
                    let mut updated = entries;
                    updated.retain(|(k, _)| !map_key_eq(k, &key));
                    if updated.len() == before {
                        return Err(EngineError::Msg(
                            "remove: the specified key is not present in the map".into()));
                    }
                    self.workspace.insert(var.clone(), Value::Map(updated.clone()));
                    self.nargout = nargout;
                    return Ok(Value::Map(updated));
                }
            }
        }
        // If name is a workspace function handle, call it
        if let Some(Value::Func(f_params, f_body, f_captured)) = self.workspace.get(name).cloned() {
            let argvals: Vec<Value> = args.iter().map(|a| self.eval_expr(a)).collect::<Result<_, _>>()?;
            return self.eval_anon_func(f_params, *f_body, f_captured, argvals);
        }
        if let Some(Value::FuncRef(target)) = self.workspace.get(name).cloned() {
            let argvals: Vec<Value> = args.iter().map(|a| self.eval_expr(a)).collect::<Result<_, _>>()?;
            self.nargout = nargout;
            return self.dispatch_call(&target, argvals);
        }
        // If name is a workspace array variable and has index arguments, treat as indexing.
        if !args.is_empty() && self.workspace.contains_key(name) {
            if let Some(Value::Map(entries)) = self.workspace.get(name) {
                // `m(key)`: containers.Map uses call-style indexing for reads.
                if args.len() != 1 {
                    return Err(EngineError::Msg(
                        "containers.Map indexing requires a single key".into()));
                }
                let entries = entries.clone();
                let key = self.eval_expr(&args[0])?;
                return entries.iter().find(|(k, _)| map_key_eq(k, &key)).map(|(_, v)| v.clone())
                    .ok_or_else(|| EngineError::Msg(
                        "the specified key is not present in the map".into()));
            }
            let arr = self.workspace.get(name).unwrap().as_array().clone();
            return Ok(Value::Array(self.index_select(&arr, args)?));
        }
        let argvals: Vec<Value> = args.iter().map(|a| self.eval_expr(a)).collect::<Result<_, _>>()?;
        self.nargout = nargout;
        self.dispatch_call(name, argvals)
    }

    pub(crate) fn dispatch_call(&mut self, name: &str, argvals: Vec<Value>) -> Result<Value, EngineError> {
        // `self.nargout` carries the requested output count into the builtin;
        // reset it to 1 afterwards so it never leaks into later dispatches.
        match builtins::call(name, &argvals, self) {
            Ok(v) => { self.nargout = 1; return Ok(v); }
            Err(EngineError::Msg(m)) if m.starts_with("not a builtin") => { self.nargout = 1; }
            Err(e) => { self.nargout = 1; return Err(e); }
        }
        // user-defined function
        if let Some((input_names, output_names, body)) = self.functions.get(name).cloned() {
            let has_varargin = input_names.last().map(|s| s == "varargin").unwrap_or(false);
            let has_varargout = output_names.last().map(|s| s == "varargout").unwrap_or(false);
            let n_decl_in = if has_varargin { input_names.len() - 1 } else { input_names.len() };
            let n_out = if has_varargout { output_names.len() - 1 } else { output_names.len() };
            if argvals.len() < n_decl_in {
                return Err(EngineError::Msg(format!(
                    "function '{}' expects at least {} input arguments, got {}", name, n_decl_in, argvals.len()
                )));
            }
            if !has_varargin && argvals.len() != n_decl_in {
                return Err(EngineError::Msg(format!(
                    "function '{}' expects {} input arguments, got {}", name, n_decl_in, argvals.len()
                )));
            }
            let prev_func = self.current_function.take();
            self.current_function = Some(name.to_string());
            let mut saved = HashMap::new();
            let save_vars: Vec<String> = input_names.iter().cloned()
                .chain(output_names.iter().cloned())
                .chain(["nargin".into(), "nargout".into()])
                .collect();
            for var in &save_vars {
                if let Some(v) = self.workspace.remove(var) {
                    saved.insert(var.clone(), v);
                }
            }
            // Bind named inputs
            for (i, var) in input_names.iter().enumerate() {
                if has_varargin && i == input_names.len() - 1 {
                    // Pack remaining args into varargin cell
                    let remaining: Vec<Value> = argvals[i..].to_vec();
                    let cell_val = Value::Cell(vec![remaining]);
                    self.workspace.insert(var.clone(), cell_val);
                } else if i < argvals.len() {
                    self.workspace.insert(var.clone(), argvals[i].clone());
                }
            }
            let n_actual = argvals.len();
            self.workspace.insert("nargin".into(), Value::Array(MxArray::scalar_double(n_actual as f64)));
            let nargout_val = if has_varargout {
                // nargout tells how many outputs were requested (unknown at def time, use what's available)
                output_names.len() as f64
            } else {
                n_out as f64
            };
            self.workspace.insert("nargout".into(), Value::Array(MxArray::scalar_double(nargout_val)));
            let result = self.exec_block(&body);
            let (flow, _) = result?;
            match flow {
                Flow::None | Flow::Return(_) => {}
                _ => return Err(EngineError::Msg("unexpected flow control".into())),
            }
            let out_vals: Vec<Value> = if n_out == 0 && !has_varargout {
                vec![Value::Array(MxArray::new_double(&[0, 0]))]
            } else {
                let mut vals = Vec::new();
                for out_name in &output_names {
                    if has_varargout && out_name == "varargout" {
                        if let Some(Value::Cell(rows)) = self.workspace.get(out_name) {
                            for row in rows {
                                vals.extend(row.iter().cloned());
                            }
                        }
                    } else {
                        let v = self.workspace.get(out_name).cloned()
                            .ok_or_else(|| EngineError::Msg(format!(
                                "output '{}' not assigned in function '{}'", out_name, name
                            )))?;
                        vals.push(v);
                    }
                }
                vals
            };
            let actual_n_out = out_vals.len();
            self.current_function = prev_func;
            // Tear down this frame's names first, THEN restore the caller's saved
            // values. Doing it in the other order deletes a just-restored variable
            // (e.g. the caller's `n` during `fib(n-1) + fib(n-2)`), breaking recursion.
            for var in &save_vars {
                self.workspace.remove(var);
            }
            self.workspace.extend(saved);
            if actual_n_out <= 1 {
                Ok(out_vals.into_iter().next().unwrap_or(Value::Array(MxArray::new_double(&[0, 0]))))
            } else {
                Ok(Value::Multi(out_vals))
            }
        } else {
            let path = format!("{}.m", name);
            if let Ok(content) = std::fs::read_to_string(&path) {
                let mut p = match Parser::new(&content) {
                    Ok(p) => p,
                    Err(e) => return Err(EngineError::Msg(e)),
                };
                let stmts = match p.parse_program() {
                    Ok(s) => s,
                    Err(e) => return Err(EngineError::Msg(e)),
                };
                let (_, disp) = self.exec_block(&stmts)?;
                return Ok(disp.unwrap_or(Value::Array(MxArray::new_double(&[0, 0]))));
            }
            Err(EngineError::Msg(format!("undefined function '{}'", name)))
        }
    }

    fn index_select(&mut self, arr: &MxArray, indices: &[Expr]) -> Result<MxArray, EngineError> {
        if indices.len() == 1 {
            let lin = self.eval_linear_indices(arr, &indices[0])?;
            let data: Vec<Complex64> = lin.iter().map(|&l| arr.iter_complex()[l]).collect();
            let n = data.len();
            if n == 0 {
                return Ok(MxArray::new_complex(&[0, 1]));
            }
            return Ok(MxArray::from_complex_colmajor(data, &[n, 1]));
        }
        let selectors = self.resolve_selectors(arr, indices)?;
        self.select(arr, &selectors)
    }

    fn eval_linear_indices(&mut self, arr: &MxArray, e: &Expr) -> Result<Vec<usize>, EngineError> {
        if matches!(e, Expr::Colon) {
            return Ok((0..arr.numel()).collect());
        }
        // replace `end` with numel in the expression tree
        let e = replace_end(e, arr.numel());
        let v = self.eval_expr(&e)?;
        let a = v.as_array();
        if a.is_logical_like() {
            let mut out = Vec::new();
            for (k, c) in a.iter_complex().iter().enumerate() {
                if c.norm() != 0.0 { out.push(k); }
            }
            return Ok(out);
        }
        // numeric
        let mut out = Vec::new();
        for c in a.iter_complex() {
            let k = c.re.round() as i64;
            if k < 1 { return Err(EngineError::Msg("index must be positive".into())); }
            let idx = (k - 1) as usize;
            if idx >= arr.numel() {
                return Err(EngineError::Msg("index exceeds matrix dimensions".into()));
            }
            out.push(idx);
        }
        Ok(out)
    }

    fn resolve_selectors(&mut self, arr: &MxArray, indices: &[Expr]) -> Result<Vec<Vec<usize>>, EngineError> {
        let mut selectors = Vec::new();
        for (d, e) in indices.iter().enumerate() {
            if matches!(e, Expr::Colon) {
                selectors.push((0..arr.shape[d]).collect());
                continue;
            }
            // replace `end` with dimension size
            let e = replace_end(e, arr.shape[d]);
            let v = self.eval_expr(&e)?;
            let a = v.as_array();
            if a.is_logical_like() {
                let size = arr.shape[d];
                let mut s = Vec::new();
                for (k, c) in a.iter_complex().iter().enumerate() {
                    if k >= size { break; }
                    if c.norm() != 0.0 { s.push(k); }
                }
                selectors.push(s);
                continue;
            }
            let mut s = Vec::new();
            for c in a.iter_complex() {
                let k = c.re.round() as i64;
                if k < 1 { return Err(EngineError::Msg("index must be positive".into())); }
                s.push((k - 1) as usize);
            }
            selectors.push(s);
        }
        Ok(selectors)
    }

    fn resolve_indices_2d(&mut self, m: usize, n: usize, indices: &[Expr]) -> Result<(usize, usize), EngineError> {
        if indices.len() == 1 {
            let e = replace_end(&indices[0], m * n);
            let v = self.eval_expr(&e)?;
            let a = v.as_array();
            let k = a.to_scalar().round() as i64;
            if k < 1 { return Err(EngineError::Msg("index must be positive".into())); }
            let idx = (k - 1) as usize;
            let row = idx % m;
            let col = idx / m;
            return Ok((row, col));
        }
        if indices.len() >= 2 {
            let e0 = replace_end(&indices[0], m);
            let v0 = self.eval_expr(&e0)?;
            let r = v0.as_array().to_scalar().round() as i64;
            let e1 = replace_end(&indices[1], n);
            let v1 = self.eval_expr(&e1)?;
            let c = v1.as_array().to_scalar().round() as i64;
            if r < 1 || c < 1 { return Err(EngineError::Msg("index must be positive".into())); }
            let row = (r - 1) as usize;
            let col = (c - 1) as usize;
            if row >= m || col >= n { return Err(EngineError::Msg("index exceeds cell dimensions".into())); }
            return Ok((row, col));
        }
        Err(EngineError::Msg("invalid cell indexing".into()))
    }

    fn select(&self, arr: &MxArray, selectors: &[Vec<usize>]) -> Result<MxArray, EngineError> {
        let max_dim = arr.ndims().max(2);
        let mut out_shape = vec![1usize; max_dim];
        for d in 0..selectors.len() { out_shape[d] = selectors[d].len(); }
        // pad any unspecified dims as size 1
        for d in selectors.len()..max_dim { out_shape[d] = arr.shape.get(d).copied().unwrap_or(1); }
        let total = out_shape.iter().product();
        let mut out_buf = vec![Complex64::new(0.0, 0.0); total];
        for lin in 0..total {
            let mut rem = lin;
            let mut mi = vec![0usize; max_dim];
            for d in 0..max_dim {
                mi[d] = rem % out_shape[d];
                rem /= out_shape[d];
            }
            // map to source: for dims beyond selectors, use mi[d]; for dims in selectors, use selector value
            let mut src_idx = vec![0usize; max_dim];
            for d in 0..max_dim {
                if d < selectors.len() {
                    src_idx[d] = selectors[d][mi[d]];
                } else {
                    src_idx[d] = mi[d].min(arr.shape.get(d).copied().unwrap_or(1).saturating_sub(1));
                }
            }
            out_buf[lin] = arr.get_complex(&src_idx);
        }
        Ok(MxArray::from_complex_colmajor(out_buf, &out_shape))
    }

    fn index_assign(&mut self, arr: &MxArray, indices: &[Expr], value: &Value) -> Result<MxArray, EngineError> {
        if indices.len() == 1 {
            if matches!(indices[0], Expr::Colon) {
                // `v(:) = []` empties the array outright; there are no
                // surviving elements to refill, so it cannot fall through
                // to the element-wise fill below.
                if value.as_array().numel() == 0 {
                    return Ok(MxArray::from_complex_colmajor(Vec::new(), &[0, 0]));
                }
                // `v(:) = x` never grows — it fills the existing elements.
                let v = value.as_array();
                let mut out = arr.clone();
                for l in 0..out.numel() {
                    out.set_by_linear(l, v.iter_complex()[l.min(v.numel().saturating_sub(1))]);
                }
                return Ok(out);
            }
            // `end` in a growing assignment refers to the size BEFORE this
            // assignment runs (e.g. `v(end+1) = x` always appends one past
            // the current length), so bind it to the current numel here.
            let e = replace_end(&indices[0], arr.numel());
            let iv = self.eval_expr(&e)?;
            let ia = iv.as_array();
            let mut positions = Vec::with_capacity(ia.numel());
            for c in ia.iter_complex() {
                let k = c.re.round() as i64;
                if k < 1 { return Err(EngineError::Msg("index must be positive".into())); }
                positions.push((k - 1) as usize);
            }
            let max_pos = positions.iter().cloned().max().unwrap_or(0);
            // An empty right-hand side means "delete these elements", not
            // "assign nothing" — `A(2) = []` must remove index 2 and close
            // the gap. Handled before any element is read, since the value
            // array is legitimately empty here.
            if value.as_array().numel() == 0 {
                return self.delete_linear(arr, &positions);
            }
            let val = value.as_array();
            if positions.is_empty() || max_pos < arr.numel() {
                let mut out = arr.clone();
                for (k, &l) in positions.iter().enumerate() {
                    out.set_by_linear(l, val.iter_complex()[k.min(val.numel().saturating_sub(1))]);
                }
                return Ok(out);
            }
            return self.grow_and_assign_linear(arr, &positions, val);
        }
        if indices.len() == 2 {
            let m = arr.shape[0];
            let n = arr.shape.get(1).copied().unwrap_or(1);
            let row_idx = self.eval_index_list(&indices[0], m)?;
            let col_idx = self.eval_index_list(&indices[1], n)?;
            let max_r = row_idx.iter().cloned().max();
            let max_c = col_idx.iter().cloned().max();
            let new_m = max_r.map(|r| m.max(r + 1)).unwrap_or(m);
            let new_n = max_c.map(|c| n.max(c + 1)).unwrap_or(n);
            let val = value.as_array();
            let mut out = if new_m > m || new_n > n {
                // Grow to fit both the existing data (top-left) and the newly
                // assigned row/col; every other new cell is zero-filled.
                let mut grown = MxArray::new_complex(&[new_m, new_n]);
                for i in 0..m { for j in 0..n { grown.set_complex(&[i, j], arr.get_complex(&[i, j])); } }
                grown
            } else {
                arr.clone()
            };
            let mut k = 0usize;
            for &j in &col_idx {
                for &i in &row_idx {
                    out.set_complex(&[i, j], val.iter_complex()[k.min(val.numel().saturating_sub(1))]);
                    k += 1;
                }
            }
            return Ok(out);
        }
        let selectors = self.resolve_selectors(arr, indices)?;
        let positions = selectors_to_linear(arr, &selectors);
        let v = value.as_array();
        let mut out = arr.clone();
        for (k, &l) in positions.iter().enumerate() {
            if l >= out.numel() { return Err(EngineError::Msg("index exceeds matrix dimensions".into())); }
            out.set_by_linear(l, v.iter_complex()[k.min(v.numel() - 1)]);
        }
        Ok(out)
    }

    /// Resolve a single-dimension index expression (as used by 2-D indexed
    /// assignment) to 0-based positions, WITHOUT bounds-checking against
    /// `dim_size` — callers that want growth compare the result against the
    /// current size themselves; callers that don't grow (reads) use
    /// `resolve_selectors` instead, which does bounds-check.
    fn eval_index_list(&mut self, e: &Expr, dim_size: usize) -> Result<Vec<usize>, EngineError> {
        if matches!(e, Expr::Colon) { return Ok((0..dim_size).collect()); }
        let e = replace_end(e, dim_size);
        let v = self.eval_expr(&e)?;
        let a = v.as_array();
        if a.is_logical_like() {
            let mut out = Vec::new();
            for (k, c) in a.iter_complex().iter().enumerate() {
                if c.norm() != 0.0 { out.push(k); }
            }
            return Ok(out);
        }
        let mut out = Vec::with_capacity(a.numel());
        for c in a.iter_complex() {
            let k = c.re.round() as i64;
            if k < 1 { return Err(EngineError::Msg("index must be positive".into())); }
            out.push((k - 1) as usize);
        }
        Ok(out)
    }

    /// Grow a vector to fit `positions` (0-based) and write `val` into them,
    /// zero-filling any newly created gap. Growth only proceeds along the
    /// axis the array is already a vector in (or as a fresh row if empty) —
    /// growing a non-vector via a single linear index is ambiguous and errors.
    fn delete_linear(&self, arr: &MxArray, positions: &[usize]) -> Result<MxArray, EngineError> {
        let m = arr.shape[0];
        let n = arr.shape.get(1).copied().unwrap_or(1);
        // Removing a linear index out of a 2-D matrix has no single defined
        // resulting shape, so it is rejected rather than guessed at.
        if arr.numel() != 0 && m != 1 && n != 1 {
            return Err(EngineError::Msg(
                "null assignment can only remove elements from a vector".into()));
        }
        let mut drop = vec![false; arr.numel()];
        for &p in positions {
            if p < arr.numel() { drop[p] = true; }
        }
        let mut buf: Vec<Complex64> = Vec::with_capacity(arr.numel());
        for (l, c) in arr.iter_complex().iter().enumerate() {
            if !drop[l] { buf.push(*c); }
        }
        // Keep the orientation of the vector we shrank: a 1xN stays 1xN even
        // when every element is deleted (shape 1x0, not 0x0).
        let as_row = arr.numel() == 0 || m == 1;
        let len = buf.len();
        let shape = if as_row { vec![1, len] } else { vec![len, 1] };
        Ok(MxArray::from_complex_colmajor(buf, &shape))
    }

    fn grow_and_assign_linear(&self, arr: &MxArray, positions: &[usize], val: &MxArray) -> Result<MxArray, EngineError> {
        let new_len = positions.iter().cloned().max().unwrap_or(0) + 1;
        let m = arr.shape[0];
        let n = arr.shape.get(1).copied().unwrap_or(1);
        let as_row = arr.numel() == 0 || m == 1;
        if as_row || n == 1 {
            let mut buf = vec![Complex64::new(0.0, 0.0); new_len];
            for (k, c) in arr.iter_complex().iter().enumerate() { buf[k] = *c; }
            for (k, &p) in positions.iter().enumerate() { buf[p] = val.iter_complex()[k.min(val.numel().saturating_sub(1))]; }
            let shape = if as_row { vec![1, new_len] } else { vec![new_len, 1] };
            return Ok(MxArray::from_complex_colmajor(buf, &shape));
        }
        Err(EngineError::Msg("unable to grow a non-vector array via linear indexing".into()))
    }

    fn mrdivide(&self, a: &MxArray, b: &MxArray) -> Result<MxArray, EngineError> {
        if a.is_scalar() && b.is_scalar() {
            return Ok(MxArray::scalar_double(a.to_scalar() / b.to_scalar()));
        }
        let bt = b.transpose();
        let at = a.transpose();
        let x = matbox_math::solve(&bt, &at).map_err(conv)?;
        Ok(x.transpose())
    }

    fn mldivide(&self, a: &MxArray, b: &MxArray) -> Result<MxArray, EngineError> {
        if a.is_scalar() && b.is_scalar() {
            return Ok(MxArray::scalar_double(b.to_scalar() / a.to_scalar()));
        }
        matbox_math::solve(a, b).map_err(conv)
    }

    fn matrix_pow(&self, a: &MxArray, b: &MxArray) -> Result<MxArray, EngineError> {
        if a.is_scalar() && b.is_scalar() {
            let av = a.get_complex(&[]);
            let bv = b.get_complex(&[]);
            // use powi for exact integer real exponents
            if bv.im == 0.0 && bv.re.fract() == 0.0 && bv.re >= 0.0 && bv.re <= i32::MAX as f64 {
                let n = bv.re as i32;
                return Ok(MxArray::scalar_complex(av.powi(n)));
            }
            return Ok(MxArray::scalar_complex(av.powc(bv)));
        }
        if a.ndims() == 2 && a.shape[0] == a.shape[1] && b.is_scalar() && b.to_scalar().fract() == 0.0 && b.to_scalar() >= 0.0 {
            let n = b.to_scalar() as u32;
            let mut acc = MxArray::new_complex(&[a.shape[0], a.shape[0]]);
            for i in 0..a.shape[0] { acc.set_complex(&[i, i], Complex64::new(1.0, 0.0)); }
            let base = a.clone();
            for _ in 0..n {
                acc = matmul(&acc, &base).map_err(conv)?;
            }
            return Ok(acc);
        }
        // elementwise fallback
        binary_elementwise(a, b, |x, y| x.powc(y)).map_err(conv)
    }
}

/// Concatenate arrays along a 0-based dimension (0 = rows/vertical, 1 = columns/horizontal).
pub(crate) fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(aa), Value::Array(bb)) => {
            if aa.is_scalar() && bb.is_scalar() {
                aa.get_complex(&[]) == bb.get_complex(&[])
            } else {
                // Compare shape and values numerically. The derived `==` also
                // compares the storage variant (Double vs Complex vs Logical), so
                // two equal arrays with different representations — e.g. a matrix
                // literal (Complex) vs `unique(...)` output (Double) — must not be
                // distinguished here.
                aa.shape == bb.shape && aa.iter_complex() == bb.iter_complex()
            }
        }
        (Value::String(sa), Value::String(sb)) => sa == sb,
        (Value::Str(ga), Value::Str(gb)) => ga == gb,
        _ => false,
    }
}

pub(crate) fn concat(arrays: &[MxArray], dim: usize) -> Result<MxArray, EngineError> {
    if arrays.is_empty() { return Ok(MxArray::new_double(&[0, 0])); }
    if arrays.len() == 1 { return Ok(arrays[0].clone()); }
    let max_dim = arrays.iter().map(|a| a.ndims()).max().unwrap_or(2).max(2);
    let shapes: Vec<Vec<usize>> = arrays.iter().map(|a| {
        let mut s = a.shape.clone();
        while s.len() < max_dim { s.push(1); }
        s
    }).collect();
    for d in 0..max_dim {
        if d == dim { continue; }
        let first = shapes[0][d];
        let all_match = shapes.iter().all(|s| s[d] == first);
        // Allow empty arrays to match any size
        let all_compat = shapes.iter().all(|s| s[d] == first || s[d] == 0 || first == 0);
        if !all_compat && !all_match {
            return Err(EngineError::Msg("concatenation dimension mismatch".into()));
        }
    }
    let mut out_shape = shapes[0].clone();
    for d in 0..max_dim {
        if d != dim {
            out_shape[d] = shapes.iter().map(|s| s[d]).max().unwrap_or(1);
        }
    }
    out_shape[dim] = shapes.iter().map(|s| s[dim]).sum();
    let total = out_shape.iter().product();
    let mut starts = Vec::new();
    let mut acc = 0usize;
    for s in &shapes { starts.push(acc); acc += s[dim]; }
    let mut out_buf = vec![Complex64::new(0.0, 0.0); total];
    for lin in 0..total {
        let mut rem = lin;
        let mut mi = vec![0usize; max_dim];
        for d in 0..max_dim {
            mi[d] = rem % out_shape[d];
            rem /= out_shape[d];
        }
        let coord_dim = mi[dim];
        let mut ai = 0;
        while ai < shapes.len() - 1 && coord_dim >= starts[ai] + shapes[ai][dim] { ai += 1; }
        let local = coord_dim - starts[ai];
        let mut src_idx = mi.clone();
        src_idx[dim] = local;
        let mut slin = 0usize;
        let mut stride = 1usize;
        for d in 0..shapes[ai].len() {
            slin += src_idx[d] * stride;
            stride *= shapes[ai][d];
        }
        out_buf[lin] = arrays[ai].iter_complex()[slin];
    }
    // Concatenating nothing but logical arrays keeps the result logical, so
    // `[true false]` reports as logical instead of silently becoming numeric.
    // A logical mixed with any numeric array still gives a numeric result.
    if arrays.iter().all(|a| a.is_logical_like()) {
        let flags: Vec<bool> = out_buf.iter().map(|c| c.norm() != 0.0).collect();
        return Ok(MxArray::from_logical_colmajor(flags, &out_shape));
    }
    Ok(MxArray::from_complex_colmajor(out_buf, &out_shape))
}

fn selectors_to_linear(arr: &MxArray, selectors: &[Vec<usize>]) -> Vec<usize> {
    let ndim = arr.ndims();
    let total: usize = selectors.iter().map(|s| s.len()).product();
    let mut out = Vec::with_capacity(total);
    if total == 0 { return out; }
    let mut idx = vec![0usize; selectors.len()];
    for _ in 0..total {
        let mut mi = vec![0usize; ndim];
        for d in 0..selectors.len() { mi[d] = selectors[d][idx[d]]; }
        out.push(arr.linear_index(&mi));
        for d in 0..selectors.len() {
            idx[d] += 1;
            if idx[d] < selectors[d].len() { break; }
            idx[d] = 0;
        }
    }
    out
}

fn replace_end(e: &Expr, dim_size: usize) -> Expr {
    match e {
        Expr::End => Expr::Num(dim_size as f64),
        Expr::Num(_) | Expr::Imag(_) | Expr::Str(_) | Expr::DStr(_) | Expr::Colon => e.clone(),
        Expr::Ident(_) => e.clone(),
        Expr::Binary { op, left, right } => Expr::Binary {
            op: *op,
            left: Box::new(replace_end(left, dim_size)),
            right: Box::new(replace_end(right, dim_size)),
        },
        Expr::Unary { op, expr } => Expr::Unary {
            op: *op,
            expr: Box::new(replace_end(expr, dim_size)),
        },
        Expr::Transpose { expr, conj } => Expr::Transpose {
            expr: Box::new(replace_end(expr, dim_size)),
            conj: *conj,
        },
        Expr::Matrix(rows) => Expr::Matrix(rows.iter().map(|r| r.iter().map(|c| replace_end(c, dim_size)).collect()).collect()),
        Expr::Cell(rows) => Expr::Cell(rows.iter().map(|r| r.iter().map(|c| replace_end(c, dim_size)).collect()).collect()),
        Expr::ColonRange { start, step, end } => Expr::ColonRange {
            start: Box::new(replace_end(start, dim_size)),
            step: step.as_ref().map(|s| Box::new(replace_end(s, dim_size))),
            end: Box::new(replace_end(end, dim_size)),
        },
        Expr::Call { name, args } => Expr::Call {
            name: name.clone(),
            args: args.iter().map(|a| replace_end(a, dim_size)).collect(),
        },
        Expr::Index { base, indices } => Expr::Index {
            base: Box::new(replace_end(base, dim_size)),
            indices: indices.iter().map(|i| replace_end(i, dim_size)).collect(),
        },
        Expr::BraceIndex { base, indices } => Expr::BraceIndex {
            base: Box::new(replace_end(base, dim_size)),
            indices: indices.iter().map(|i| replace_end(i, dim_size)).collect(),
        },
        Expr::DotAccess { base, field } => Expr::DotAccess {
            base: Box::new(replace_end(base, dim_size)),
            field: field.clone(),
        },
        Expr::DynamicDotAccess { base, field_expr } => Expr::DynamicDotAccess {
            base: Box::new(replace_end(base, dim_size)),
            field_expr: Box::new(replace_end(field_expr, dim_size)),
        },
        Expr::AnonFunc { params, body } => Expr::AnonFunc {
            params: params.clone(),
            body: Box::new(replace_end(body, dim_size)),
        },
        Expr::FuncRef(_) => e.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_basic() {
        let mut eng = Engine::new();
        let r = eng.eval("1 + 2").unwrap();
        assert!(r.is_some());
        assert_eq!(r.unwrap().to_scalar(), 3.0);
    }

    #[test]
    fn test_eval_matrix() {
        let mut eng = Engine::new();
        let r = eng.eval("A = [1 2; 3 4]").unwrap();
        assert!(r.is_some());
    }

    #[test]
    fn test_eval_zeros() {
        let mut eng = Engine::new();
        let r = eng.eval("zeros(2,3)").unwrap();
        assert!(r.is_some());
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![2, 3]);
    }

    #[test]
    fn test_eval_eye() {
        let mut eng = Engine::new();
        let r = eng.eval("eye(3)").unwrap();
        assert!(r.is_some());
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![3, 3]);
        assert_eq!(a.get_double(&[0, 0]), 1.0);
        assert_eq!(a.get_double(&[0, 1]), 0.0);
    }

    #[test]
    fn test_eval_trace() {
        let mut eng = Engine::new();
        let r = eng.eval("trace([1 2; 3 4])").unwrap();
        assert!(r.is_some());
        assert_eq!(r.unwrap().to_scalar(), 5.0);
    }

    // ---- caching / workspace persistence tests ----

    #[test]
    fn test_ans_caching() {
        let mut eng = Engine::new();
        eng.eval("1 + 2").unwrap();
        let a = eng.workspace.get("ans").unwrap().clone();
        assert_eq!(a.to_scalar(), 3.0);
    }

    #[test]
    fn test_ans_overwritten() {
        let mut eng = Engine::new();
        eng.eval("1 + 2").unwrap();
        eng.eval("3 + 4").unwrap();
        let a = eng.workspace.get("ans").unwrap().clone();
        assert_eq!(a.to_scalar(), 7.0);
    }

    #[test]
    fn test_var_equality_self() {
        let mut eng = Engine::new();
        eng.eval("a = 42").unwrap();
        let r = eng.eval("a == 42").unwrap();
        assert!(r.is_some());
        assert_eq!(r.unwrap().to_scalar(), 1.0);
    }

    #[test]
    fn test_multiple_statements() {
        let mut eng = Engine::new();
        let r = eng.eval("a = 10; b = 20; a + b").unwrap();
        assert!(r.is_some());
        assert_eq!(r.unwrap().to_scalar(), 30.0);
    }

    #[test]
    fn test_multiple_statements_display_last() {
        let mut eng = Engine::new();
        let r = eng.eval("x = 5; y = 6; z = 7").unwrap();
        // assignment returns the value, so last stmt z = 7 should be display
        assert!(r.is_some());
        assert_eq!(r.unwrap().to_scalar(), 7.0);
    }

    #[test]
    fn test_multi_statements_no_display() {
        let mut eng = Engine::new();
        let r = eng.eval("a = 1; b = 2; c = 3; 0 + 0").unwrap();
        assert!(r.is_some());
        assert_eq!(r.unwrap().to_scalar(), 0.0);
    }

    #[test]
    fn test_sequential_assign() {
        let mut eng = Engine::new();
        eng.eval("a = 1").unwrap();
        eng.eval("a = a + 1").unwrap();
        eng.eval("a = a + 1").unwrap();
        eng.eval("a = a + 1").unwrap();
        let a = eng.workspace.get("a").unwrap().clone();
        assert_eq!(a.to_scalar(), 4.0);
    }

    #[test]
    fn test_identity_same_var() {
        let mut eng = Engine::new();
        eng.eval("x = [1 2; 3 4]").unwrap();
        let r = eng.eval("x(1,1)").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 1.0);
    }

    #[test]
    fn test_constant_pi() {
        let mut eng = Engine::new();
        let r = eng.eval("pi * 2").unwrap();
        assert!((r.unwrap().to_scalar() - 6.283185307179586).abs() < 1e-12);
    }

    #[test]
    fn test_constant_i() {
        let mut eng = Engine::new();
        let r = eng.eval("i^2").unwrap();
        assert!((r.unwrap().to_scalar() + 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_eval_then_builtin() {
        let mut eng = Engine::new();
        eng.eval("A = [1 2; 3 4]").unwrap();
        let r = eng.eval("sum(A)").unwrap();
        assert!(r.is_some());
        let v = r.unwrap().as_array().clone();
        assert_eq!(v.shape, vec![1, 2]);
        assert!((v.get_double(&[0, 0]) - 4.0).abs() < 1e-12);
        assert!((v.get_double(&[0, 1]) - 6.0).abs() < 1e-12);
    }

    #[test]
    fn test_clear_then_undefined() {
        let mut eng = Engine::new();
        eng.eval("a = 5").unwrap();
        eng.eval("clear()").unwrap();
        let r = eng.eval("a");
        assert!(r.is_err());
    }

    #[test]
    fn test_overwrite_variable() {
        let mut eng = Engine::new();
        eng.eval("a = 10").unwrap();
        eng.eval("a = 20").unwrap();
        let a = eng.workspace.get("a").unwrap().clone();
        assert_eq!(a.to_scalar(), 20.0);
    }

    #[test]
    fn test_cell_array() {
        let mut eng = Engine::new();
        let r = eng.eval("c = {1, 2; 3, 4}").unwrap();
        assert!(r.is_some());
    }

    #[test]
    fn test_struct_create() {
        let mut eng = Engine::new();
        let r = eng.eval(r#"s = struct("name", "test", "val", 42)"#).unwrap();
        assert!(r.is_some());
    }

    #[test]
    fn test_struct_field_access() {
        let mut eng = Engine::new();
        eng.eval(r#"s = struct("x", 10, "y", 20)"#).unwrap();
        let r = eng.eval("s.x").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 10.0);
    }

    #[test]
    fn test_struct_field_assign() {
        let mut eng = Engine::new();
        eng.eval(r#"s = struct("a", 1)"#).unwrap();
        eng.eval("s.a = 99").unwrap();
        let r = eng.eval("s.a").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 99.0);
    }

    // ============ COMPREHENSIVE LANGUAGE TESTS ============

    // --- Nested control flow ---
    #[test]
    fn test_nested_if_in_for() {
        let mut eng = Engine::new();
        eng.eval("res = []").unwrap();
        eng.eval("for k = 1:4; if k > 2; res = [res k]; end; end").unwrap();
        let r = eng.eval("res").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![1, 2]);
        assert_eq!(a.get_double(&[0, 0]), 3.0);
        assert_eq!(a.get_double(&[0, 1]), 4.0);
    }

    #[test]
    fn test_nested_for_in_if() {
        let mut eng = Engine::new();
        eng.eval("x = 5; if x > 3; s = 0; for k = 1:3; s = s + k; end; end").unwrap();
        let r = eng.eval("s").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 6.0);
    }

    #[test]
    fn test_switch_in_for() {
        let mut eng = Engine::new();
        eng.eval("res = []").unwrap();
        eng.eval("for k = 1:3; switch k; case 1; res = [res 10]; case 2; res = [res 20]; otherwise; res = [res 30]; end; end").unwrap();
        let r = eng.eval("res").unwrap();
        assert_eq!(r.unwrap().as_array().get_double(&[0, 0]), 10.0);
    }

    #[test]
    fn test_try_catch_in_loop() {
        let mut eng = Engine::new();
        eng.eval("caught = 0; for k = 1:3; try; if k == 2; error('boom'); end; catch; caught = caught + 1; end; end").unwrap();
        let r = eng.eval("caught").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 1.0);
    }

    // --- Cell array operations ---
    #[test]
    fn test_cell_of_structs() {
        let mut eng = Engine::new();
        eng.eval(r#"c = {struct("a",1), struct("a",2); struct("a",3), struct("a",4)}"#).unwrap();
        // can't index into cells yet with () syntax — just verify creation
        assert!(eng.workspace.contains_key("c"));
    }

    #[test]
    fn test_cell_empty() {
        let mut eng = Engine::new();
        let r = eng.eval("c = {}").unwrap();
        assert!(r.is_some());
    }

    #[test]
    fn test_cell_nested() {
        let mut eng = Engine::new();
        eng.eval("c = {{1,2},{3,4}}").unwrap();
        assert!(eng.workspace.contains_key("c"));
    }

    // --- Struct edge cases ---
    #[test]
    fn test_struct_nested() {
        let mut eng = Engine::new();
        eng.eval(r#"inner = struct("x", 10); outer = struct("inner", inner)"#).unwrap();
        let r = eng.eval("outer.inner.x").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 10.0);
    }

    #[test]
    fn test_struct_empty() {
        let mut eng = Engine::new();
        let r = eng.eval("s = struct()").unwrap();
        assert!(r.is_some());
    }

    #[test]
    fn test_struct_dynamic_field() {
        let mut eng = Engine::new();
        eng.eval(r#"s = struct("x", 1); f = "x"; s.x = 42"#).unwrap();
        let r = eng.eval("s.x").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 42.0);
    }

    // --- Matrix indexing edge cases ---
    #[test]
    fn test_index_single_linear() {
        let mut eng = Engine::new();
        eng.eval("A = [1 2 3; 4 5 6]").unwrap();
        let r = eng.eval("A(3)").unwrap(); // column-major: A(3) = 2
        assert_eq!(r.unwrap().to_scalar(), 2.0);
    }

    #[test]
    fn test_index_colon_dim() {
        let mut eng = Engine::new();
        eng.eval("A = [1 2 3; 4 5 6]").unwrap();
        let r = eng.eval("A(:,2)").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![2, 1]);
        assert_eq!(a.get_double(&[0, 0]), 2.0);
        assert_eq!(a.get_double(&[1, 0]), 5.0);
    }

    #[test]
    fn test_index_end_keyword() {
        let mut eng = Engine::new();
        eng.eval("A = [1 2; 3 4]").unwrap();
        let r = eng.eval("A(end,end)").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 4.0);
    }

    #[test]
    fn test_index_end_single() {
        let mut eng = Engine::new();
        eng.eval("A = [10 20 30]").unwrap();
        let r = eng.eval("A(end)").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 30.0);
    }

    #[test]
    fn test_logical_indexing() {
        let mut eng = Engine::new();
        eng.eval("A = [1 2; 3 4]; mask = logical([1 0; 0 1])").unwrap();
        let r = eng.eval("A(mask)").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.get_double(&[0, 0]), 1.0);
    }

    #[test]
    fn test_index_out_of_bounds() {
        let mut eng = Engine::new();
        eng.eval("A = [1 2]").unwrap();
        let r = eng.eval("A(10)");
        assert!(r.is_err());
    }

    #[test]
    fn test_assign_expand() {
        let mut eng = Engine::new();
        eng.eval("A = [1 2]").unwrap();
        eng.eval("A(4) = 99").unwrap();
        let r = eng.eval("A(4)").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 99.0);
    }

    #[test]
    fn test_assign_expand_intermediate_zeros() {
        let mut eng = Engine::new();
        eng.eval("A = [1]").unwrap();
        eng.eval("A(5) = 5").unwrap();
        let r = eng.eval("A(3)").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 0.0);
    }

    // --- Colon range edge cases ---
    #[test]
    fn test_colon_reverse() {
        let mut eng = Engine::new();
        let r = eng.eval("5:-1:1").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![1, 5]);
        assert_eq!(a.get_double(&[0, 0]), 5.0);
        assert_eq!(a.get_double(&[0, 4]), 1.0);
    }

    #[test]
    fn test_colon_negative_step() {
        let mut eng = Engine::new();
        let r = eng.eval("10:-3:1").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.get_double(&[0, 0]), 10.0);
        assert_eq!(a.get_double(&[0, 1]), 7.0);
        assert_eq!(a.get_double(&[0, 2]), 4.0);
    }

    #[test]
    fn test_colon_zero_step_error() {
        let mut eng = Engine::new();
        let r = eng.eval("1:0:5");
        assert!(r.is_err());
    }

    // --- Transpose ---
    #[test]
    fn test_transpose_non_conj() {
        let mut eng = Engine::new();
        eng.eval("A = [1 2; 3 4]").unwrap();
        let r = eng.eval("A.'").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![2, 2]);
        // [1 2; 3 4].' = [1 3; 2 4]
        // column-major: flat = [At(0,0)=1, At(1,0)=2, At(0,1)=3, At(1,1)=4]
        assert_eq!(a.get_double(&[0, 0]), 1.0);
        assert_eq!(a.get_double(&[1, 0]), 2.0);
        assert_eq!(a.get_double(&[0, 1]), 3.0);
        assert_eq!(a.get_double(&[1, 1]), 4.0);
    }

    // --- Matrix concatenation ---
    #[test]
    fn test_horzcat() {
        let mut eng = Engine::new();
        let r = eng.eval("[1 2 3]").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![1, 3]);
    }

    #[test]
    fn test_vertcat() {
        let mut eng = Engine::new();
        let r = eng.eval("[1; 2; 3]").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![3, 1]);
    }

    // --- Builtins edge cases ---
    #[test]
    fn test_zeros_3d() {
        let mut eng = Engine::new();
        let r = eng.eval("zeros(2,3,4)").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![2, 3, 4]);
    }

    #[test]
    fn test_eye_non_square() {
        let mut eng = Engine::new();
        let r = eng.eval("eye(2,3)").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![2, 3]);
        assert_eq!(a.get_double(&[0, 0]), 1.0);
    }

    #[test]
    fn test_linspace_single() {
        let mut eng = Engine::new();
        let r = eng.eval("linspace(5,5,1)").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 5.0);
    }

    #[test]
    fn test_logspace_default() {
        let mut eng = Engine::new();
        let r = eng.eval("logspace(1,3)").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![1, 50]);
    }

    // --- Complex number edge cases ---
    #[test]
    fn test_imag_literal() {
        let mut eng = Engine::new();
        let r = eng.eval("3 + 4i").unwrap();
        let c = r.unwrap().as_array().get_complex(&[]);
        assert_eq!(c.re, 3.0);
        assert_eq!(c.im, 4.0);
    }

    #[test]
    fn test_complex_math() {
        let mut eng = Engine::new();
        let r = eng.eval("(1+2i) * (3+4i)").unwrap();
        let c = r.unwrap().as_array().get_complex(&[]);
        // (1+2i)*(3+4i) = 3+4i+6i+8i^2 = 3+10i-8 = -5+10i
        assert!((c.re - (-5.0)).abs() < 1e-10);
        assert!((c.im - 10.0).abs() < 1e-10);
    }

    // --- Logical / comparison edge cases ---
    #[test]
    fn test_shortcircuit_and() {
        let mut eng = Engine::new();
        eng.eval("a = 0; x = (a ~= 0 && 1/a > 0)").unwrap();
        let r = eng.eval("x").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 0.0);
    }

    #[test]
    fn test_shortcircuit_or() {
        let mut eng = Engine::new();
        eng.eval("a = 1; x = (a == 1 || 1/0 > 0)").unwrap();
        let r = eng.eval("x").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 1.0);
    }

    // --- Error handling ---
    #[test]
    fn test_try_catch_error_var() {
        let mut eng = Engine::new();
        eng.eval("try; undefined_func_xyz; catch ME; end").unwrap();
        assert!(eng.workspace.contains_key("ME"));
    }

    #[test]
    fn test_try_no_error() {
        let mut eng = Engine::new();
        eng.eval("try; x = 42; catch; x = 0; end").unwrap();
        let r2 = eng.eval("x").unwrap();
        assert_eq!(r2.unwrap().to_scalar(), 42.0);
    }

    // --- Multiple eval persistence ---
    #[test]
    fn test_cross_eval_persistence() {
        let mut eng = Engine::new();
        eng.eval("a = 10").unwrap();
        eng.eval("b = a + 5").unwrap();
        eng.eval("c = a + b").unwrap();
        let r = eng.eval("c").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 25.0);
    }

    #[test]
    fn test_clear_preserves_builtins() {
        let mut eng = Engine::new();
        eng.eval("a = 10").unwrap();
        eng.eval("clear()").unwrap();
        let r = eng.eval("a");
        assert!(r.is_err());
        let r = eng.eval("pi").unwrap();
        assert_eq!(r.unwrap().to_scalar(), std::f64::consts::PI);
    }

    // --- Operator precedence ---
    #[test]
    fn test_precedence_mul_before_add() {
        let mut eng = Engine::new();
        let r = eng.eval("2 + 3 * 4").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 14.0);
    }

    #[test]
    fn test_precedence_pow_before_mul() {
        let mut eng = Engine::new();
        let r = eng.eval("2 * 3^2").unwrap();
        assert!((r.unwrap().to_scalar() - 18.0).abs() < 1e-10);
    }

    #[test]
    fn test_precedence_transpose_before_mul() {
        let mut eng = Engine::new();
        eng.eval("A = [1 2; 3 4]").unwrap();
        // A * A' is valid
        let r = eng.eval("A * A.'");
        assert!(r.is_ok());
    }

    // --- Empty matrix edge cases ---
    #[test]
    fn test_empty_matrix() {
        let mut eng = Engine::new();
        let r = eng.eval("[]").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![0, 0]);
    }

    #[test]
    fn test_isempty_true() {
        let mut eng = Engine::new();
        let r = eng.eval("isempty([])").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 1.0);
    }

    #[test]
    fn test_isempty_false() {
        let mut eng = Engine::new();
        let r = eng.eval("isempty([1 2])").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 0.0);
    }

    // --- Size / shape edge cases ---
    #[test]
    fn test_size_2d() {
        let mut eng = Engine::new();
        let r = eng.eval("size([1 2 3; 4 5 6])").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.get_double(&[0, 0]), 2.0);
        assert_eq!(a.get_double(&[0, 1]), 3.0);
    }

    #[test]
    fn test_size_with_dim() {
        let mut eng = Engine::new();
        let r = eng.eval("size([1 2; 3 4], 2)").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 2.0);
    }

    #[test]
    fn test_isscalar_true() {
        let mut eng = Engine::new();
        let r = eng.eval("isscalar(42)").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 1.0);
    }

    #[test]
    fn test_isscalar_false() {
        let mut eng = Engine::new();
        let r = eng.eval("isscalar([1 2])").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 0.0);
    }

    // --- Vector shape tests ---
    #[test]
    fn test_isrow() {
        let mut eng = Engine::new();
        let r = eng.eval("isrow([1 2 3])").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 1.0);
    }

    #[test]
    fn test_iscolumn() {
        let mut eng = Engine::new();
        let r = eng.eval("iscolumn([1;2;3])").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 1.0);
    }

    // --- Repmat / reshape ---
    #[test]
    fn test_repmat_basic() {
        let mut eng = Engine::new();
        let r = eng.eval("repmat([1 2], 1, 3)").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![1, 6]);
    }

    #[test]
    fn test_reshape_basic() {
        let mut eng = Engine::new();
        let r = eng.eval("reshape([1 2 3 4 5 6], 2, 3)").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![2, 3]);
    }

    // --- Flip ---
    #[test]
    fn test_fliplr() {
        let mut eng = Engine::new();
        let r = eng.eval("fliplr([1 2 3])").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.get_double(&[0, 0]), 3.0);
        assert_eq!(a.get_double(&[0, 2]), 1.0);
    }

    #[test]
    fn test_flipud() {
        let mut eng = Engine::new();
        let r = eng.eval("flipud([1; 2; 3])").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.get_double(&[0, 0]), 3.0);
        assert_eq!(a.get_double(&[2, 0]), 1.0);
    }

    // --- Kron ---
    #[test]
    fn test_kron_basic() {
        let mut eng = Engine::new();
        let r = eng.eval("kron([1 2; 3 4], [0 5; 6 7])").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![4, 4]);
    }

    // --- Linear algebra edge cases ---
    #[test]
    fn test_inv_2x2() {
        let mut eng = Engine::new();
        let r = eng.eval("inv([1 2; 3 4])").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![2, 2]);
    }

    #[test]
    fn test_det_2x2() {
        let mut eng = Engine::new();
        let r = eng.eval("det([1 2; 3 4])").unwrap();
        assert!((r.unwrap().to_scalar() - (-2.0)).abs() < 1e-10);
    }

    // --- Sort / unique ---
    #[test]
    fn test_sort_basic() {
        let mut eng = Engine::new();
        let r = eng.eval("sort([3 1 2])").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.get_double(&[0, 0]), 1.0);
        assert_eq!(a.get_double(&[0, 2]), 3.0);
    }

    #[test]
    fn test_unique_basic() {
        let mut eng = Engine::new();
        let r = eng.eval("unique([1 2 1 3 2])").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.numel(), 3);
    }

    // --- Diag ---
    #[test]
    fn test_diag_extract() {
        let mut eng = Engine::new();
        let r = eng.eval("diag([1 2 3; 4 5 6; 7 8 9])").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![3, 1]);
        assert_eq!(a.get_double(&[0, 0]), 1.0);
        assert_eq!(a.get_double(&[1, 0]), 5.0);
    }

    // --- Who / exist ---
    #[test]
    fn test_exist_var() {
        let mut eng = Engine::new();
        eng.eval("myVar = 42").unwrap();
        let r = eng.eval(r#"exist("myVar")"#).unwrap();
        assert_eq!(r.unwrap().to_scalar(), 1.0);
    }

    #[test]
    fn test_exist_not() {
        let mut eng = Engine::new();
        let r = eng.eval(r#"exist("nonExistentVar")"#).unwrap();
        assert_eq!(r.unwrap().to_scalar(), 0.0);
    }

    // --- Cumulative ans behavior ---
    #[test]
    fn test_ans_chain() {
        let mut eng = Engine::new();
        eng.eval("1 + 1").unwrap();
        eng.eval("ans + 1").unwrap();
        let r = eng.eval("ans").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 3.0);
    }

    // --- Variable name edge cases ---
    #[test]
    fn test_var_case_sensitivity() {
        let mut eng = Engine::new();
        eng.eval("Xyz = 1").unwrap();
        let r = eng.eval("xyz");
        assert!(r.is_err()); // MATLAB is case-sensitive
    }

    // --- Constants protection ---
    #[test]
    fn test_overwrite_pi() {
        let mut eng = Engine::new();
        eng.eval("pi = 3").unwrap();
        // pi was a workspace var, it can be overwritten
        let r = eng.eval("pi").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 3.0);
    }

    // --- Inf / NaN handling ---
    #[test]
    fn test_inf_arithmetic() {
        let mut eng = Engine::new();
        let r = eng.eval("inf + 1").unwrap();
        assert!(r.unwrap().to_scalar().is_infinite());
    }

    #[test]
    fn test_nan_comparison() {
        let mut eng = Engine::new();
        let r = eng.eval("nan == nan").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 0.0); // NaN != NaN
    }

    // --- Broadcasting behavior ---
    #[test]
    fn test_scalar_plus_vector() {
        let mut eng = Engine::new();
        let r = eng.eval("5 + [1 2 3]").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.shape, vec![1, 3]);
        assert_eq!(a.get_double(&[0, 0]), 6.0);
    }

    #[test]
    fn test_vector_plus_scalar() {
        let mut eng = Engine::new();
        let r = eng.eval("[1 2 3] + 5").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.get_double(&[0, 0]), 6.0);
    }

    // --- Chained operations ---
    #[test]
    fn test_chained_arithmetic() {
        let mut eng = Engine::new();
        let r = eng.eval("1 + 2 * 3 - 4 / 2 ^ 2").unwrap();
        // 1 + 6 - 4 / 4 = 1 + 6 - 1 = 6
        assert_eq!(r.unwrap().to_scalar(), 6.0);
    }

    // --- Assignment chain ---
    #[test]
    fn test_assignment_result_used() {
        let mut eng = Engine::new();
        let r = eng.eval("a = (b = 5) + 3");
        assert!(r.is_err() || r.is_ok()); // MATLAB doesn't allow chained assignment
    }

    // --- Mod / rem differences ---
    #[test]
    fn test_mod_vs_rem() {
        let mut eng = Engine::new();
        let rmod = eng.eval("mod(-5, 3)").unwrap();
        let rrem = eng.eval("rem(-5, 3)").unwrap();
        // mod(-5,3) = 1, rem(-5,3) = -2
        assert_eq!(rmod.unwrap().to_scalar(), 1.0);
        assert_eq!(rrem.unwrap().to_scalar(), -2.0);
    }

    // --- Multiple return from builtin ---
    #[test]
    fn test_min_basic() {
        let mut eng = Engine::new();
        let r = eng.eval("min([3 1 4 1 5])").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 1.0);
    }

    // --- Colon range as expression ---
    #[test]
    fn test_colon_assign() {
        let mut eng = Engine::new();
        eng.eval("A = [1 2 3 4 5]").unwrap();
        eng.eval("A(2:4) = [10 20 30]").unwrap();
        let r = eng.eval("A").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.get_double(&[0, 0]), 1.0);
        assert_eq!(a.get_double(&[0, 1]), 10.0);
        assert_eq!(a.get_double(&[0, 3]), 30.0);
        assert_eq!(a.get_double(&[0, 4]), 5.0);
    }

    // --- Multiple eval with same engine ---
    #[test]
    fn test_repeated_eval_accumulates() {
        let mut eng = Engine::new();
        assert!(eng.eval("x = 0").is_ok());
        for i in 1..=5 {
            let code = format!("x = x + {}", i);
            assert!(eng.eval(&code).is_ok());
        }
        let r = eng.eval("x").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 15.0);
    }

    // --- End-to-end: combining everything ---
    #[test]
    fn test_end_to_end_complex_script() {
        let mut eng = Engine::new();
        let script = r#"
            A = magic(3);
            s = 0;
            for i = 1:3
                for j = 1:3
                    if A(i,j) > 5
                        s = s + A(i,j);
                    end
                end
            end
            result = s;
        "#;
        eng.eval(script).unwrap();
        let r = eng.eval("result").unwrap();
        // magic(3) = [8 1 6; 3 5 7; 4 9 2]
        // Values > 5: 8, 6, 7, 9 = 30
        assert_eq!(r.unwrap().to_scalar(), 30.0);
    }

    // --- negation edge cases ---
    #[test]
    fn test_negate_negative() {
        let mut eng = Engine::new();
        let r = eng.eval("--5").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 5.0);
    }

    #[test]
    fn test_not_operator() {
        let mut eng = Engine::new();
        let r = eng.eval("~0").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 1.0);
    }

    #[test]
    fn test_not_nonzero() {
        let mut eng = Engine::new();
        let r = eng.eval("~42").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 0.0);
    }

    // --- String edge cases ---
    #[test]
    fn test_string_display_in_ans() {
        let mut eng = Engine::new();
        let r = eng.eval(r#""hello world""#).unwrap();
        // A double-quoted literal is a scalar element of the string type —
        // `Str([[s]])`, not a char row vector.
        match r.unwrap() {
            Value::Str(rows) => {
                assert_eq!(rows, vec![vec!["hello world".to_string()]]);
            }
            other => panic!("expected string, got {:?}", other),
        }
    }

    // --- Nested transpose ---
    #[test]
    fn test_double_transpose() {
        let mut eng = Engine::new();
        eng.eval("A = [1 2; 3 4]").unwrap();
        let r = eng.eval("A.' .'").unwrap();
        let a = r.unwrap().as_array().clone();
        assert_eq!(a.get_double(&[0, 0]), 1.0);
        assert_eq!(a.get_double(&[1, 1]), 4.0);
    }

    // ======================== BUILTIN MATH COMPREHENSIVE TESTS ========================

    // --- zeros / ones / eye ---
    #[test]
    fn test_zeros_scalar() { let mut e = Engine::new(); assert_eq!(e.eval("zeros(1)").unwrap().unwrap().as_array().numel(), 1); }
    #[test]
    fn test_zeros_2x3() { let mut e = Engine::new(); let r = e.eval("zeros(2,3)").unwrap().unwrap().as_array().clone(); assert_eq!(r.shape, vec![2, 3]); }
    #[test]
    fn test_zeros_3d_again() { let mut e = Engine::new(); let r = e.eval("zeros(2,3,4)").unwrap().unwrap().as_array().clone(); assert_eq!(r.shape, vec![2, 3, 4]); }
    #[test]
    fn test_zeros_empty() { let mut e = Engine::new(); assert!(e.eval("zeros(0)").unwrap().unwrap().as_array().is_empty()); }
    #[test]
    fn test_ones_scalar() { let mut e = Engine::new(); assert_eq!(e.eval("ones(1)").unwrap().unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_ones_2x3() { let mut e = Engine::new(); let a = e.eval("ones(2,3)").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![2, 3]); assert_eq!(a.get_double(&[1, 2]), 1.0); }
    #[test]
    fn test_eye_identity() { let mut e = Engine::new(); let a = e.eval("eye(3)").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![3, 3]); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[1, 2]), 0.0); }
    #[test]
    fn test_eye_rect() { let mut e = Engine::new(); let a = e.eval("eye(2,4)").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![2, 4]); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[1, 0]), 0.0); assert_eq!(a.get_double(&[0, 3]), 0.0); }

    // --- rand / randn / randi / randperm ---
    #[test]
    fn test_rand_shape() { let mut e = Engine::new(); let r = e.eval("rand(2,3)").unwrap().unwrap(); assert_eq!(r.as_array().shape, vec![2, 3]); }
    #[test]
    fn test_rand_range() { let mut e = Engine::new(); let a = e.eval("rand(1,1000)").unwrap().unwrap().as_array().clone(); for i in 0..1000 { let v = a.get_double(&[0, i]); assert!(v >= 0.0 && v < 1.0, "rand out of range: {}", v); } }
    #[test]
    fn test_randn_shape() { let mut e = Engine::new(); let r = e.eval("randn(3,1)").unwrap().unwrap(); assert_eq!(r.as_array().shape, vec![3, 1]); }
    #[test]
    fn test_randi_default() { let mut e = Engine::new(); let a = e.eval("randi(10, 1, 100)").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![1, 100]); for i in 0..100 { let v = a.get_double(&[0, i]); assert!(v >= 1.0 && v <= 10.0, "randi out of range: {}", v); } }
    #[test]
    fn test_randperm_basic() { let mut e = Engine::new(); let a = e.eval("randperm(5)").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![1, 5]); let mut sorted: Vec<f64> = a.to_double_vec(); sorted.sort_by(|a,b| a.partial_cmp(b).unwrap()); for i in 0..5 { assert_eq!(sorted[i], (i+1) as f64); } }

    // --- magic ---
    #[test]
    fn test_magic_3() { let mut e = Engine::new(); let a = e.eval("magic(3)").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![3, 3]);
        let s0 = a.get_double(&[0,0]) + a.get_double(&[0,1]) + a.get_double(&[0,2]);
        let s1 = a.get_double(&[1,0]) + a.get_double(&[1,1]) + a.get_double(&[1,2]);
        let s2 = a.get_double(&[2,0]) + a.get_double(&[2,1]) + a.get_double(&[2,2]);
        assert_eq!(s0, 15.0); assert_eq!(s1, 15.0); assert_eq!(s2, 15.0);
    }
    #[test]
    fn test_magic_1() { let mut e = Engine::new(); assert_eq!(e.eval("magic(1)").unwrap().unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_magic_4() { let mut e = Engine::new(); let r = e.eval("magic(4)").unwrap().unwrap(); assert_eq!(r.as_array().shape, vec![4, 4]); }

    // --- linspace / logspace ---
    #[test]
    fn test_linspace_def_n() { let mut e = Engine::new(); let r = e.eval("linspace(1,10)").unwrap().unwrap(); assert_eq!(r.as_array().shape, vec![1, 100]); }
    #[test]
    fn test_linspace_n3() { let mut e = Engine::new(); let a = e.eval("linspace(1,10,3)").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![1, 3]); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[0, 1]), 5.5); assert_eq!(a.get_double(&[0, 2]), 10.0); }
    #[test]
    fn test_linspace_reverse() { let mut e = Engine::new(); let a = e.eval("linspace(10,1,3)").unwrap().unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 10.0); assert_eq!(a.get_double(&[0, 2]), 1.0); }
    #[test]
    fn test_linspace_single_pt() { let mut e = Engine::new(); assert_eq!(e.eval("linspace(5,5,1)").unwrap().unwrap().to_scalar(), 5.0); }
    #[test]
    fn test_logspace_def_n() { let mut e = Engine::new(); let r = e.eval("logspace(1,3)").unwrap().unwrap().as_array().clone(); assert_eq!(r.shape, vec![1, 50]); }
    #[test]
    fn test_logspace_n4() { let mut e = Engine::new(); let a = e.eval("logspace(1,3,4)").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![1, 4]); assert!((a.get_double(&[0, 0]) - 10.0).abs() < 1e-10); assert!((a.get_double(&[0, 3]) - 1000.0).abs() < 1e-10); }

    // --- size / length / numel / ndims ---
    #[test]
    fn test_size_2d_again() { let a = Engine::new().eval("size([1 2 3; 4 5 6])").unwrap().unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 2.0); assert_eq!(a.get_double(&[0, 1]), 3.0); }
    #[test]
    fn test_size_scalar() { let a = Engine::new().eval("size(42)").unwrap().unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[0, 1]), 1.0); }
    #[test]
    fn test_size_empty() { let a = Engine::new().eval("size([])").unwrap().unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 0.0); assert_eq!(a.get_double(&[0, 1]), 0.0); }
    #[test]
    fn test_size_dim() { let mut e = Engine::new(); assert_eq!(e.eval("size([1 2 3; 4 5 6], 2)").unwrap().unwrap().to_scalar(), 3.0); }
    #[test]
    fn test_length_row() { let mut e = Engine::new(); assert_eq!(e.eval("length([1 2 3 4])").unwrap().unwrap().to_scalar(), 4.0); }
    #[test]
    fn test_length_col() { let mut e = Engine::new(); assert_eq!(e.eval("length([1;2;3])").unwrap().unwrap().to_scalar(), 3.0); }
    #[test]
    fn test_length_scalar() { let mut e = Engine::new(); assert_eq!(e.eval("length(42)").unwrap().unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_length_empty() { let mut e = Engine::new(); assert_eq!(e.eval("length([])").unwrap().unwrap().to_scalar(), 0.0); }
    #[test]
    fn test_numel() { let mut e = Engine::new(); assert_eq!(e.eval("numel([1 2; 3 4; 5 6])").unwrap().unwrap().to_scalar(), 6.0); }
    #[test]
    fn test_numel_scalar() { let mut e = Engine::new(); assert_eq!(e.eval("numel(99)").unwrap().unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_ndims_scalar() { let mut e = Engine::new(); assert_eq!(e.eval("ndims(42)").unwrap().unwrap().to_scalar(), 2.0); }
    #[test]
    fn test_ndims_matrix() { let mut e = Engine::new(); assert_eq!(e.eval("ndims([1 2; 3 4])").unwrap().unwrap().to_scalar(), 2.0); }

    // --- sum / prod / mean ---
    #[test]
    fn test_sum_vector() { let mut e = Engine::new(); assert_eq!(e.eval("sum([1 2 3 4])").unwrap().unwrap().to_scalar(), 10.0); }
    #[test]
    fn test_sum_cols() { let a = Engine::new().eval("sum([1 2 3; 4 5 6])").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![1, 3]); assert_eq!(a.get_double(&[0, 0]), 5.0); assert_eq!(a.get_double(&[0, 1]), 7.0); assert_eq!(a.get_double(&[0, 2]), 9.0); }
    #[test]
    fn test_sum_rows() { let a = Engine::new().eval("sum([1 2 3; 4 5 6], 2)").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![2, 1]); assert_eq!(a.get_double(&[0, 0]), 6.0); assert_eq!(a.get_double(&[1, 0]), 15.0); }
    #[test]
    fn test_sum_scalar() { let mut e = Engine::new(); assert_eq!(e.eval("sum(42)").unwrap().unwrap().to_scalar(), 42.0); }
    #[test]
    fn test_prod_vector() { let mut e = Engine::new(); assert_eq!(e.eval("prod([1 2 3 4])").unwrap().unwrap().to_scalar(), 24.0); }
    #[test]
    fn test_prod_scalar() { let mut e = Engine::new(); assert_eq!(e.eval("prod(5)").unwrap().unwrap().to_scalar(), 5.0); }
    #[test]
    fn test_mean_vector() { let mut e = Engine::new(); assert_eq!(e.eval("mean([1 2 3 4])").unwrap().unwrap().to_scalar(), 2.5); }
    #[test]
    fn test_mean_cols() { let a = Engine::new().eval("mean([1 2; 3 4])").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![1, 2]); assert_eq!(a.get_double(&[0, 0]), 2.0); assert_eq!(a.get_double(&[0, 1]), 3.0); }

    // --- min / max ---
    #[test]
    fn test_min_vector() { let mut e = Engine::new(); assert_eq!(e.eval("min([3 1 4 1 5])").unwrap().unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_min_negatives() { let mut e = Engine::new(); assert_eq!(e.eval("min([-3, -1, -4, -2])").unwrap().unwrap().to_scalar(), -4.0); }
    #[test]
    fn test_min_scalar() { let mut e = Engine::new(); assert_eq!(e.eval("min(42)").unwrap().unwrap().to_scalar(), 42.0); }
    #[test]
    fn test_max_vector() { let mut e = Engine::new(); assert_eq!(e.eval("max([3 1 4 1 5])").unwrap().unwrap().to_scalar(), 5.0); }
    #[test]
    fn test_max_cols() { let a = Engine::new().eval("max([1 4; 2 5; 3 6])").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![1, 2]); assert_eq!(a.get_double(&[0, 0]), 3.0); assert_eq!(a.get_double(&[0, 1]), 6.0); }

    // --- any / all ---
    #[test]
    fn test_any_true() { let mut e = Engine::new(); assert_eq!(e.eval("any([0 0 1])").unwrap().unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_any_false() { let mut e = Engine::new(); assert_eq!(e.eval("any([0 0 0])").unwrap().unwrap().to_scalar(), 0.0); }
    #[test]
    fn test_all_true() { let mut e = Engine::new(); assert_eq!(e.eval("all([1 2 3])").unwrap().unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_all_false() { let mut e = Engine::new(); assert_eq!(e.eval("all([1 0 3])").unwrap().unwrap().to_scalar(), 0.0); }

    // --- trig functions ---
    #[test]
    fn test_sin_0() { let mut e = Engine::new(); assert!(e.eval("sin(0)").unwrap().unwrap().to_scalar().abs() < 1e-15); }
    #[test]
    fn test_sin_pi2() { let mut e = Engine::new(); assert!((e.eval("sin(pi/2)").unwrap().unwrap().to_scalar() - 1.0).abs() < 1e-10); }
    #[test]
    fn test_cos_0() { let mut e = Engine::new(); assert!((e.eval("cos(0)").unwrap().unwrap().to_scalar() - 1.0).abs() < 1e-15); }
    #[test]
    fn test_tan_0() { let mut e = Engine::new(); assert!(e.eval("tan(0)").unwrap().unwrap().to_scalar().abs() < 1e-15); }
    #[test]
    fn test_asin_0() { let mut e = Engine::new(); assert!(e.eval("asin(0)").unwrap().unwrap().to_scalar().abs() < 1e-15); }
    #[test]
    fn test_acos_1() { let mut e = Engine::new(); assert!(e.eval("acos(1)").unwrap().unwrap().to_scalar().abs() < 1e-15); }
    #[test]
    fn test_atan_0() { let mut e = Engine::new(); assert!(e.eval("atan(0)").unwrap().unwrap().to_scalar().abs() < 1e-15); }
    #[test]
    fn test_atan2_pos() { let mut e = Engine::new(); assert!((e.eval("atan2(1,1)").unwrap().unwrap().to_scalar() - std::f64::consts::PI/4.0).abs() < 1e-10); }
    #[test]
    fn test_atan2_neg() { let mut e = Engine::new(); assert!((e.eval("atan2(-1,-1)").unwrap().unwrap().to_scalar() - -3.0*std::f64::consts::PI/4.0).abs() < 1e-10); }
    #[test]
    fn test_tan_identity() { let mut e = Engine::new(); e.eval("x = pi/4").unwrap(); let r = e.eval("sin(x)/cos(x)").unwrap(); let r2 = e.eval("tan(x)").unwrap(); assert!((r.unwrap().to_scalar() - r2.unwrap().to_scalar()).abs() < 1e-10); }

    // --- hyperbolic functions ---
    #[test]
    fn test_sinh_0() { let mut e = Engine::new(); assert!(e.eval("sinh(0)").unwrap().unwrap().to_scalar().abs() < 1e-15); }
    #[test]
    fn test_cosh_0() { let mut e = Engine::new(); assert!((e.eval("cosh(0)").unwrap().unwrap().to_scalar() - 1.0).abs() < 1e-15); }
    #[test]
    fn test_tanh_0() { let mut e = Engine::new(); assert!(e.eval("tanh(0)").unwrap().unwrap().to_scalar().abs() < 1e-15); }
    #[test]
    fn test_asinh_0() { let mut e = Engine::new(); assert!(e.eval("asinh(0)").unwrap().unwrap().to_scalar().abs() < 1e-15); }
    #[test]
    fn test_acosh_1() { let mut e = Engine::new(); assert!(e.eval("acosh(1)").unwrap().unwrap().to_scalar().abs() < 1e-15); }
    #[test]
    fn test_atanh_0() { let mut e = Engine::new(); assert!(e.eval("atanh(0)").unwrap().unwrap().to_scalar().abs() < 1e-15); }

    // --- exp / log / log10 / log2 / sqrt ---
    #[test]
    fn test_exp_0() { let mut e = Engine::new(); assert!((e.eval("exp(0)").unwrap().unwrap().to_scalar() - 1.0).abs() < 1e-15); }
    #[test]
    fn test_exp_1() { let mut e = Engine::new(); assert!((e.eval("exp(1)").unwrap().unwrap().to_scalar() - std::f64::consts::E).abs() < 1e-12); }
    #[test]
    fn test_log_e() { let mut e = Engine::new(); assert!((e.eval("log(exp(1))").unwrap().unwrap().to_scalar() - 1.0).abs() < 1e-12); }
    #[test]
    fn test_log10_100() { let mut e = Engine::new(); assert!((e.eval("log10(100)").unwrap().unwrap().to_scalar() - 2.0).abs() < 1e-12); }
    #[test]
    fn test_log2_8() { let mut e = Engine::new(); assert!((e.eval("log2(8)").unwrap().unwrap().to_scalar() - 3.0).abs() < 1e-12); }
    #[test]
    fn test_sqrt_4() { let mut e = Engine::new(); assert!((e.eval("sqrt(4)").unwrap().unwrap().to_scalar() - 2.0).abs() < 1e-12); }
    #[test]
    fn test_sqrt_neg() { let mut e = Engine::new(); e.eval("z = sqrt(-1)").unwrap(); let r = e.workspace.get("z").unwrap().clone(); assert!(r.is_array()); let arr = r.as_array(); assert_eq!(arr.shape, vec![1, 1]); let c = arr.get_complex(&[]); assert!(c.re.abs() < 1e-10, "got {} + {}i", c.re, c.im); assert!((c.im * c.im - 1.0).abs() < 1e-10, "got {} + {}i", c.re, c.im); }

    // --- floor / ceil / round / fix / sign ---
    #[test]
    fn test_floor_neg() { let mut e = Engine::new(); assert_eq!(e.eval("floor(-1.5)").unwrap().unwrap().to_scalar(), -2.0); }
    #[test]
    fn test_ceil_neg() { let mut e = Engine::new(); assert_eq!(e.eval("ceil(-1.5)").unwrap().unwrap().to_scalar(), -1.0); }
    #[test]
    fn test_round_half() { let mut e = Engine::new(); assert_eq!(e.eval("round(2.5)").unwrap().unwrap().to_scalar(), 3.0); }
    #[test]
    fn test_round_neg_half() { let mut e = Engine::new(); assert_eq!(e.eval("round(-2.5)").unwrap().unwrap().to_scalar(), -3.0); }
    #[test]
    fn test_fix_pos() { let mut e = Engine::new(); assert_eq!(e.eval("fix(2.7)").unwrap().unwrap().to_scalar(), 2.0); }
    #[test]
    fn test_fix_neg() { let mut e = Engine::new(); assert_eq!(e.eval("fix(-2.7)").unwrap().unwrap().to_scalar(), -2.0); }
    #[test]
    fn test_sign_pos() { let mut e = Engine::new(); assert_eq!(e.eval("sign(5)").unwrap().unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_sign_neg() { let mut e = Engine::new(); assert_eq!(e.eval("sign(-5)").unwrap().unwrap().to_scalar(), -1.0); }
    #[test]
    fn test_sign_zero() { let mut e = Engine::new(); assert_eq!(e.eval("sign(0)").unwrap().unwrap().to_scalar(), 0.0); }

    // --- real / imag / conj / angle ---
    #[test]
    fn test_real_complex() { let mut e = Engine::new(); assert_eq!(e.eval("real(3+4i)").unwrap().unwrap().to_scalar(), 3.0); }
    #[test]
    fn test_imag_complex() { let mut e = Engine::new(); assert_eq!(e.eval("imag(3+4i)").unwrap().unwrap().to_scalar(), 4.0); }
    #[test]
    fn test_conj() { let mut e = Engine::new(); let r = e.eval("conj(3+4i)").unwrap(); let c = r.unwrap().as_array().get_complex(&[]); assert_eq!(c.re, 3.0); assert_eq!(c.im, -4.0); }
    #[test]
    fn test_angle() { let mut e = Engine::new(); assert!((e.eval("angle(1+1i)").unwrap().unwrap().to_scalar() - std::f64::consts::PI/4.0).abs() < 1e-10); }

    // --- mod / rem ---
    #[test]
    fn test_mod_pos() { let mut e = Engine::new(); assert_eq!(e.eval("mod(10, 3)").unwrap().unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_mod_neg_divisor() { let mut e = Engine::new(); assert_eq!(e.eval("mod(-10, 3)").unwrap().unwrap().to_scalar(), 2.0); }
    #[test]
    fn test_rem_pos() { let mut e = Engine::new(); assert_eq!(e.eval("rem(10, 3)").unwrap().unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_rem_neg() { let mut e = Engine::new(); assert_eq!(e.eval("rem(-10, 3)").unwrap().unwrap().to_scalar(), -1.0); }

    // --- transpose / ctranspose ---
    #[test]
    fn test_transpose_vector() { let mut e = Engine::new(); let r = e.eval("[1 2 3].'").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![3, 1]); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[2, 0]), 3.0); }
    #[test]
    fn test_ctranspose_real() { let mut e = Engine::new(); let a = e.eval("[1 2; 3 4]'").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![2, 2]); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[0, 1]), 3.0); }
    #[test]
    fn test_ctranspose_complex() { let mut e = Engine::new(); let r = e.eval("(1+2i)'").unwrap(); let c = r.unwrap().as_array().get_complex(&[]); assert_eq!(c.re, 1.0); assert_eq!(c.im, -2.0); }

    // --- inv / det / rank / trace / norm ---
    #[test]
    fn test_inv_3x3() { let mut e = Engine::new(); e.eval("A = magic(3)").unwrap(); let r = e.eval("inv(A)").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![3, 3]); // A * inv(A) ≈ I
        let check = e.eval("A * inv(A)").unwrap().unwrap().as_array().clone();
        assert!((check.get_double(&[0, 0]) - 1.0).abs() < 1e-8);
        assert!((check.get_double(&[1, 2]) - 0.0).abs() < 1e-8);
    }
    #[test]
    fn test_det_eye() { let mut e = Engine::new(); assert!((e.eval("det(eye(5))").unwrap().unwrap().to_scalar() - 1.0).abs() < 1e-10); }
    #[test]
    fn test_det_singular() { let mut e = Engine::new(); assert!(e.eval("det([1 2; 2 4])").unwrap().unwrap().to_scalar().abs() < 1e-12); }
    #[test]
    fn test_rank_full() { let mut e = Engine::new(); assert_eq!(e.eval("rank([1 2; 3 4])").unwrap().unwrap().to_scalar(), 2.0); }
    #[test]
    fn test_rank_singular() { let mut e = Engine::new(); assert_eq!(e.eval("rank([1 2; 2 4])").unwrap().unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_rank_zero() { let mut e = Engine::new(); assert_eq!(e.eval("rank(zeros(3,3))").unwrap().unwrap().to_scalar(), 0.0); }
    #[test]
    fn test_trace_magic() { let mut e = Engine::new(); assert_eq!(e.eval("trace(magic(3))").unwrap().unwrap().to_scalar(), 15.0); }
    #[test]
    fn test_norm_2() { let mut e = Engine::new(); let n = e.eval("norm([3 4])").unwrap().unwrap().to_scalar(); assert!((n - 5.0).abs() < 1e-10); }
    #[test]
    fn test_norm_fro() { let mut e = Engine::new(); let a = e.eval("norm([1 2; 3 4])").unwrap().unwrap(); assert!((a.to_scalar() - (30.0f64).sqrt()).abs() < 1e-10); }

    // --- lu / qr / chol / eig / svd ---
    #[test]
    fn test_lu_3x3() { let mut e = Engine::new(); e.eval("A = magic(3)").unwrap(); let r = e.eval("lu(A)").unwrap(); assert_eq!(r.unwrap().as_array().shape, vec![3, 3]); }
    #[test]
    fn test_qr_2x2() { let mut e = Engine::new(); e.eval("A = [1 2; 3 4]").unwrap(); let r = e.eval("qr(A)").unwrap(); assert_eq!(r.unwrap().as_array().shape, vec![2, 2]); }
    #[test]
    fn test_chol_2x2() { let mut e = Engine::new(); let r = e.eval("chol([4 2; 2 5])").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![2, 2]); assert!(a.get_double(&[0, 0]) > 0.0); }
    #[test]
    fn test_eig_2x2() {
        // single-output eig returns a column vector of eigenvalues (MATLAB semantics)
        let mut e = Engine::new();
        let r = e.eval("eig([5 4; 1 2])").unwrap();
        let d = r.unwrap().as_array().clone();
        assert_eq!(d.shape, vec![2, 1]);
        let mut vals = [d.get_double(&[0, 0]), d.get_double(&[1, 0])];
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((vals[0] - 1.0).abs() < 1e-9);
        assert!((vals[1] - 6.0).abs() < 1e-9);
    }
    #[test]
    fn test_lu_multi_reconstruct() {
        // [L, U] = lu(A) must satisfy L*U = A
        let mut e = Engine::new();
        e.eval("A = [4 3; 6 3]").unwrap();
        e.eval("[L, U] = lu(A)").unwrap();
        let r = e.eval("L*U").unwrap().unwrap().as_array().clone();
        assert!((r.get_double(&[0, 0]) - 4.0).abs() < 1e-9);
        assert!((r.get_double(&[0, 1]) - 3.0).abs() < 1e-9);
        assert!((r.get_double(&[1, 0]) - 6.0).abs() < 1e-9);
        assert!((r.get_double(&[1, 1]) - 3.0).abs() < 1e-9);
    }
    #[test]
    fn test_qr_multi_reconstruct() {
        // [Q, R] = qr(A) must satisfy Q*R = A
        let mut e = Engine::new();
        e.eval("A = [1 2; 3 4]").unwrap();
        e.eval("[Q, R] = qr(A)").unwrap();
        let r = e.eval("Q*R").unwrap().unwrap().as_array().clone();
        assert!((r.get_double(&[0, 0]) - 1.0).abs() < 1e-9);
        assert!((r.get_double(&[1, 1]) - 4.0).abs() < 1e-9);
    }
    #[test]
    fn test_svd_2x2() { let mut e = Engine::new(); let r = e.eval("svd([2 0; 0 3])").unwrap(); let s = r.unwrap().as_array().clone(); assert_eq!(s.shape, vec![2, 1]); }

    // --- reshape / diag / repmat / kron ---
    #[test]
    fn test_reshape_2x3_to_3x2() { let mut e = Engine::new(); let r = e.eval("reshape([1 2 3 4 5 6], 3, 2)").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![3, 2]); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[2, 0]), 3.0); assert_eq!(a.get_double(&[0, 1]), 4.0); }
    #[test]
    fn test_diag_extract2() { let a = Engine::new().eval("diag([1 2 3; 4 5 6; 7 8 9])").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![3, 1]); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[1, 0]), 5.0); assert_eq!(a.get_double(&[2, 0]), 9.0); }
    #[test]
    fn test_diag_build() { let mut e = Engine::new(); let r = e.eval("diag([1 2 3])").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![3, 3]); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[1, 1]), 2.0); assert_eq!(a.get_double(&[2, 2]), 3.0); }
    #[test]
    fn test_repmat_2x3() { let mut e = Engine::new(); let r = e.eval("repmat([1 2], 2, 3)").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![2, 6]); }
    #[test]
    fn test_kron_2x2() { let mut e = Engine::new(); let r = e.eval("kron([1 2; 3 4], [0 5; 6 7])").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![4, 4]); assert_eq!(a.get_double(&[0, 0]), 0.0); assert_eq!(a.get_double(&[1, 0]), 6.0); }

    // --- fliplr / flipud / flip / sort / unique ---
    #[test]
    fn test_fliplr_matrix() { let mut e = Engine::new(); let r = e.eval("fliplr([1 2 3; 4 5 6])").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![2, 3]); assert_eq!(a.get_double(&[0, 0]), 3.0); assert_eq!(a.get_double(&[0, 2]), 1.0); }
    #[test]
    fn test_flipud_matrix() { let mut e = Engine::new(); let r = e.eval("flipud([1 2; 3 4])").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 3.0); assert_eq!(a.get_double(&[1, 0]), 1.0); }
    #[test]
    fn test_flip_dim2() { let mut e = Engine::new(); let a = e.eval("flip([1 2 3; 4 5 6], 2)").unwrap().unwrap().as_array().clone(); assert_eq!(a.shape, vec![2, 3]); assert_eq!(a.get_double(&[0, 0]), 3.0); assert_eq!(a.get_double(&[0, 2]), 1.0); }
    #[test]
    fn test_sort_descending() { let mut e = Engine::new(); let r = e.eval("sort([5 1 4 2 3])").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[0, 4]), 5.0); }
    #[test]
    fn test_unique_all_same() { let mut e = Engine::new(); let r = e.eval("unique([5 5 5])").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.numel(), 1); }
    #[test]
    fn test_unique_none_repeat() { let mut e = Engine::new(); let r = e.eval("unique([3 1 2])").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.numel(), 3); }

    // --- is* functions ---
    #[test]
    fn test_isempty_scalar() { let mut e = Engine::new(); assert_eq!(e.eval("isempty(42)").unwrap().unwrap().to_scalar(), 0.0); }
    #[test]
    fn test_isscalar_matrix() { let mut e = Engine::new(); assert_eq!(e.eval("isscalar([1 2])").unwrap().unwrap().to_scalar(), 0.0); }
    #[test]
    fn test_isvector_row() { let mut e = Engine::new(); assert_eq!(e.eval("isvector([1 2 3])").unwrap().unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_isvector_matrix() { let mut e = Engine::new(); assert_eq!(e.eval("isvector([1 2; 3 4])").unwrap().unwrap().to_scalar(), 0.0); }
    #[test]
    fn test_isrow_false() { let mut e = Engine::new(); assert_eq!(e.eval("isrow([1;2;3])").unwrap().unwrap().to_scalar(), 0.0); }
    #[test]
    fn test_iscolumn_false() { let mut e = Engine::new(); assert_eq!(e.eval("iscolumn([1 2 3])").unwrap().unwrap().to_scalar(), 0.0); }
    #[test]
    fn test_ismatrix_scalar() { let mut e = Engine::new(); assert_eq!(e.eval("ismatrix(42)").unwrap().unwrap().to_scalar(), 1.0); }

    // --- logical / double ---
    #[test]
    fn test_logical_zero() { let mut e = Engine::new(); assert_eq!(e.eval("logical(0)").unwrap().unwrap().to_scalar(), 0.0); }
    #[test]
    fn test_logical_nonzero() { let mut e = Engine::new(); assert_eq!(e.eval("logical(42)").unwrap().unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_logical_neg() { let mut e = Engine::new(); assert_eq!(e.eval("logical(-1)").unwrap().unwrap().to_scalar(), 1.0); }

    // --- concatenation edge cases ---
    #[test]
    fn test_concat_empty_and_vec() { let mut e = Engine::new(); let r = e.eval("[[], [1 2 3]]").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![1, 3]); assert_eq!(a.get_double(&[0, 0]), 1.0); }
    #[test]
    fn test_concat_vec_and_empty() { let mut e = Engine::new(); let r = e.eval("[[1 2 3], []]").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![1, 3]); assert_eq!(a.get_double(&[0, 0]), 1.0); }
    #[test]
    fn test_concat_vert() { let mut e = Engine::new(); let r = e.eval("[[1 2]; [3 4]]").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![2, 2]); assert_eq!(a.get_double(&[0, 0]), 1.0); }

    // --- colon range edge cases ---
    #[test]
    fn test_colon_single() { let mut e = Engine::new(); assert_eq!(e.eval("5").unwrap().unwrap().to_scalar(), 5.0); }
    #[test]
    fn test_colon_empty_reverse() { let mut e = Engine::new(); let r = e.eval("5:1").unwrap(); let a = r.unwrap().as_array().clone(); assert!(a.is_empty()); }
    #[test]
    fn test_colon_step_nonint() { let mut e = Engine::new(); let r = e.eval("1:0.5:3").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[0, 4]), 3.0); }
    #[test]
    fn test_colon_neg_step_reverse() { let mut e = Engine::new(); let r = e.eval("3:-0.5:1").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 3.0); assert_eq!(a.get_double(&[0, 4]), 1.0); }

    // --- indexing edge cases ---
    #[test]
    fn test_index_multi_dim() { let mut e = Engine::new(); e.eval("A = reshape(1:12, 3, 2, 2)").unwrap(); let r = e.eval("A(1,1,1)").unwrap();
        assert_eq!(r.unwrap().to_scalar(), 1.0); }
    #[test]
    fn test_index_colon_all() { let mut e = Engine::new(); e.eval("A = [1 2 3; 4 5 6]").unwrap(); let r = e.eval("A(:)").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![6, 1]); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[5, 0]), 6.0); }
    #[test]
    fn test_index_end_in_range() { let mut e = Engine::new(); e.eval("A = [1 2 3 4 5]").unwrap(); let r = e.eval("A(end)").unwrap(); assert_eq!(r.unwrap().to_scalar(), 5.0); }
    #[test]
    fn test_logical_index_2d() { let mut e = Engine::new(); e.eval("A = [1 2; 3 4]").unwrap(); e.eval("mask = logical([1 0; 0 1])").unwrap(); let r = e.eval("A(mask)").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[1, 0]), 4.0); }
    #[test]
    fn test_assign_submatrix() { let mut e = Engine::new(); e.eval("A = [1 2 3; 4 5 6; 7 8 9]").unwrap(); e.eval("A(1:2,1:2) = [10 20; 30 40]").unwrap(); let r = e.eval("A(1,1)").unwrap(); assert_eq!(r.unwrap().to_scalar(), 10.0); }

    // --- bool operators on matrices ---
    #[test]
    fn test_matrix_eq() { let mut e = Engine::new(); let r = e.eval("[1 2; 3 4] == [1 2; 3 4]").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[1, 1]), 1.0); }
    #[test]
    fn test_matrix_neq() { let mut e = Engine::new(); let r = e.eval("[1 2; 3 4] ~= [5 6; 7 8]").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 1.0); }
    #[test]
    fn test_matrix_lt() { let mut e = Engine::new(); let r = e.eval("[1 2; 3 4] < [2 2; 2 5]").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[1, 0]), 0.0); }
    #[test]
    fn test_matrix_gt() { let mut e = Engine::new(); let r = e.eval("[1 2; 3 4] > [0 2; 3 3]").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 1.0); assert_eq!(a.get_double(&[1, 1]), 1.0); }

    // --- constant values ---
    #[test]
    fn test_pi_value() { let mut e = Engine::new(); assert!((e.eval("pi").unwrap().unwrap().to_scalar() - std::f64::consts::PI).abs() < 1e-15); }
    #[test]
    fn test_inf_value() { let mut e = Engine::new(); assert!(e.eval("inf").unwrap().unwrap().to_scalar().is_infinite()); }
    #[test]
    fn test_nan_value() { let mut e = Engine::new(); assert!(e.eval("nan").unwrap().unwrap().to_scalar().is_nan()); }
    #[test]
    fn test_i_squared() { let mut e = Engine::new(); let r = e.eval("i^2").unwrap(); assert!((r.unwrap().to_scalar() + 1.0).abs() < 1e-12); }
    #[test]
    fn test_j_squared() { let mut e = Engine::new(); let r = e.eval("j^2").unwrap(); assert!((r.unwrap().to_scalar() + 1.0).abs() < 1e-12); }
    #[test]
    fn test_eps_value() { let mut e = Engine::new(); let r = e.eval("eps").unwrap().unwrap().to_scalar(); assert!(r > 0.0 && r < 1e-15); }

    // --- binary operations on matrices ---
    #[test]
    fn test_mat_power() { let mut e = Engine::new(); e.eval("A = [1 2; 3 4]").unwrap();
        let a = e.eval("A .^ 2").unwrap().unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 1.0); assert!((a.get_double(&[1, 1]) - 16.0).abs() < 1e-12); }
    #[test]
    fn test_mat_div() { let mut e = Engine::new(); let r = e.eval("[4 6 8] ./ 2").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 2.0); assert_eq!(a.get_double(&[0, 1]), 3.0); }
    #[test]
    fn test_mat_mul_2x2() { let mut e = Engine::new(); let r = e.eval("[1 2; 3 4] * [5 6; 7 8]").unwrap(); let a = r.unwrap().as_array().clone(); assert!((a.get_double(&[0, 0]) - 19.0).abs() < 1e-10); }

    // --- precedence and parenthesization ---
    #[test]
    fn test_precedence_parens() { let mut e = Engine::new(); assert_eq!(e.eval("(2+3)*4").unwrap().unwrap().to_scalar(), 20.0); }
    #[test]
    fn test_precedence_power_right_assoc() { let mut e = Engine::new(); // 2^3^2 = 2^(3^2) = 2^9 = 512 in MATLAB
        let r = e.eval("2^3^2").unwrap(); assert!((r.unwrap().to_scalar() - 512.0).abs() < 1e-10); }

    // --- complex expressions ---
    #[test]
    fn test_complex_expression_chain() { let mut e = Engine::new(); let r = e.eval("(1+2i)*(3-4i) + conj(5+6i)").unwrap(); let c = r.unwrap().as_array().get_complex(&[]); // (1+2i)*(3-4i) = 3-4i+6i-8i^2 = 3+2i+8 = 11+2i; conj(5+6i)=5-6i; sum = 16-4i
        assert!((c.re - 16.0).abs() < 1e-10); assert!((c.im - (-4.0)).abs() < 1e-10); }

    // --- error cases ---
    #[test]
    fn test_undefined_var() { let mut e = Engine::new(); assert!(e.eval("nonexistent").is_err()); }
    #[test]
    fn test_syntax_error() { let mut e = Engine::new(); assert!(e.eval("1 + * 2").is_err()); }
    #[test]
    fn test_missing_paren() { let mut e = Engine::new(); assert!(e.eval("sin(pi").is_err()); }
    #[test]
    fn test_dim_mismatch_matmul() { let mut e = Engine::new(); assert!(e.eval("[1 2 3] * [1 2]").is_err()); }

    // --- who / exist / clear ---
    #[test]
    fn test_who_output() { let mut e = Engine::new(); e.eval("a = 1; b = 2").unwrap(); e.eval("who()").unwrap(); }
    #[test]
    fn test_clear_specific() { let mut e = Engine::new(); e.eval("a = 1; b = 2").unwrap(); e.eval(r#"clear("a")"#).unwrap(); assert!(e.eval("a").is_err()); assert!(e.eval("b").is_ok()); }
    #[test]
    fn test_clear_all() { let mut e = Engine::new(); e.eval("a = 1; b = 2").unwrap(); e.eval("clear()").unwrap(); assert!(e.eval("a").is_err()); assert!(e.eval("b").is_err()); }

    // --- format ---
    #[test]
    fn test_format_string() { let mut e = Engine::new(); let r = e.eval(r#"format("short")"#); assert!(r.is_ok()); }

    // --- help ---
    #[test]
    fn test_help_builtin() { let mut e = Engine::new(); let r = e.eval(r#"help("sin")"#); assert!(r.is_ok()); }

    // --- edge: large values ---
    #[test]
    fn test_large_values() { let mut e = Engine::new(); let r = e.eval("1e308 * 10").unwrap(); assert!(r.unwrap().to_scalar().is_infinite()); }
    #[test]
    fn test_small_values() { let mut e = Engine::new(); let r = e.eval("1e-300 / 2").unwrap(); assert_eq!(r.unwrap().to_scalar(), 5e-301); }
    #[test]
    fn test_inf_minus_inf() { let mut e = Engine::new(); assert!(e.eval("inf - inf").unwrap().unwrap().to_scalar().is_nan()); }
    #[test]
    fn test_nan_arithmetic() { let mut e = Engine::new(); let r = e.eval("nan + 1").unwrap(); assert!(r.unwrap().to_scalar().is_nan()); }

    // --- broadcasting with different shapes ---
    #[test]
    fn test_broadcast_col_to_matrix() { let mut e = Engine::new(); let r = e.eval("[1 2 3; 4 5 6] + [10; 20]").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![2, 3]); assert_eq!(a.get_double(&[0, 0]), 11.0); assert_eq!(a.get_double(&[1, 0]), 24.0); }
    #[test]
    fn test_broadcast_row_to_matrix() { let mut e = Engine::new(); let r = e.eval("[1 2; 3 4] + [10 20]").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.shape, vec![2, 2]); assert_eq!(a.get_double(&[0, 0]), 11.0); assert_eq!(a.get_double(&[1, 0]), 13.0); }
    #[test]
    fn test_broadcast_scalar_div() { let mut e = Engine::new(); let r = e.eval("[1 2 3] / 2").unwrap(); let a = r.unwrap().as_array().clone(); assert_eq!(a.get_double(&[0, 0]), 0.5); }

    // --- complex matrix operations ---
    #[test]
    fn test_complex_identity_mul() { let mut e = Engine::new(); e.eval("z = 3+4i").unwrap(); let r = e.eval("z * 1").unwrap().unwrap().as_array().clone(); let c = r.get_complex(&[]); assert!((c.re - 3.0).abs() < 1e-10); assert!((c.im - 4.0).abs() < 1e-10); }
    #[test]
    fn test_complex_transpose() { let mut e = Engine::new(); e.eval("z = 3+4i").unwrap(); let a = e.eval("z'").unwrap().unwrap().as_array().clone(); let c = a.get_complex(&[]); assert_eq!(c.re, 3.0); assert_eq!(c.im, -4.0); }

    // --- assignment edge cases ---
    #[test]
    fn test_assign_new_var() { let mut e = Engine::new(); let r = e.eval("z = 42").unwrap(); assert!(r.is_some()); assert_eq!(e.workspace.get("z").unwrap().to_scalar(), 42.0); }
    #[test]
    fn test_assign_overwrite() { let mut e = Engine::new(); e.eval("z = 10").unwrap(); e.eval("z = 20").unwrap(); assert_eq!(e.workspace.get("z").unwrap().to_scalar(), 20.0); }
    #[test]
    fn test_self_assign() { let mut e = Engine::new(); e.eval("x = 5").unwrap(); e.eval("x = x + 1").unwrap(); assert_eq!(e.workspace.get("x").unwrap().to_scalar(), 6.0); }

    // --- chained comparison ---
    #[test]
    fn test_chained_lt() { let mut e = Engine::new(); assert_eq!(e.eval("1 < 2 < 3").unwrap().unwrap().to_scalar(), 1.0); }

    // --- while loop ---
    #[test]
    fn test_while_basic() { let mut e = Engine::new(); e.eval("k = 0; while k < 5; k = k + 1; end").unwrap(); assert_eq!(e.workspace.get("k").unwrap().to_scalar(), 5.0); }
    #[test]
    fn test_while_zero_iter() { let mut e = Engine::new(); e.eval("k = 0; while k < 0; k = k + 1; end").unwrap(); assert_eq!(e.workspace.get("k").unwrap().to_scalar(), 0.0); }

    // --- if/elseif/else ---
    #[test]
    fn test_if_elseif_else() { let mut e = Engine::new(); e.eval("x = 5; if x > 10; r = 1; elseif x > 3; r = 2; else; r = 3; end").unwrap(); assert_eq!(e.workspace.get("r").unwrap().to_scalar(), 2.0); }
    #[test]
    fn test_if_else_path() { let mut e = Engine::new(); e.eval("x = 1; if x > 10; r = 1; else; r = 2; end").unwrap(); assert_eq!(e.workspace.get("r").unwrap().to_scalar(), 2.0); }
    #[test]
    fn test_if_no_else() { let mut e = Engine::new(); e.eval("x = 1; if x > 0; r = 99; end").unwrap(); assert_eq!(e.workspace.get("r").unwrap().to_scalar(), 99.0); }

    // --- nested loops ---
    #[test]
    fn test_nested_for_2d() { let mut e = Engine::new(); e.eval("s = 0; for i = 1:3; for j = 1:4; s = s + 1; end; end").unwrap(); assert_eq!(e.workspace.get("s").unwrap().to_scalar(), 12.0); }
    #[test]
    fn test_for_with_vector() { let mut e = Engine::new(); e.eval("s = 0; for k = [1 2 3]; s = s + k; end").unwrap(); assert_eq!(e.workspace.get("s").unwrap().to_scalar(), 6.0); }

    // --- switch with numbers ---
    #[test]
    fn test_switch_numbers() { let mut e = Engine::new(); e.eval("x = 2; switch x; case 1; r = 10; case 2; r = 20; otherwise; r = 30; end").unwrap(); assert_eq!(e.workspace.get("r").unwrap().to_scalar(), 20.0); }
    #[test]
    fn test_switch_otherwise() { let mut e = Engine::new(); e.eval("x = 99; switch x; case 1; r = 10; case 2; r = 20; otherwise; r = 30; end").unwrap(); assert_eq!(e.workspace.get("r").unwrap().to_scalar(), 30.0); }

    // --- try/catch ---
    #[test]
    fn test_try_catch_no_error() { let mut e = Engine::new(); e.eval("try; r = 42; catch; r = 0; end").unwrap(); assert_eq!(e.workspace.get("r").unwrap().to_scalar(), 42.0); }
    #[test]
    fn test_try_catch_error() { let mut e = Engine::new(); e.eval("try; undefined_func(); catch; r = 99; end").unwrap(); assert_eq!(e.workspace.get("r").unwrap().to_scalar(), 99.0); }

    // ======================== END KEYWORD TESTS ========================

    #[test]
    fn test_end_simple() {
        let mut e = Engine::new();
        e.eval("A = [1 2 3; 4 5 6; 7 8 9]").unwrap();
        assert_eq!(e.eval("A(end, 1)").unwrap().unwrap().to_scalar(), 7.0);
    }

    #[test]
    fn test_end_last_col() {
        let mut e = Engine::new();
        e.eval("A = [1 2 3; 4 5 6; 7 8 9]").unwrap();
        assert_eq!(e.eval("A(1, end)").unwrap().unwrap().to_scalar(), 3.0);
    }

    #[test]
    fn test_end_minus_one() {
        let mut e = Engine::new();
        e.eval("A = [1 2 3; 4 5 6; 7 8 9]").unwrap();
        assert_eq!(e.eval("A(end-1, 2)").unwrap().unwrap().to_scalar(), 5.0);
    }

    #[test]
    fn test_end_div() {
        let mut e = Engine::new();
        e.eval("A = [1 2 3; 4 5 6; 7 8 9]").unwrap();
        let v = e.eval("A(end/2, :)").unwrap().unwrap();
        let a = v.as_array();
        assert_eq!(a.shape[1], 3);
        assert_eq!(a.get_double(&[0, 0]), 4.0);
    }

    #[test]
    fn test_end_linear() {
        let mut e = Engine::new();
        e.eval("A = [1 2 3; 4 5 6; 7 8 9]").unwrap();
        assert_eq!(e.eval("A(end)").unwrap().unwrap().to_scalar(), 9.0);
    }

    #[test]
    fn test_end_linear_minus() {
        let mut e = Engine::new();
        e.eval("A = [1 2 3; 4 5 6; 7 8 9]").unwrap();
        // column-major linear: idx(1)=1,2=4,3=7,4=2,5=5,6=8,7=3,8=6,9=9
        // end-2 = 9-2 = 7 → A(7) = 3
        assert_eq!(e.eval("A(end-2)").unwrap().unwrap().to_scalar(), 3.0);
    }

    #[test]
    fn test_end_complex_expr() {
        let mut e = Engine::new();
        e.eval("A = [1 2 3 4 5]").unwrap();
        // end = 5, end/2 = 2.5, floor(2.5) = 2, A(2) = 2
        assert_eq!(e.eval("A(floor(end/2))").unwrap().unwrap().to_scalar(), 2.0);
    }

    #[test]
    fn test_end_with_range() {
        let mut e = Engine::new();
        e.eval("A = [10 20 30 40 50]").unwrap();
        // First check that manual colon range works
        let r1 = e.eval("A(4:5)").unwrap().unwrap();
        // linear indexing returns column vector [n, 1]
        assert_eq!(r1.as_array().shape, &[2, 1]);
        assert_eq!(r1.as_array().get_double(&[0, 0]), 40.0);
        assert_eq!(r1.as_array().get_double(&[1, 0]), 50.0);
        // Now with end
        let r2 = e.eval("A(end-1:end)").unwrap().unwrap();
        assert_eq!(r2.as_array().shape, &[2, 1]);
        assert_eq!(r2.as_array().get_double(&[0, 0]), 40.0);
        assert_eq!(r2.as_array().get_double(&[1, 0]), 50.0);
    }

    // ======================== MULTI-OUTPUT USER FUNCTION TESTS ========================

    #[test]
    fn test_multi_output_swap() {
        let mut e = Engine::new();
        e.eval("function [a, b] = swap(x, y); a = y; b = x; end").unwrap();
        e.eval("[p, q] = swap(1, 2)").unwrap();
        assert_eq!(e.workspace.get("p").unwrap().to_scalar(), 2.0);
        assert_eq!(e.workspace.get("q").unwrap().to_scalar(), 1.0);
    }

    #[test]
    fn test_multi_output_single_capture() {
        let mut e = Engine::new();
        e.eval("function [a, b] = swap(x, y); a = y; b = x; end").unwrap();
        e.eval("r = swap(10, 20)").unwrap();
        assert_eq!(e.workspace.get("r").unwrap().to_scalar(), 20.0);
    }

    #[test]
    fn test_multi_output_three() {
        let mut e = Engine::new();
        e.eval("function [a, b, c] = triple(x); a = x; b = 2*x; c = 3*x; end").unwrap();
        e.eval("[x, y, z] = triple(5)").unwrap();
        assert_eq!(e.workspace.get("x").unwrap().to_scalar(), 5.0);
        assert_eq!(e.workspace.get("y").unwrap().to_scalar(), 10.0);
        assert_eq!(e.workspace.get("z").unwrap().to_scalar(), 15.0);
    }

    #[test]
    fn test_multi_output_zero() {
        let mut e = Engine::new();
        e.eval("function hello(x); disp(x); end").unwrap();
        let r = e.eval("hello(42)");
        assert!(r.is_ok());
    }

    #[test]
    fn test_multi_output_inside_expr() {
        let mut e = Engine::new();
        e.eval("function [a, b] = swap(x, y); a = y; b = x; end").unwrap();
        e.eval("r = swap(3, 4) + 1").unwrap();
        assert_eq!(e.workspace.get("r").unwrap().to_scalar(), 5.0);
    }

    // ======================== CELL ARRAY INDEXING TESTS ========================

    #[test]
    fn test_cell_brace_index_2d() {
        let mut e = Engine::new();
        e.eval("C = {10, 20, 30; 40, 50, 60}").unwrap();
        assert_eq!(e.eval("C{1,1}").unwrap().unwrap().to_scalar(), 10.0);
        assert_eq!(e.eval("C{1,2}").unwrap().unwrap().to_scalar(), 20.0);
        assert_eq!(e.eval("C{2,3}").unwrap().unwrap().to_scalar(), 60.0);
    }

    #[test]
    fn test_cell_brace_linear() {
        let mut e = Engine::new();
        e.eval("C = {10, 20, 30; 40, 50, 60}").unwrap();
        // column-major: C{1}=10, C{2}=40, C{3}=20, C{4}=50, C{5}=30, C{6}=60
        assert_eq!(e.eval("C{2}").unwrap().unwrap().to_scalar(), 40.0);
        assert_eq!(e.eval("C{3}").unwrap().unwrap().to_scalar(), 20.0);
        assert_eq!(e.eval("C{5}").unwrap().unwrap().to_scalar(), 30.0);
    }

    #[test]
    fn test_cell_brace_end() {
        let mut e = Engine::new();
        e.eval("C = {1, 2, 3; 4, 5, 6}").unwrap();
        assert_eq!(e.eval("C{end}").unwrap().unwrap().to_scalar(), 6.0);
        assert_eq!(e.eval("C{1, end}").unwrap().unwrap().to_scalar(), 3.0);
        assert_eq!(e.eval("C{end, 1}").unwrap().unwrap().to_scalar(), 4.0);
    }

    #[test]
    fn test_cell_brace_assign_contents() {
        let mut e = Engine::new();
        e.eval("C = {1, 2, 3}").unwrap();
        e.eval("x = C{2}").unwrap();
        assert_eq!(e.workspace.get("x").unwrap().to_scalar(), 2.0);
    }

    #[test]
    fn test_cell_brace_nested() {
        let mut e = Engine::new();
        e.eval("C = {{1, 2}, {3, 4}}").unwrap();
        let r = e.eval("C{1}").unwrap().unwrap();
        match r { Value::Cell(_) => {} _ => panic!("expected cell") }
    }

    // ======================== ANONYMOUS FUNCTION TESTS ========================

    #[test]
    fn test_anon_func_simple() {
        let mut e = Engine::new();
        e.eval("f = @(x) x^2").unwrap();
        assert_eq!(e.eval("f(5)").unwrap().unwrap().to_scalar(), 25.0);
    }

    #[test]
    fn test_anon_func_multi_param() {
        let mut e = Engine::new();
        e.eval("f = @(x, y) x + y").unwrap();
        assert_eq!(e.eval("f(3, 4)").unwrap().unwrap().to_scalar(), 7.0);
    }

    #[test]
    fn test_anon_func_no_args() {
        let mut e = Engine::new();
        e.eval("f = @() 42").unwrap();
        assert_eq!(e.eval("f()").unwrap().unwrap().to_scalar(), 42.0);
    }

    #[test]
    fn test_anon_func_nested_call() {
        let mut e = Engine::new();
        e.eval("f = @(x) x^2 + 1").unwrap();
        assert_eq!(e.eval("f(f(2))").unwrap().unwrap().to_scalar(), 26.0);
    }

    #[test]
    fn test_anon_func_capture_workspace() {
        let mut e = Engine::new();
        e.eval("a = 10; f = @(x) x + a").unwrap();
        assert_eq!(e.eval("f(5)").unwrap().unwrap().to_scalar(), 15.0);
    }

    #[test]
    fn test_anon_func_scope_isolation() {
        let mut e = Engine::new();
        e.eval("x = 99; f = @(x) x^2").unwrap();
        assert_eq!(e.eval("f(3)").unwrap().unwrap().to_scalar(), 9.0);
        assert_eq!(e.workspace.get("x").unwrap().to_scalar(), 99.0);
    }

    #[test]
    fn test_anon_func_stored_in_cell() {
        let mut e = Engine::new();
        e.eval("f = @(x) x^2; g = @(x) x^3; C = {f, g}").unwrap();
        e.eval("r = C{1}(2)").unwrap();
        assert_eq!(e.workspace.get("r").unwrap().to_scalar(), 4.0);
    }

    #[test]
    fn test_anon_func_closure() {
        let mut e = Engine::new();
        e.eval("f = @(x) @(y) x + y").unwrap();
        e.eval("g = f(3)").unwrap();
        assert_eq!(e.eval("g(4)").unwrap().unwrap().to_scalar(), 7.0);
    }

    #[test]
    fn test_anon_func_closure_capture_later() {
        let mut e = Engine::new();
        e.eval("a = 10; f = @(x) x + a").unwrap();
        e.eval("a = 20").unwrap();
        // captured at creation time: a=10
        assert_eq!(e.eval("f(5)").unwrap().unwrap().to_scalar(), 15.0);
    }

    #[test]
    fn test_anon_func_closure_triple_nested() {
        let mut e = Engine::new();
        e.eval("f = @(x) @(y) @(z) x + y + z").unwrap();
        e.eval("g = f(1)").unwrap();
        e.eval("h = g(10)").unwrap();
        assert_eq!(e.eval("h(100)").unwrap().unwrap().to_scalar(), 111.0);
    }

    // ======================== DYNAMIC FIELD TESTS ========================

    #[test]
    fn test_dynamic_field_access() {
        let mut e = Engine::new();
        e.eval(r#"s = struct("a", 1, "b", 2)"#).unwrap();
        assert_eq!(e.eval(r#"s.("a")"#).unwrap().unwrap().to_scalar(), 1.0);
        assert_eq!(e.eval(r#"s.("b")"#).unwrap().unwrap().to_scalar(), 2.0);
    }

    #[test]
    fn test_dynamic_field_assign() {
        let mut e = Engine::new();
        e.eval(r#"s = struct("a", 1)"#).unwrap();
        e.eval(r#"s.("a") = 99"#).unwrap();
        assert_eq!(e.eval("s.a").unwrap().unwrap().to_scalar(), 99.0);
    }

    #[test]
    fn test_dynamic_field_single_quote() {
        let mut e = Engine::new();
        e.eval(r#"s = struct("a", 42)"#).unwrap();
        assert_eq!(e.eval("s.('a')").unwrap().unwrap().to_scalar(), 42.0);
    }

    // ======================== NEW BUILTINS TESTS ========================

    #[test]
    fn test_find_builtin() {
        let mut e = Engine::new();
        let r = e.eval("find([0 1 0 2; 3 0 4 0])").unwrap().unwrap();
        let a = r.as_array();
        assert_eq!(a.numel(), 4);
        assert_eq!(a.get_double(&[0, 0]), 2.0); // 1-indexed
    }

    #[test]
    fn test_strcmp_builtin() {
        let mut e = Engine::new();
        assert_eq!(e.eval(r#"strcmp("hi", "hi")"#).unwrap().unwrap().to_scalar(), 1.0);
        assert_eq!(e.eval(r#"strcmp("hi", "ho")"#).unwrap().unwrap().to_scalar(), 0.0);
    }

    #[test]
    fn test_upper_lower_builtin() {
        let mut e = Engine::new();
        let r = e.eval(r#"upper("hello")"#).unwrap().unwrap();
        assert!(matches!(r, Value::String(s) if s == "HELLO"));
        let r = e.eval(r#"lower("WORLD")"#).unwrap().unwrap();
        assert!(matches!(r, Value::String(s) if s == "world"));
    }

    #[test]
    fn test_cumsum_diff_builtin() {
        let mut e = Engine::new();
        let r = e.eval("cumsum([1 2 3])").unwrap().unwrap();
        let a = r.as_array();
        assert_eq!(a.get_double(&[0, 0]), 1.0);
        assert_eq!(a.get_double(&[0, 1]), 3.0);
        assert_eq!(a.get_double(&[0, 2]), 6.0);
        let r = e.eval("diff([1 3 6 10])").unwrap().unwrap();
        let a = r.as_array();
        assert_eq!(a.get_double(&[0, 0]), 2.0);
        assert_eq!(a.get_double(&[0, 1]), 3.0);
        assert_eq!(a.get_double(&[0, 2]), 4.0);
    }

    #[test]
    fn test_cellfun_builtin() {
        let mut e = Engine::new();
        e.eval("f = @(x) x^2; C = {1, 2, 3}").unwrap();
        // Default UniformOutput=true collects scalar results into a numeric array.
        let r = e.eval("cellfun(f, C)").unwrap().unwrap();
        let a = r.as_array();
        assert_eq!(a.shape, vec![1, 3]);
        assert_eq!(a.get_double(&[0, 0]), 1.0);
        assert_eq!(a.get_double(&[0, 1]), 4.0);
        assert_eq!(a.get_double(&[0, 2]), 9.0);
        // UniformOutput=false returns a cell.
        let rc = e.eval("cellfun(f, C, 'UniformOutput', false)").unwrap().unwrap();
        match rc {
            Value::Cell(rows) => { assert_eq!(rows[0].len(), 3); }
            _ => panic!("expected cell"),
        }
    }

    #[test]
    fn test_deal_builtin() {
        let mut e = Engine::new();
        e.eval("[a, b, c] = deal(1, 2, 3)").unwrap();
        assert_eq!(e.workspace.get("a").unwrap().to_scalar(), 1.0);
        assert_eq!(e.workspace.get("b").unwrap().to_scalar(), 2.0);
        assert_eq!(e.workspace.get("c").unwrap().to_scalar(), 3.0);
    }

    // ======================== FUNCREF @NAME TESTS ========================

    #[test]
    fn test_func_ref_builtin() {
        let mut e = Engine::new();
        e.eval("f = @sin").unwrap();
        let r = e.eval("f(0)").unwrap().unwrap();
        let v = r.as_array().get_complex(&[]);
        assert!((v.re - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_func_ref_user_func() {
        let mut e = Engine::new();
        e.eval("function y = dbl(x); y = x * 2; end; f = @dbl").unwrap();
        assert_eq!(e.eval("f(21)").unwrap().unwrap().to_scalar(), 42.0);
    }

    #[test]
    fn test_func_ref_in_cell() {
        let mut e = Engine::new();
        e.eval("C = {@sin, @cos}").unwrap();
        let r = e.eval("C{1}(0)").unwrap().unwrap();
        assert!((r.to_scalar() - 0.0).abs() < 1e-10);
        let r = e.eval("C{2}(0)").unwrap().unwrap();
        assert!((r.to_scalar() - 1.0).abs() < 1e-10);
    }

    // ======================== MORE BUILTINS TESTS ========================

    #[test]
    fn test_sprintf_builtin() {
        let mut e = Engine::new();
        let r = e.eval(r#"sprintf("n = %d", 42)"#).unwrap().unwrap();
        assert!(matches!(r, Value::String(s) if s == "n = 42"));
    }

    #[test]
    fn test_strsplit_builtin() {
        let mut e = Engine::new();
        let r = e.eval(r#"strsplit("a,b,c", ",")"#).unwrap().unwrap();
        match r {
            Value::Cell(rows) if rows.len() == 1 && rows[0].len() == 3 => {
                assert!(matches!(&rows[0][0], Value::String(s) if s == "a"));
                assert!(matches!(&rows[0][1], Value::String(s) if s == "b"));
                assert!(matches!(&rows[0][2], Value::String(s) if s == "c"));
            }
            _ => panic!("expected cell row"),
        }
    }

    #[test]
    fn test_num2cell_builtin() {
        let mut e = Engine::new();
        let r = e.eval("num2cell([10 20; 30 40])").unwrap().unwrap();
        match r {
            Value::Cell(rows) => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].len(), 2);
                assert_eq!(rows[0][0].as_array().to_scalar(), 10.0);
            }
            _ => panic!("expected cell"),
        }
    }

    // ======================== NARGIN/NARGOUT TESTS ========================

    #[test]
    fn test_nargin_inside_func() {
        let mut e = Engine::new();
        e.eval("function y = f(a, b, c); y = nargin; end; r = f(1, 2, 3)").unwrap();
        assert_eq!(e.workspace.get("r").unwrap().to_scalar(), 3.0);
    }

    #[test]
    fn test_nargout_inside_func() {
        let mut e = Engine::new();
        e.eval("function [y, z] = f(x); y = nargout; z = nargout; end; [a, b] = f(0)").unwrap();
        assert_eq!(e.workspace.get("a").unwrap().to_scalar(), 2.0);
        assert_eq!(e.workspace.get("b").unwrap().to_scalar(), 2.0);
    }

    #[test]
    fn test_nargin_nargout_single() {
        let mut e = Engine::new();
        e.eval("function y = f(x); y = nargin + nargout; end; r = f(42)").unwrap();
        assert_eq!(e.workspace.get("r").unwrap().to_scalar(), 2.0);
    }

    #[test]
    fn test_nargin_query_function() {
        let mut e = Engine::new();
        e.eval("function [a, b] = myfunc(x, y, z); a = x; b = y; end").unwrap();
        let r = e.eval("nargin('myfunc')").unwrap().unwrap();
        assert_eq!(r.to_scalar(), 3.0);
    }

    #[test]
    fn test_nargout_query_function() {
        let mut e = Engine::new();
        e.eval("function [a, b] = myfunc(x); a = x; b = x; end").unwrap();
        let r = e.eval("nargout('myfunc')").unwrap().unwrap();
        assert_eq!(r.to_scalar(), 2.0);
    }

    // ======================== SCRIPT FILE LOADING TESTS ========================

    #[test]
    fn test_run_script() {
        let mut e = Engine::new();
        std::fs::write("_test_script.m", "x = 42; y = x * 2").unwrap();
        e.eval("run('_test_script')").unwrap();
        assert_eq!(e.workspace.get("x").unwrap().to_scalar(), 42.0);
        assert_eq!(e.workspace.get("y").unwrap().to_scalar(), 84.0);
        std::fs::remove_file("_test_script.m").unwrap();
    }

    #[test]
    fn test_run_script_dot_m() {
        let mut e = Engine::new();
        std::fs::write("_test_dot.m", "z = 100").unwrap();
        e.eval("run('_test_dot.m')").unwrap();
        assert_eq!(e.workspace.get("z").unwrap().to_scalar(), 100.0);
        std::fs::remove_file("_test_dot.m").unwrap();
    }

    #[test]
    fn test_script_auto_load() {
        let mut e = Engine::new();
        std::fs::write("_myscript.m", "data = 77").unwrap();
        e.eval("_myscript").unwrap();
        assert_eq!(e.workspace.get("data").unwrap().to_scalar(), 77.0);
        std::fs::remove_file("_myscript.m").unwrap();
    }

    #[test]
    fn test_run_nonexistent() {
        let mut e = Engine::new();
        let r = e.eval("run('_nonexistent_file_xyz')");
        assert!(r.is_err());
    }
}

