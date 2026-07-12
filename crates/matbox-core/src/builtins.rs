//! Builtin functions for MATBOX (MATLAB-equivalent signatures).

use crate::eval::{Engine, EngineError, Value};
use crate::eval::values_equal;
use matbox_math::array::{MxArray, MxData};
use matbox_math::linalg;
use matbox_math::{binary_elementwise, reduce, unary_elementwise};
use num_complex::Complex64;
use std::io::Write;

pub(crate) fn call(name: &str, args: &[Value], engine: &mut Engine) -> Result<Value, EngineError> {
    let a = |i: usize| -> Result<MxArray, EngineError> {
        Ok(args.get(i).ok_or_else(|| EngineError::Msg(format!("{}: not enough input arguments", name)))?.as_array().clone())
    };
    let n = args.len();

    macro_rules! elem1 {
        ($f:expr) => {{
            let x = a(0)?;
            Ok(Value::Array(unary_elementwise(&x, $f)))
        }};
    }
    macro_rules! elem2 {
        ($f:expr) => {{
            let x = a(0)?;
            let y = a(1)?;
            Ok(Value::Array(binary_elementwise(&x, &y, $f).map_err(|e| EngineError::Msg(e.to_string()))?))
        }};
    }

    match name {
        "zeros" | "ones" => {
            let shape = parse_size(args);
            let mut arr = MxArray::new_double(&shape);
            if name == "ones" {
                for k in 0..arr.numel() { arr.set_double(&linear_of(&arr, k), 1.0); }
            }
            Ok(Value::Array(arr))
        }
        "eye" => {
            let shape = parse_size(args);
            let m = shape[0];
            let nn = shape.get(1).copied().unwrap_or(m);
            let mut arr = MxArray::new_double(&[m, nn]);
            for i in 0..m.min(nn) { arr.set_double(&[i, i], 1.0); }
            Ok(Value::Array(arr))
        }
        "rand" => {
            let shape = parse_size(args);
            let mut arr = MxArray::new_double(&shape);
            for k in 0..arr.numel() { arr.set_double(&linear_of(&arr, k), engine.rng.uniform()); }
            Ok(Value::Array(arr))
        }
        "randn" => {
            let shape = parse_size(args);
            let mut arr = MxArray::new_double(&shape);
            for k in 0..arr.numel() { arr.set_double(&linear_of(&arr, k), engine.rng.normal()); }
            Ok(Value::Array(arr))
        }
        "randi" => {
            // randi(n) or randi([lo hi], ...) or randi(imax, m, n)
            let (lo, hi, rest) = if let Some(Value::Array(v)) = args.first() {
                if v.ndims() == 2 && v.shape[1] >= 2 && v.shape[0] == 1 {
                    (v.get_double(&[0, 0]) as i64, v.get_double(&[0, 1]) as i64, &args[1..])
                } else { (1, v.to_scalar() as i64, &args[1..]) }
            } else { (1, 100, &args[0..0]) };
            let shape = parse_size(rest);
            let span = (hi - lo + 1).max(1) as f64;
            let mut arr = MxArray::new_double(&shape);
            for k in 0..arr.numel() {
                arr.set_double(&linear_of(&arr, k), lo as f64 + (engine.rng.uniform() * span).floor());
            }
            Ok(Value::Array(arr))
        }
        "randperm" => {
            let nn = a(0)?.to_scalar() as usize;
            let mut v: Vec<f64> = (1..=nn).map(|x| x as f64).collect();
            for i in (1..nn).rev() {
                let j = (engine.rng.uniform() * (i as f64 + 1.0)).floor() as usize;
                v.swap(i, j);
            }
            Ok(Value::Array(MxArray::from_double_colmajor(v, &[1, nn])))
        }
        "magic" => {
            let nn = a(0)?.to_scalar() as usize;
            Ok(Value::Array(magic_square(nn)))
        }
        "linspace" => {
            let s = a(0)?.to_scalar();
            let e = a(1)?.to_scalar();
            let cnt = if n >= 3 { a(2)?.to_scalar() as usize } else { 100 };
            let cnt = cnt.max(1);
            let mut v = Vec::with_capacity(cnt);
            for k in 0..cnt {
                let t = if cnt == 1 { 0.0 } else { k as f64 / (cnt - 1) as f64 };
                v.push(s + t * (e - s));
            }
            Ok(Value::Array(MxArray::from_double_colmajor(v, &[1, cnt])))
        }
        "logspace" => {
            let s = a(0)?.to_scalar();
            let e = a(1)?.to_scalar();
            let cnt = if n >= 3 { a(2)?.to_scalar() as usize } else { 50 };
            let cnt = cnt.max(1);
            let mut v = Vec::with_capacity(cnt);
            for k in 0..cnt {
                let t = if cnt == 1 { 0.0 } else { k as f64 / (cnt - 1) as f64 };
                v.push(10f64.powf(s + t * (e - s)));
            }
            Ok(Value::Array(MxArray::from_double_colmajor(v, &[1, cnt])))
        }
        "size" => {
            let arg0 = args.first().ok_or_else(|| EngineError::Msg("size: not enough input arguments".into()))?;
            let mut shape = value_shape(arg0);
            while shape.len() < 2 { shape.push(1); } // MATLAB arrays are always >= 2-D
            if n >= 2 {
                let d = a(1)?.to_scalar() as usize;
                return Ok(Value::Array(MxArray::scalar_double(shape.get(d - 1).copied().unwrap_or(1) as f64)));
            }
            let sz: Vec<f64> = shape.iter().map(|&s| s as f64).collect();
            // [r, c, ...] = size(A): one output per requested dimension. When fewer
            // outputs are requested than dimensions, the last one holds the product
            // of the remaining sizes (MATLAB semantics).
            if engine.nargout >= 2 {
                let k = engine.nargout;
                let mut outs: Vec<Value> = Vec::with_capacity(k);
                for i in 0..k {
                    let val = if i + 1 < k {
                        sz.get(i).copied().unwrap_or(1.0)
                    } else {
                        // last output collects the product of all remaining dims
                        sz.iter().skip(i).product::<f64>()
                    };
                    outs.push(Value::Array(MxArray::scalar_double(val)));
                }
                return Ok(Value::Multi(outs));
            }
            let nn = sz.len();
            Ok(Value::Array(MxArray::from_double_colmajor(sz, &[1, nn])))
        }
        "length" => {
            let shape = value_shape(args.first().ok_or_else(|| EngineError::Msg("length: not enough input arguments".into()))?);
            // length is 0 for any empty array, else the largest dimension
            let len = if shape.iter().any(|&d| d == 0) { 0 } else { shape.iter().cloned().max().unwrap_or(0) };
            Ok(Value::Array(MxArray::scalar_double(len as f64)))
        }
        "numel" => {
            let shape = value_shape(args.first().ok_or_else(|| EngineError::Msg("numel: not enough input arguments".into()))?);
            Ok(Value::Array(MxArray::scalar_double(shape.iter().product::<usize>() as f64)))
        }
        "ndims" => {
            let shape = value_shape(args.first().ok_or_else(|| EngineError::Msg("ndims: not enough input arguments".into()))?);
            Ok(Value::Array(MxArray::scalar_double(shape.len().max(2) as f64)))
        }
        "sum" => reduce_along(&a(0)?, args.get(1), |acc, x| acc + x, 0.0),
        "prod" => reduce_along(&a(0)?, args.get(1), |acc, x| acc * x, 1.0),
        "mean" => {
            let x = a(0)?;
            let s = reduce_along(&x, args.get(1), |acc, y| acc + y, 0.0)?;
            // divide by count along dim
            let cnt = if args.get(1).is_some() {
                let d = args[1].as_array().to_scalar() as usize;
                x.shape.get(d - 1).copied().unwrap_or(1) as f64
            } else {
                let d = x.shape.iter().enumerate().find(|(_, &s)| s != 1).map(|(i, _)| i).unwrap_or(0);
                x.shape[d] as f64
            };
            Ok(Value::Array(unary_elementwise(&s.as_array().clone(), |c| c / cnt)))
        }
        "min" => minmax(&a(0)?, args.get(1), false),
        "max" => minmax(&a(0)?, args.get(1), true),
        "any" => logical_reduce(&a(0)?, args.get(1), false, |acc, x| acc || x),
        "all" => logical_reduce(&a(0)?, args.get(1), true, |acc, x| acc && x),
        "abs" => elem1!(|c| Complex64::new(c.norm(), 0.0)),
        "sin" => elem1!(|c| c.sin()),
        "cos" => elem1!(|c| c.cos()),
        "tan" => elem1!(|c| c.tan()),
        "asin" => elem1!(|c| c.asin()),
        "acos" => elem1!(|c| c.acos()),
        "atan" => elem1!(|c| c.atan()),
        "atan2" => elem2!(|y, x| Complex64::new(y.re.atan2(x.re), 0.0)),
        "sinh" => elem1!(|c| c.sinh()),
        "cosh" => elem1!(|c| c.cosh()),
        "tanh" => elem1!(|c| c.tanh()),
        "asinh" => elem1!(|c| c.asinh()),
        "acosh" => elem1!(|c| c.acosh()),
        "atanh" => elem1!(|c| c.atanh()),
        "exp" => elem1!(|c| c.exp()),
        "log" => elem1!(|c| c.ln()),
        "log10" => elem1!(|c| c.ln() / std::f64::consts::LN_10),
        "log2" => elem1!(|c| c.ln() / std::f64::consts::LN_2),
        "sqrt" => elem1!(|c| c.sqrt()),
        "floor" => elem1!(|c| Complex64::new(c.re.floor(), c.im.floor())),
        "ceil" => elem1!(|c| Complex64::new(c.re.ceil(), c.im.ceil())),
        "round" => elem1!(|c| Complex64::new(round_half_away(c.re), round_half_away(c.im))),
        "fix" => elem1!(|c| Complex64::new(c.re.trunc(), c.im.trunc())),
        "sign" => elem1!(|c| Complex64::new(if c.re == 0.0 && c.im == 0.0 { 0.0 } else if c.im == 0.0 { c.re.signum() } else { (c / c.norm()).re }, 0.0)),
        "real" => elem1!(|c| Complex64::new(c.re, 0.0)),
        "imag" => elem1!(|c| Complex64::new(c.im, 0.0)),
        "conj" => elem1!(|c| c.conj()),
        "angle" => elem1!(|c| Complex64::new(c.im.atan2(c.re), 0.0)),
        "logical" => {
            let x = a(0)?;
            let v: Vec<bool> = x.iter_complex().iter().map(|c| c.norm() != 0.0 && !c.re.is_nan()).collect();
            let mut out = MxArray::new_logical(&x.shape);
            if let MxData::Logical(d) = &mut out.data {
                for (k, &b) in v.iter().enumerate() { d[k] = b; }
            }
            Ok(Value::Array(out))
        }
        "mod" => elem2!(|x, y| Complex64::new(mod_f(x.re, y.re), 0.0)),
        "rem" => elem2!(|x, y| Complex64::new(rem_f(x.re, y.re), 0.0)),
        "transpose" => Ok(Value::Array(a(0)?.transpose())),
        "ctranspose" => {
            let t = a(0)?.transpose();
            let data: Vec<Complex64> = t.iter_complex().iter().map(|c| c.conj()).collect();
            Ok(Value::Array(MxArray::from_complex_colmajor(data, &t.shape)))
        }
        "inv" => Ok(Value::Array(linalg::inv(&a(0)?).map_err(conv)?)),
        "det" => Ok(Value::Array(MxArray::scalar_complex(linalg::det(&a(0)?).map_err(conv)?))),
        "rank" => {
            let tol = if n >= 2 { a(1)?.to_scalar() } else { 0.0 };
            Ok(Value::Array(MxArray::scalar_double(linalg::rank(&a(0)?, tol) as f64)))
        }
        "trace" => Ok(Value::Array(MxArray::scalar_complex(linalg::trace(&a(0)?)))),
        "norm" => {
            let ord = if n >= 2 { arg_str(args[1].as_array()) } else { "2".into() };
            Ok(Value::Array(MxArray::scalar_double(linalg::norm(&a(0)?, &ord).map_err(conv)?)))
        }
        "lu" => {
            let (l, u, piv) = linalg::lu(&a(0)?).map_err(conv)?;
            let m = l.shape[0];
            let nc = l.shape.get(1).copied().unwrap_or(1);
            // Permutation matrix P with P*A = L*U (P[i, piv[i]] = 1).
            let mut p = MxArray::new_double(&[m, m]);
            for i in 0..m { p.set_double(&[i, piv[i]], 1.0); }
            match engine.nargout {
                0 | 1 => Ok(Value::Array(u)), // single output: upper factor
                2 => {
                    // [L, U] = lu(A): fold the permutation into L so that L*U = A.
                    let mut lp = MxArray::new_complex(&[m, nc]);
                    for i in 0..m {
                        for j in 0..nc { lp.set_complex(&[piv[i], j], l.get_complex(&[i, j])); }
                    }
                    Ok(Value::Multi(vec![Value::Array(lp), Value::Array(u)]))
                }
                _ => Ok(Value::Multi(vec![Value::Array(l), Value::Array(u), Value::Array(p)])),
            }
        }
        "qr" => {
            let (qh, r) = linalg::qr_complex(&a(0)?).map_err(conv)?;
            if engine.nargout >= 2 {
                // qr_complex accumulates the Householder reflectors as Q^H; the
                // orthogonal factor Q (with Q*R = A) is its conjugate transpose.
                let qt = qh.transpose();
                let q = MxArray::from_complex_colmajor(
                    qt.iter_complex().iter().map(|c| c.conj()).collect(),
                    &qt.shape,
                );
                Ok(Value::Multi(vec![Value::Array(q), Value::Array(r)]))
            } else {
                Ok(Value::Array(r))
            }
        }
        "chol" => Ok(Value::Array(linalg::chol(&a(0)?).map_err(conv)?)),
        "eig" => {
            let (v, d) = linalg::eig(&a(0)?).map_err(conv)?;
            if engine.nargout >= 2 {
                // [V, D] = eig(A): eigenvectors and diagonal eigenvalue matrix.
                Ok(Value::Multi(vec![Value::Array(v), Value::Array(d)]))
            } else {
                // single output: column vector of eigenvalues (diagonal of D)
                let n = d.shape[0];
                let evals: Vec<Complex64> = (0..n).map(|i| d.get_complex(&[i, i])).collect();
                Ok(Value::Array(MxArray::from_complex_colmajor(evals, &[n, 1])))
            }
        }
        "svd" => {
            let (sv, u, vt) = linalg::svd(&a(0)?).map_err(conv)?;
            let ns = sv.len().max(1);
            if engine.nargout >= 2 {
                // [U, S, V] = svd(A): S is the diagonal matrix of singular values.
                let x = a(0)?;
                let m = x.shape[0];
                let nc = x.shape.get(1).copied().unwrap_or(1);
                let mut s = MxArray::new_double(&[m, nc]);
                for (i, &val) in sv.iter().enumerate() {
                    if i < m && i < nc { s.set_double(&[i, i], val); }
                }
                Ok(Value::Multi(vec![Value::Array(u), Value::Array(s), Value::Array(vt)]))
            } else {
                Ok(Value::Array(MxArray::from_double_colmajor(sv, &[ns, 1])))
            }
        }
        "pinv" => {
            let (s, u, v) = linalg::svd(&a(0)?).map_err(conv)?;
            let m = a(0)?.shape[0];
            let nn = a(0)?.shape.get(1).copied().unwrap_or(1);
            let mut inv = vec![vec![Complex64::new(0.0, 0.0); m]; nn];
            for j in 0..nn {
                let sj = if s[j] > 1e-12 { s[j] } else { 1.0 };
                for i in 0..m { inv[j][i] = u.get_complex(&[i, j]) / sj * v.get_complex(&[j, i]).conj(); }
            }
            Ok(Value::Array(MxArray::from_complex_colmajor(flatten(&inv), &[nn, m])))
        }
        "reshape" => {
            let x = a(0)?;
            let shape = parse_size(&args[1..]);
            Ok(Value::Array(x.reshape(&shape)))
        }
        "diag" => Ok(Value::Array(diag(&a(0)?))),
        "repmat" => {
            let x = a(0)?;
            let reps = &args[1..];
            Ok(Value::Array(repmat(&x, reps)))
        }
        "kron" => {
            let x = a(0)?;
            let y = a(1)?;
            Ok(Value::Array(kron(&x, &y)))
        }
        "fliplr" => Ok(Value::Array(flip_dim(&a(0)?, 1))),
        "flipud" => Ok(Value::Array(flip_dim(&a(0)?, 0))),
        "flip" => {
            let x = a(0)?;
            let dim = if n >= 2 {
                a(1)?.to_scalar() as usize - 1
            } else {
                // default: first non-singleton dimension (MATLAB semantics)
                x.shape.iter().position(|&s| s > 1).unwrap_or(0)
            };
            Ok(Value::Array(flip_dim(&x, dim)))
        }
        "sort" => {
            let x = a(0)?;
            let mut v = x.iter_complex();
            v.sort_by(|a, b| a.re.partial_cmp(&b.re).unwrap());
            Ok(Value::Array(MxArray::from_complex_colmajor(v, &x.shape)))
        }
        "unique" => {
            let x = a(0)?;
            let mut v: Vec<f64> = x.iter_complex().iter().map(|c| c.re).collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v.dedup();
            let n = v.len();
            Ok(Value::Array(MxArray::from_double_colmajor(v, &[1, n])))
        }
        "isempty" => {
            let shape = value_shape(args.first().ok_or_else(|| EngineError::Msg("isempty: not enough input arguments".into()))?);
            Ok(Value::Array(logical_scalar(shape.iter().product::<usize>() == 0)))
        }
        "isscalar" => Ok(Value::Array(logical_scalar(a(0)?.is_scalar()))),
        "isvector" => Ok(Value::Array(logical_scalar(a(0)?.is_vector()))),
        "isrow" => Ok(Value::Array(logical_scalar({
            let x = a(0)?;
            x.ndims() == 2 && x.shape[0] == 1
        }))),
        "iscolumn" => Ok(Value::Array(logical_scalar({
            let x = a(0)?;
            x.ndims() == 2 && x.shape[1] == 1
        }))),
        "ismatrix" => Ok(Value::Array(logical_scalar(a(0)?.ndims() == 2))),
        "disp" => {
            println!("{}", format_value(&args[0]));
            Ok(Value::Array(MxArray::new_double(&[0, 0])))
        }
        "nargin" => {
            if n >= 1 {
                let name = match &args[0] { Value::String(s) => s, _ => return Ok(Value::Array(MxArray::scalar_double(0.0))), };
                if let Some((inp, _, _)) = engine.functions.get(name) { Ok(Value::Array(MxArray::scalar_double(inp.len() as f64))) }
                else { Ok(Value::Array(MxArray::scalar_double(-1.0))) }
            } else {
                Ok(Value::Array(MxArray::scalar_double(-1.0)))
            }
        }
        "nargout" => {
            if n >= 1 {
                let name = match &args[0] { Value::String(s) => s, _ => return Ok(Value::Array(MxArray::scalar_double(0.0))), };
                if let Some((_, out, _)) = engine.functions.get(name) { Ok(Value::Array(MxArray::scalar_double(out.len() as f64))) }
                else { Ok(Value::Array(MxArray::scalar_double(-1.0))) }
            } else {
                Ok(Value::Array(MxArray::scalar_double(-1.0)))
            }
        }
        "run" => {
            let filename = match &args[0] { Value::String(s) => s.clone(), _ => return Err(EngineError::Msg("run: argument must be a string".into())) };
            let path = if filename.ends_with(".m") { filename } else { format!("{}.m", filename) };
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => return Err(EngineError::Msg(format!("run: file '{}' not found", path))),
            };
            let mut p = crate::parser::Parser::new(&content).map_err(EngineError::Msg)?;
            let stmts = p.parse_program().map_err(EngineError::Msg)?;
            let (_, disp) = engine.exec_block(&stmts)?;
            Ok(disp.unwrap_or(Value::Array(MxArray::new_double(&[0, 0]))))
        }
        "error" => {
            let msg = format_value(&args[0]);
            Err(EngineError::Msg(msg))
        }
        "assert" => {
            if n == 0 {
                return Err(EngineError::Msg("assert: not enough input arguments".into()));
            }
            if engine.is_true(&args[0]) {
                return Ok(Value::Array(MxArray::new_double(&[0, 0])));
            }
            // Failure message: assert(cond), assert(cond, msg), or
            // assert(cond, fmt, ...) with sprintf-style formatting.
            let msg = match n {
                1 => "Assertion failed.".to_string(),
                2 => format_value(&args[1]),
                _ => match call("sprintf", &args[1..], engine) {
                    Ok(Value::String(s)) => s,
                    _ => "Assertion failed.".to_string(),
                },
            };
            Err(EngineError::Msg(msg))
        }
        "warning" => {
            if n > 0 {
                let msg = format_value(&args[0]);
                eprintln!("Warning: {}", msg);
            }
            Ok(Value::Array(MxArray::new_double(&[0, 0])))
        }
        "who" => {
            // Reserved constants live in the workspace but are not user variables,
            // so `who` omits them (matching MATLAB) and lists names alphabetically.
            let mut names: Vec<String> = engine.workspace.keys()
                .filter(|k| !crate::eval::is_reserved_constant(k))
                .cloned()
                .collect();
            names.sort();
            if names.is_empty() {
                println!("Your variables are:");
            } else {
                println!("Your variables are:");
                for nm in &names { println!("  {}", nm); }
            }
            Ok(Value::Array(MxArray::new_double(&[0, 0])))
        }
        "clear" => {
            if n == 0 { engine.workspace.clear(); engine.reset_constants(); }
            else {
                for arg in args {
                    if let Value::String(s) = arg { engine.workspace.remove(s); }
                }
            }
            Ok(Value::Array(MxArray::new_double(&[0, 0])))
        }
        "exist" => {
            let name = if let Value::String(s) = &args[0] { s } else { return Ok(Value::Array(MxArray::scalar_double(0.0))); };
            Ok(Value::Array(MxArray::scalar_double(if engine.workspace.contains_key(name) { 1.0 } else { 0.0 })))
        }
        "format" => {
            if n >= 1 { if let Value::String(s) = &args[0] { engine.format = s.clone(); } }
            Ok(Value::Array(MxArray::new_double(&[0, 0])))
        }
        "struct" => {
            let mut map = std::collections::HashMap::new();
            for i in (0..n).step_by(2) {
                let field = match &args[i] {
                    Value::String(s) => s.clone(),
                    _ => return Err(EngineError::Msg("struct field names must be strings".into())),
                };
                let val = if i + 1 < n { args[i + 1].clone() } else { Value::Array(MxArray::new_double(&[0, 0])) };
                map.insert(field, val);
            }
            Ok(Value::Struct(map))
        }
        "isfield" => {
            let contains = match &args[0] {
                Value::Struct(m) => {
                    let f = match &args[1] {
                        Value::String(s) => s,
                        _ => return Err(EngineError::Msg("isfield: second argument must be a string".into())),
                    };
                    m.contains_key(f)
                }
                _ => false,
            };
            Ok(Value::Array(logical_scalar(contains)))
        }
        "fieldnames" => {
            let m = match &args[0] {
                Value::Struct(m) => m,
                _ => return Err(EngineError::Msg("fieldnames: argument must be a struct".into())),
            };
            Ok(Value::Array(MxArray::new_double(&[1, m.len().max(1)])))
        }
        "find" => {
            let x = a(0)?;
            let mut idx = Vec::new();
            for k in 0..x.numel() {
                if x.get_complex(&linear_of(&x, k)).norm() != 0.0 {
                    idx.push((k + 1) as f64);
                }
            }
            let n = idx.len();
            // find of a row vector yields a row vector; otherwise a column (MATLAB).
            let is_row = x.ndims() == 2 && x.shape[0] == 1;
            let shape = if is_row { [1, n] } else { [n, 1] };
            Ok(Value::Array(MxArray::from_double_colmajor(idx, &shape)))
        }
        "strcmp" => {
            let s1 = match &args[0] { Value::String(s) => s, _ => "unimplemented" };
            let s2 = match &args[1] { Value::String(s) => s, _ => "unimplemented" };
            Ok(Value::Array(logical_scalar(s1 == s2)))
        }
        "strcmpi" => {
            let s1 = match &args[0] { Value::String(s) => s, _ => "unimplemented" };
            let s2 = match &args[1] { Value::String(s) => s, _ => "unimplemented" };
            Ok(Value::Array(logical_scalar(s1.to_lowercase() == s2.to_lowercase())))
        }
        "upper" => {
            let s = match &args[0] {
                Value::String(s) => s.to_uppercase(),
                Value::Array(a) if a.is_scalar() => format!("{}", a.to_scalar()).to_uppercase(),
                _ => return Err(EngineError::Msg("upper: argument must be a string".into())),
            };
            Ok(Value::String(s))
        }
        "lower" => {
            let s = match &args[0] {
                Value::String(s) => s.to_lowercase(),
                Value::Array(a) if a.is_scalar() => format!("{}", a.to_scalar()).to_lowercase(),
                _ => return Err(EngineError::Msg("lower: argument must be a string".into())),
            };
            Ok(Value::String(s))
        }
        "num2str" => {
            if let Value::Array(a) = &args[0] {
                let mut parts = Vec::new();
                for k in 0..a.numel() {
                    parts.push(format!("{}", a.get_double(&linear_of(a, k))));
                }
                Ok(Value::String(parts.join(" ")))
            } else {
                Ok(Value::String(format!("{:?}", args[0])))
            }
        }
        "int2str" => {
            let x = a(0)?;
            let mut parts = Vec::new();
            for k in 0..x.numel() {
                parts.push(format!("{}", x.get_double(&linear_of(&x, k)) as i64));
            }
            Ok(Value::String(parts.join(" ")))
        }
        "str2double" => {
            let s = match &args[0] {
                Value::String(s) => s,
                Value::Array(a) if a.numel() == 1 => return Ok(Value::Array(a.clone())),
                _ => return Err(EngineError::Msg("str2double: argument must be a string".into())),
            };
            match s.trim().parse::<f64>() {
                Ok(v) => Ok(Value::Array(MxArray::scalar_double(v))),
                Err(_) => Ok(Value::Array(MxArray::scalar_double(f64::NAN))),
            }
        }
        "contains" => {
            let s = match &args[0] { Value::String(s) => s, _ => return Err(EngineError::Msg("contains: first argument must be a string".into())) };
            let pat = match &args[1] { Value::String(p) => p, _ => return Err(EngineError::Msg("contains: second argument must be a string".into())) };
            Ok(Value::Array(logical_scalar(s.contains(pat.as_str()))))
        }
        "startsWith" | "startswith" => {
            let s = match &args[0] { Value::String(s) => s, _ => return Err(EngineError::Msg("startsWith: first argument must be a string".into())) };
            let pat = match &args[1] { Value::String(p) => p, _ => return Err(EngineError::Msg("startsWith: second argument must be a string".into())) };
            Ok(Value::Array(logical_scalar(s.starts_with(pat.as_str()))))
        }
        "endsWith" | "endswith" => {
            let s = match &args[0] { Value::String(s) => s, _ => return Err(EngineError::Msg("endsWith: first argument must be a string".into())) };
            let pat = match &args[1] { Value::String(p) => p, _ => return Err(EngineError::Msg("endsWith: second argument must be a string".into())) };
            Ok(Value::Array(logical_scalar(s.ends_with(pat.as_str()))))
        }
        "strtrim" => {
            let s = match &args[0] { Value::String(s) => s, _ => return Err(EngineError::Msg("strtrim: argument must be a string".into())) };
            Ok(Value::String(s.trim().to_string()))
        }
        "deblank" => {
            let s = match &args[0] { Value::String(s) => s, _ => return Err(EngineError::Msg("deblank: argument must be a string".into())) };
            Ok(Value::String(s.trim_end().to_string()))
        }
        "strsplit" => {
            let s = match &args[0] { Value::String(s) => s, _ => return Err(EngineError::Msg("strsplit: first argument must be a string".into())) };
            let delim = if args.len() > 1 { match &args[1] { Value::String(d) => d.clone(), _ => return Err(EngineError::Msg("strsplit: delimiter must be a string".into())) } } else { " ".into() };
            let parts: Vec<Value> = s.split(&delim).map(|p| Value::String(p.to_string())).collect();
            Ok(Value::Cell(vec![parts]))
        }
        "strjoin" => {
            let parts = match &args[0] { Value::Cell(rows) if rows.len() == 1 => rows[0].clone(), _ => return Err(EngineError::Msg("strjoin: first argument must be a cell array of strings".into())) };
            let delim = if args.len() > 1 { match &args[1] { Value::String(d) => d.clone(), _ => return Err(EngineError::Msg("strjoin: delimiter must be a string".into())) } } else { "".into() };
            let strings: Vec<&str> = parts.iter().map(|v| match v { Value::String(s) => s.as_str(), _ => "" }).collect();
            Ok(Value::String(strings.join(&delim)))
        }
        "num2cell" => {
            let x = a(0)?;
            let mut rows = Vec::new();
            for i in 0..x.shape[0] {
                let mut row = Vec::new();
                for j in 0..x.shape.get(1).copied().unwrap_or(1) {
                    row.push(Value::Array(MxArray::scalar_double(x.get_double(&[i, j]))));
                }
                rows.push(row);
            }
            Ok(Value::Cell(rows))
        }
        "sprintf" => {
            let fmt = match &args[0] { Value::String(s) => s, _ => return Err(EngineError::Msg("sprintf: first argument must be a format string".into())) };
            let mut result = String::new();
            let mut fmt_iter = fmt.chars().peekable();
            let mut arg_idx = 1usize;
            while let Some(ch) = fmt_iter.next() {
                if ch == '%' && fmt_iter.peek() != Some(&'%') {
                    let spec = fmt_iter.next().unwrap_or(' ');
                    let val = if arg_idx < args.len() {
                        match &args[arg_idx] {
                            Value::Array(a) if a.is_scalar() => format!("{}", a.to_scalar()),
                            Value::String(s) => s.clone(),
                            _ => "?".into(),
                        }
                    } else { "?".into() };
                    match spec {
                        'd' | 'i' => {
                            if arg_idx < args.len() {
                                let n = match &args[arg_idx] {
                                    Value::Array(a) if a.is_scalar() => a.to_scalar() as i64,
                                    _ => 0,
                                };
                                result.push_str(&format!("{}", n));
                            }
                        }
                        'f' | 'g' | 'e' => result.push_str(&val),
                        's' => result.push_str(&val),
                        _ => { result.push('%'); result.push(spec); }
                    }
                    arg_idx += 1;
                } else if ch == '%' && fmt_iter.peek() == Some(&'%') {
                    result.push('%');
                    fmt_iter.next();
                } else {
                    result.push(ch);
                }
            }
            Ok(Value::String(result))
        }
        "cumsum" => {
            let x = a(0)?;
            let dim = match args.get(1) {
                Some(v) => v.as_array().to_scalar() as usize,
                None => x.shape.iter().enumerate().find(|(_, &s)| s != 1).map(|(i, _)| i + 1).unwrap_or(1),
            };
            let nd = x.ndims();
            if dim > nd || dim < 1 {
                return Err(EngineError::Msg("cumsum: dimension out of range".into()));
            }
            let d = dim - 1;
            let mut out = x.clone();
            let len = x.shape[d];
            if len <= 1 { return Ok(Value::Array(out)); }
            let total = x.numel();
            for lin in 0..total {
                let idx = linear_of(&x, lin);
                if idx[d] > 0 {
                    let mut prev = idx.clone();
                    prev[d] = idx[d] - 1;
                    let val = x.get_complex(&idx) + out.get_complex(&prev);
                    out.set_complex(&idx, val);
                }
            }
            Ok(Value::Array(out))
        }
        "cumprod" => {
            let x = a(0)?;
            let dim = match args.get(1) {
                Some(v) => v.as_array().to_scalar() as usize,
                None => x.shape.iter().enumerate().find(|(_, &s)| s != 1).map(|(i, _)| i + 1).unwrap_or(1),
            };
            let nd = x.ndims();
            if dim > nd || dim < 1 {
                return Err(EngineError::Msg("cumprod: dimension out of range".into()));
            }
            let d = dim - 1;
            let mut out = x.clone();
            let len = x.shape[d];
            if len <= 1 { return Ok(Value::Array(out)); }
            let total = x.numel();
            for lin in 0..total {
                let idx = linear_of(&x, lin);
                if idx[d] > 0 {
                    let mut prev = idx.clone();
                    prev[d] = idx[d] - 1;
                    let val = x.get_complex(&idx) * out.get_complex(&prev);
                    out.set_complex(&idx, val);
                }
            }
            Ok(Value::Array(out))
        }
        "diff" => {
            let x = a(0)?;
            let dim = match args.get(1) {
                Some(v) => v.as_array().to_scalar() as usize,
                None => x.shape.iter().enumerate().find(|(_, &s)| s != 1).map(|(i, _)| i + 1).unwrap_or(1),
            };
            let nd = x.ndims();
            if dim > nd || dim < 1 {
                return Err(EngineError::Msg("diff: dimension out of range".into()));
            }
            let d = dim - 1;
            let len = x.shape[d];
            if len <= 1 {
                return Ok(Value::Array(MxArray::new_double(&[0, 0])));
            }
            let mut new_shape = x.shape.clone();
            new_shape[d] = len - 1;
            let mut out = MxArray::new_complex(&new_shape);
            for lin in 0..x.numel() {
                let idx = linear_of(&x, lin);
                if idx[d] > 0 {
                    let mut prev = idx.clone();
                    prev[d] = idx[d] - 1;
                    let mut out_idx = idx.clone();
                    out_idx[d] = idx[d] - 1;
                    out.set_complex(&out_idx, x.get_complex(&idx) - x.get_complex(&prev));
                }
            }
            Ok(Value::Array(out))
        }
        "deal" => {
            if args.len() == 1 {
                Ok(args[0].clone())
            } else {
                Ok(Value::Multi(args.to_vec()))
            }
        }
        "cellfun" | "arrayfun" => {
            let func = args.first().ok_or_else(|| EngineError::Msg(format!("{}: not enough input arguments", name)))?.clone();
            // Split trailing name-value options (UniformOutput / ErrorHandler)
            // from the data arguments.
            let mut uniform = true;
            let mut data: Vec<&Value> = Vec::new();
            let mut i = 1;
            while i < args.len() {
                if let Value::String(s) = &args[i] {
                    if s.eq_ignore_ascii_case("UniformOutput") && i + 1 < args.len() {
                        uniform = engine.is_true(&args[i + 1]); i += 2; continue;
                    }
                    if s.eq_ignore_ascii_case("ErrorHandler") && i + 1 < args.len() {
                        i += 2; continue;
                    }
                }
                data.push(&args[i]);
                i += 1;
            }
            if data.is_empty() {
                return Err(EngineError::Msg(format!("{}: not enough input arguments", name)));
            }
            // Collect each data argument as a flat column-major list of element
            // Values plus its shape. cellfun unwraps cell contents; arrayfun walks
            // numeric elements.
            let elems: Vec<(Vec<Value>, [usize; 2])> = data.iter()
                .map(|v| flatten_fun_arg(name, v))
                .collect::<Result<_, _>>()?;
            let shape = elems[0].1;
            let count = elems[0].0.len();
            for (_, sh) in &elems {
                if sh[0] * sh[1] != count {
                    return Err(EngineError::Msg(format!("{}: all inputs must have the same size", name)));
                }
            }
            let mut results = Vec::with_capacity(count);
            for k in 0..count {
                let call_args: Vec<Value> = elems.iter().map(|(vals, _)| vals[k].clone()).collect();
                results.push(engine.eval_func_handle(func.clone(), call_args)?);
            }
            if uniform {
                // Each result must be a scalar; collect into an array of `shape`.
                let mut buf = Vec::with_capacity(count);
                for r in &results {
                    let a = match r {
                        Value::Array(a) => a.clone(),
                        Value::String(s) => crate::eval::string_to_char_array(s),
                        _ => return Err(EngineError::Msg(format!(
                            "{}: non-uniform output; set 'UniformOutput' to false", name))),
                    };
                    if a.numel() != 1 {
                        return Err(EngineError::Msg(format!(
                            "{}: non-uniform output; set 'UniformOutput' to false", name)));
                    }
                    buf.push(a.iter_complex()[0]);
                }
                Ok(Value::Array(MxArray::from_complex_colmajor(buf, &[shape[0], shape[1]])))
            } else {
                // Reassemble a cell of the original shape (column-major fill).
                let (m, n) = (shape[0], shape[1]);
                let mut out_rows = vec![vec![Value::Array(MxArray::new_double(&[0, 0])); n]; m];
                for k in 0..count {
                    out_rows[k % m.max(1)][k / m.max(1)] = results[k].clone();
                }
                Ok(Value::Cell(out_rows))
            }
        }
        "ischar" => {
            Ok(Value::Array(logical_scalar(matches!(&args[0], Value::String(_)))))
        }
        "islogical" => {
            let a = a(0)?;
            Ok(Value::Array(logical_scalar(a.is_logical_like())))
        }
        "isnumeric" => {
            Ok(Value::Array(logical_scalar(matches!(&args[0], Value::Array(_)))))
        }
        "isreal" => {
            let a = a(0)?;
            Ok(Value::Array(logical_scalar(a.iter_complex().iter().all(|c| c.im.abs() < 1e-15))))
        }
        "help" => {
            println!("MATBOX builtin: {}", name);
            Ok(Value::Array(MxArray::new_double(&[0, 0])))
        }
        "isequal" | "isequaln" => {
            if args.is_empty() { return Ok(Value::Array(logical_scalar(true))); }
            let first = &args[0];
            let all_eq = args[1..].iter().all(|a| values_equal(first, a));
            Ok(Value::Array(logical_scalar(all_eq)))
        }
        "cell2mat" => {
            let cell = args.first().ok_or_else(|| EngineError::Msg("cell2mat: not enough input arguments".into()))?;
            let rows = match cell {
                Value::Cell(r) => r,
                _ => return Err(EngineError::Msg("cell2mat: input must be a cell array".into())),
            };
            if rows.is_empty() || rows[0].is_empty() {
                return Ok(Value::Array(MxArray::new_double(&[0, 0])));
            }
            let mut all_rows = Vec::new();
            for row in rows {
                for val in row {
                    if let Value::Array(a) = val {
                        all_rows.push(a.clone());
                    }
                }
            }
            if all_rows.is_empty() { return Ok(Value::Array(MxArray::new_double(&[0, 0]))); }
            let total_rows: usize = all_rows.iter().map(|a| a.shape[0]).sum();
            let total_cols: usize = all_rows.iter().map(|a| a.shape.get(1).copied().unwrap_or(1)).sum();
            let mut out = MxArray::new_complex(&[total_rows, total_cols]);
            let mut col_off = 0;
            for a in &all_rows {
                let m = a.shape[0];
                let n = a.shape.get(1).copied().unwrap_or(1);
                for i in 0..m {
                    for j in 0..n {
                        out.set_complex(&[i, col_off + j], a.get_complex(&[i, j]));
                    }
                }
                col_off += n;
            }
            Ok(Value::Array(out))
        }
        "feval" => {
            if args.is_empty() { return Err(EngineError::Msg("feval: not enough input arguments".into())); }
            let func_name = match &args[0] {
                Value::String(s) => s.clone(),
                Value::FuncRef(s) => s.clone(),
                _ => return Err(EngineError::Msg("feval: first argument must be a function name or handle".into())),
            };
            let call_args: Vec<Value> = args[1..].to_vec();
            engine.dispatch_call(&func_name, call_args)
        }
        "cat" => {
            let dim = a(0)?.to_scalar() as usize;
            if args.len() < 2 { return Err(EngineError::Msg("cat: at least 2 arguments required".into())); }
            let arrays: Vec<MxArray> = args[1..].iter().map(|v| v.as_array().clone()).collect();
            if arrays.is_empty() { return Ok(Value::Array(MxArray::new_double(&[0, 0]))); }
            let max_nd = arrays.iter().map(|a| a.ndims()).max().unwrap_or(2);
            let d = if dim == 0 { 0 } else { dim - 1 };
            let mut out_shape: Vec<usize> = arrays[0].shape.clone();
            while out_shape.len() < max_nd { out_shape.push(1); }
            for a in &arrays[1..] {
                let mut sh = a.shape.clone();
                while sh.len() < max_nd { sh.push(1); }
                for i in 0..max_nd {
                    if i == d { out_shape[i] += sh[i]; }
                    else if out_shape[i] != sh[i] {
                        return Err(EngineError::Msg("cat: dimension mismatch".into()));
                    }
                }
            }
            let mut out = MxArray::new_complex(&out_shape);
            let mut offset = 0usize;
            for a in &arrays {
                let len = a.numel();
                for i in 0..len {
                    let lin = offset + i;
                    let mut idx = vec![0usize; max_nd];
                    let mut rem = lin;
                    for k in 0..max_nd {
                        idx[k] = rem % out_shape[k];
                        rem /= out_shape[k];
                    }
                    out.set_complex(&idx, a.iter_complex()[i]);
                }
                offset += len;
            }
            Ok(Value::Array(out))
        }
        "tic" => {
            let now = std::time::Instant::now();
            engine.workspace.insert("__tic__".into(), Value::Array(MxArray::scalar_double(now.elapsed().as_secs_f64().recip())));
            Ok(Value::Array(MxArray::new_double(&[0, 0])))
        }
        "toc" => {
            Ok(Value::Array(MxArray::new_double(&[0, 0])))
        }
        "input" => {
            if n > 0 {
                let prompt = format_value(&args[0]);
                print!("{}", prompt);
                std::io::stdout().flush().ok();
            }
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).ok();
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() { return Ok(Value::Array(MxArray::new_double(&[0, 0]))); }
            // Try to parse as expression
            if let Ok(mut p) = crate::Parser::new(&trimmed) {
                if let Ok(stmts) = p.parse_program() {
                    if let Ok((_, Some(v))) = engine.exec_block(&stmts) {
                        return Ok(v);
                    }
                }
            }
            Ok(Value::String(trimmed))
        }
        "clc" => {
            print!("\x1B[2J\x1B[1;1H");
            std::io::stdout().flush().ok();
            Ok(Value::Array(MxArray::new_double(&[0, 0])))
        }
        "pause" => {
            if n > 0 {
                let secs = a(0)?.to_scalar();
                if secs >= 0.0 {
                    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
                }
            } else {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            Ok(Value::Array(MxArray::new_double(&[0, 0])))
        }
        "isinf" => {
            let a = a(0)?;
            let mut out = MxArray::new_logical(&a.shape);
            for i in 0..a.numel() {
                if let MxData::Logical(d) = &mut out.data { d[i] = a.iter_complex()[i].im.is_infinite() || a.iter_complex()[i].re.is_infinite(); }
            }
            Ok(Value::Array(out))
        }
        "isnan" => {
            let a = a(0)?;
            let mut out = MxArray::new_logical(&a.shape);
            for i in 0..a.numel() {
                if let MxData::Logical(d) = &mut out.data { d[i] = a.iter_complex()[i].im.is_nan() || a.iter_complex()[i].re.is_nan(); }
            }
            Ok(Value::Array(out))
        }
        "isfinite" => {
            let a = a(0)?;
            let mut out = MxArray::new_logical(&a.shape);
            for i in 0..a.numel() {
                if let MxData::Logical(d) = &mut out.data { d[i] = a.iter_complex()[i].im.is_finite() && a.iter_complex()[i].re.is_finite(); }
            }
            Ok(Value::Array(out))
        }
        "iscell" => {
            Ok(Value::Array(logical_scalar(matches!(&args[0], Value::Cell(_)))))
        }
        "isstruct" => {
            Ok(Value::Array(logical_scalar(matches!(&args[0], Value::Struct(_)))))
        }
        "rmfield" => {
            let s = match &args[0] {
                Value::Struct(m) => m.clone(),
                _ => return Err(EngineError::Msg("rmfield: first argument must be a struct".into())),
            };
            let field = match &args.get(1) {
                Some(Value::String(f)) => f.clone(),
                _ => return Err(EngineError::Msg("rmfield: second argument must be a field name".into())),
            };
            let mut out = s;
            out.remove(&field);
            Ok(Value::Struct(out))
        }
        "orderfields" => {
            let s = match &args[0] {
                Value::Struct(m) => m.clone(),
                _ => return Err(EngineError::Msg("orderfields: argument must be a struct".into())),
            };
            let mut keys: Vec<String> = s.keys().cloned().collect();
            keys.sort();
            let mut out = std::collections::HashMap::new();
            for k in keys {
                if let Some(v) = s.get(&k) { out.insert(k, v.clone()); }
            }
            Ok(Value::Struct(out))
        }
        // ── String operations ──
        "strfind" => {
            let s = match args.first() { Some(Value::String(s)) => s.as_str(), _ => "" };
            let pat = match args.get(1) { Some(Value::String(p)) => p.as_str(), _ => "" };
            if pat.is_empty() { return Ok(Value::Array(MxArray::scalar_double(1.0))); }
            let mut indices = Vec::new();
            let mut start = 0;
            while let Some(pos) = s[start..].find(pat) {
                indices.push((start + pos + 1) as f64);
                start += pos + 1;
            }
            let n = indices.len();
            let out = if n == 0 { MxArray::new_double(&[1, 0]) } else { MxArray::from_double_colmajor(indices, &[1, n]) };
            Ok(Value::Array(out))
        }
        "strrep" => {
            let s = match args.first() { Some(Value::String(s)) => s.as_str(), _ => "" };
            let old = match args.get(1) { Some(Value::String(s)) => s.as_str(), _ => "" };
            let new = match args.get(2) { Some(Value::String(s)) => s.as_str(), _ => "" };
            Ok(Value::String(s.replace(old, new)))
        }
        "regexp" | "regexpi" => {
            let s = match args.first() { Some(Value::String(s)) => s.as_str(), _ => "" };
            let pat = match args.get(1) { Some(Value::String(s)) => s.as_str(), _ => "" };
            let case_insensitive = name == "regexpi";
            let re = if case_insensitive {
                regex::RegexBuilder::new(pat).case_insensitive(true).build()
            } else {
                regex::Regex::new(pat)
            };
            match re {
                Ok(re) => {
                    let mut starts = Vec::new();
                    let mut ends = Vec::new();
                    for m in re.find_iter(s) {
                        starts.push((m.start() + 1) as f64);
                        ends.push(m.end() as f64);
                    }
                    let n = starts.len();
                    if n == 0 {
                        Ok(Value::Cell(vec![
                            vec![Value::Array(MxArray::new_double(&[1, 0]))],
                            vec![Value::Array(MxArray::new_double(&[1, 0]))],
                        ]))
                    } else {
                        Ok(Value::Cell(vec![
                            vec![Value::Array(MxArray::from_double_colmajor(starts, &[1, n]))],
                            vec![Value::Array(MxArray::from_double_colmajor(ends, &[1, n]))],
                        ]))
                    }
                }
                Err(_) => Ok(Value::Cell(vec![
                    vec![Value::Array(MxArray::new_double(&[1, 0]))],
                    vec![Value::Array(MxArray::new_double(&[1, 0]))],
                ]))
            }
        }
        "regexprep" => {
            let s = match args.first() { Some(Value::String(st)) => st.clone(), _ => String::new() };
            let pat = match args.get(1) { Some(Value::String(st)) => st.clone(), _ => String::new() };
            let rep = match args.get(2) { Some(Value::String(st)) => st.clone(), _ => String::new() };
            match regex::Regex::new(&pat) {
                Ok(re) => Ok(Value::String(re.replace_all(&s, rep).to_string())),
                Err(_) => Ok(Value::String(s)),
            }
        }
        // ── Set operations ──
        "intersect" => {
            let x = a(0)?.to_double_vec();
            let y = a(1)?.to_double_vec();
            let mut common: Vec<f64> = x.iter().filter(|v| y.contains(v)).copied().collect();
            common.sort_by(|a, b| a.partial_cmp(b).unwrap());
            common.dedup();
            let n = common.len();
            Ok(Value::Array(MxArray::from_double_colmajor(common, &[1, n])))
        }
        "setdiff" => {
            let x = a(0)?.to_double_vec();
            let y = a(1)?.to_double_vec();
            let mut diff: Vec<f64> = x.iter().filter(|v| !y.contains(v)).copied().collect();
            diff.sort_by(|a, b| a.partial_cmp(b).unwrap());
            diff.dedup();
            let n = diff.len();
            Ok(Value::Array(MxArray::from_double_colmajor(diff, &[1, n])))
        }
        "setxor" => {
            let x = a(0)?.to_double_vec();
            let y = a(1)?.to_double_vec();
            let mut xor: Vec<f64> = x.iter().filter(|v| !y.contains(v)).chain(y.iter().filter(|v| !x.contains(v))).copied().collect();
            xor.sort_by(|a, b| a.partial_cmp(b).unwrap());
            xor.dedup();
            let n = xor.len();
            Ok(Value::Array(MxArray::from_double_colmajor(xor, &[1, n])))
        }
        "union" => {
            let x = a(0)?.to_double_vec();
            let y = a(1)?.to_double_vec();
            let mut u = x.clone();
            u.extend(y);
            u.sort_by(|a, b| a.partial_cmp(b).unwrap());
            u.dedup();
            let n = u.len();
            Ok(Value::Array(MxArray::from_double_colmajor(u, &[1, n])))
        }
        "ismember" => {
            let x = a(0)?;
            let y = a(1)?;
            let yv = y.to_double_vec();
            let result: Vec<f64> = x.to_double_vec().iter().map(|v| if yv.contains(v) { 1.0 } else { 0.0 }).collect();
            Ok(Value::Array(MxArray::from_double_colmajor(result, &x.shape)))
        }
        "issorted" => {
            let v = a(0)?.to_double_vec();
            let sorted = v.windows(2).all(|w| w[0] <= w[1]);
            Ok(Value::Array(MxArray::scalar_double(if sorted { 1.0 } else { 0.0 })))
        }
        // ── Statistics ──
        "median" => {
            let v = a(0)?.to_double_vec();
            let mut s = v.clone();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mid = s.len() / 2;
            let med = if s.len() % 2 == 0 { (s[mid - 1] + s[mid]) / 2.0 } else { s[mid] };
            Ok(Value::Array(MxArray::scalar_double(med)))
        }
        "std" => {
            let v = a(0)?.to_double_vec();
            let n = v.len() as f64;
            if n <= 1.0 { return Ok(Value::Array(MxArray::scalar_double(0.0))); }
            let mean = v.iter().sum::<f64>() / n;
            let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
            Ok(Value::Array(MxArray::scalar_double(var.sqrt())))
        }
        "var" => {
            let v = a(0)?.to_double_vec();
            let n = v.len() as f64;
            if n <= 1.0 { return Ok(Value::Array(MxArray::scalar_double(0.0))); }
            let mean = v.iter().sum::<f64>() / n;
            let variance = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
            Ok(Value::Array(MxArray::scalar_double(variance)))
        }
        "mode" => {
            let v = a(0)?.to_double_vec();
            if v.is_empty() { return Ok(Value::Array(MxArray::new_double(&[1, 0]))); }
            use std::collections::HashMap;
            let mut counts: HashMap<u64, usize> = HashMap::new();
            for &val in &v { *counts.entry(val.to_bits()).or_insert(0) += 1; }
            let max_count = counts.values().max().copied().unwrap_or(0);
            let mut modes: Vec<f64> = counts.iter().filter(|(_, &c)| c == max_count).map(|(k, _)| f64::from_bits(*k)).collect();
            modes.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let n = modes.len();
            Ok(Value::Array(MxArray::from_double_colmajor(modes, &[1, n])))
        }
        "corrcoef" => {
            let x = a(0)?.to_double_vec();
            let y = a(1)?.to_double_vec();
            let n = x.len().min(y.len()) as f64;
            if n <= 1.0 { return Ok(Value::Array(MxArray::new_double(&[2, 2]))); }
            let mx = x.iter().sum::<f64>() / n;
            let my = y.iter().sum::<f64>() / n;
            let mut sxx = 0.0; let mut syy = 0.0; let mut sxy = 0.0;
            for i in 0..x.len().min(y.len()) {
                let dx = x[i] - mx; let dy = y[i] - my;
                sxx += dx * dx; syy += dy * dy; sxy += dx * dy;
            }
            let r = if sxx * syy == 0.0 { 0.0 } else { sxy / (sxx * syy).sqrt() };
            Ok(Value::Array(MxArray::from_double_colmajor(vec![1.0, r, r, 1.0], &[2, 2])))
        }
        "cov" => {
            let x = a(0)?.to_double_vec();
            let y = a(1)?.to_double_vec();
            let n = x.len().min(y.len()) as f64;
            if n <= 1.0 { return Ok(Value::Array(MxArray::new_double(&[2, 2]))); }
            let mx = x.iter().sum::<f64>() / n;
            let my = y.iter().sum::<f64>() / n;
            let mut cxy = 0.0; let mut cxx = 0.0; let mut cyy = 0.0;
            for i in 0..x.len().min(y.len()) {
                let dx = x[i] - mx; let dy = y[i] - my;
                cxx += dx * dx; cyy += dy * dy; cxy += dx * dy;
            }
            cxx /= n - 1.0; cyy /= n - 1.0; cxy /= n - 1.0;
            Ok(Value::Array(MxArray::from_double_colmajor(vec![cxx, cxy, cxy, cyy], &[2, 2])))
        }
        // ── Fourier transforms ──
        "fft" => {
            let x = a(0)?;
            let v = x.iter_complex();
            let n = v.len();
            if n <= 1 { return Ok(Value::Array(x.clone())); }
            let mut out = vec![Complex64::default(); n];
            for k in 0..n {
                let mut sum = Complex64::default();
                for j in 0..n {
                    let angle = -2.0 * std::f64::consts::PI * (k as f64) * (j as f64) / (n as f64);
                    sum += v[j] * Complex64::new(angle.cos(), angle.sin());
                }
                out[k] = sum;
            }
            Ok(Value::Array(MxArray::from_complex_colmajor(out, &x.shape)))
        }
        "ifft" => {
            let x = a(0)?;
            let v = x.iter_complex();
            let n = v.len() as f64;
            if n <= 1.0 { return Ok(Value::Array(x.clone())); }
            let n_int = v.len();
            let mut out = vec![Complex64::default(); n_int];
            for k in 0..n_int {
                let mut sum = Complex64::default();
                for j in 0..n_int {
                    let angle = 2.0 * std::f64::consts::PI * (k as f64) * (j as f64) / n;
                    sum += v[j] * Complex64::new(angle.cos(), angle.sin());
                }
                out[k] = sum / Complex64::new(n, 0.0);
            }
            Ok(Value::Array(MxArray::from_complex_colmajor(out, &x.shape)))
        }
        "fftshift" => {
            let x = a(0)?;
            let v = x.iter_complex();
            let n = v.len();
            let mid = (n + 1) / 2;
            let shifted: Vec<Complex64> = v[mid..].iter().chain(v[..mid].iter()).copied().collect();
            Ok(Value::Array(MxArray::from_complex_colmajor(shifted, &x.shape)))
        }
        "ifftshift" => {
            let x = a(0)?;
            let v = x.iter_complex();
            let n = v.len();
            let mid = n / 2;
            let shifted: Vec<Complex64> = v[mid..].iter().chain(v[..mid].iter()).copied().collect();
            Ok(Value::Array(MxArray::from_complex_colmajor(shifted, &x.shape)))
        }
        "conv" => {
            let x = a(0)?.to_double_vec();
            let y = a(1)?.to_double_vec();
            let out_len = x.len() + y.len() - 1;
            let mut out = vec![0.0; out_len];
            for i in 0..x.len() {
                for j in 0..y.len() {
                    out[i + j] += x[i] * y[j];
                }
            }
            Ok(Value::Array(MxArray::from_double_colmajor(out, &[1, out_len])))
        }
        "filter" => {
            let b = a(0)?.to_double_vec();
            let a_vec = a(1)?.to_double_vec();
            let x = a(2)?.to_double_vec();
            let n = x.len();
            let mut y = vec![0.0; n];
            for i in 0..n {
                let mut acc = 0.0;
                for j in 0..b.len().min(i + 1) { acc += b[j] * x[i - j]; }
                for j in 1..a_vec.len().min(i + 1) { acc -= a_vec[j] * y[i - j]; }
                y[i] = if a_vec.is_empty() || a_vec[0] == 0.0 { 0.0 } else { acc / a_vec[0] };
            }
            Ok(Value::Array(MxArray::from_double_colmajor(y, &[n, 1])))
        }
        // ── Polynomials ──
        "polyval" => {
            let c = a(0)?.to_double_vec();
            let x = a(1)?.to_double_vec();
            let vals: Vec<f64> = x.iter().map(|&xv| {
                c.iter().fold(0.0, |acc, &coeff| acc * xv + coeff)
            }).collect();
            let n = vals.len();
            Ok(Value::Array(MxArray::from_double_colmajor(vals, &[1, n])))
        }
        "roots" => {
            let c = a(0)?.to_double_vec();
            if c.len() <= 1 { return Ok(Value::Array(MxArray::new_complex(&[1, 0]))); }
            let n = c.len() - 1;
            let mut mat = MxArray::new_complex(&[n, n]);
            for i in 0..n {
                if i < n - 1 { mat.set_complex(&[i + 1, i], Complex64::new(1.0, 0.0)); }
                mat.set_complex(&[i, n - 1], Complex64::new(-c[n - 1 - i] / c[n], 0.0));
            }
            match linalg::eig(&mat) {
                Ok((vals, _)) => {
                    let ev: Vec<Complex64> = (0..n).map(|i| vals.get_complex(&[i, 0])).collect();
                    Ok(Value::Array(MxArray::from_complex_colmajor(ev, &[n, 1])))
                }
                Err(_) => Ok(Value::Array(MxArray::new_complex(&[1, 0])))
            }
        }
        "poly" => {
            let roots = a(0)?.iter_complex();
            let _n_roots = roots.len();
            let mut c = vec![Complex64::new(1.0, 0.0)];
            for &r in &roots {
                let mut new_c = vec![Complex64::default(); c.len() + 1];
                new_c[0] = Complex64::default();
                for i in 0..c.len() { new_c[i + 1] = c[i] * -r; }
                for i in 0..c.len() { new_c[i] += c[i]; }
                c = new_c;
            }
            let out: Vec<f64> = c.iter().map(|z| z.re).collect();
            let n = out.len();
            Ok(Value::Array(MxArray::from_double_colmajor(out, &[1, n])))
        }
        "polyfit" => {
            let x = a(0)?.to_double_vec();
            let y = a(1)?.to_double_vec();
            let n = args.get(2).and_then(|v| Some(v.as_array().to_scalar() as usize)).unwrap_or(1);
            let m = x.len().min(y.len());
            if m == 0 || n >= m { return Ok(Value::Array(MxArray::new_double(&[1, 0]))); }
            let mut a_mat = vec![vec![0.0; n + 1]; m];
            for i in 0..m {
                for j in 0..=n { a_mat[i][j] = x[i].powi(j as i32); }
            }
            let mut lhs = vec![vec![0.0; n + 1]; n + 1];
            let mut rhs = vec![0.0; n + 1];
            for i in 0..=n {
                for j in 0..=n {
                    for k in 0..m { lhs[i][j] += a_mat[k][i] * a_mat[k][j]; }
                }
                for k in 0..m { rhs[i] += a_mat[k][i] * y[k]; }
            }
            let mut mat_mx = MxArray::new_double(&[n + 1, n + 1]);
            for i in 0..=n { for j in 0..=n { mat_mx.set_double(&[i, j], lhs[i][j]); } }
            let inv = linalg::inv(&mat_mx).unwrap_or_else(|_| MxArray::new_double(&[n + 1, n + 1]));
            let mut coeffs = vec![0.0; n + 1];
            for i in 0..=n {
                for j in 0..=n { coeffs[i] += inv.get_double(&[i, j]) * rhs[j]; }
            }
            coeffs.reverse();
            Ok(Value::Array(MxArray::from_double_colmajor(coeffs, &[1, n + 1])))
        }
        // ── Numerical methods ──
        "fzero" => {
            let fun_name = match args.first() { Some(Value::String(s)) => s.clone(), _ => return Err(EngineError::Msg("fzero: first argument must be a function name".into())) };
            let x0 = a(1)?.to_scalar();
            let mut x = x0;
            for _ in 0..100 {
                let fx = engine.dispatch_call(&fun_name, vec![Value::Array(MxArray::scalar_double(x))]);
                let fval = match fx { Ok(Value::Array(a)) => a.to_scalar(), _ => break, };
                if fval.abs() < 1e-14 { break; }
                let h = x * 1e-8 + 1e-10;
                let fx_ph = engine.dispatch_call(&fun_name, vec![Value::Array(MxArray::scalar_double(x + h))]);
                let fph = match fx_ph { Ok(Value::Array(a)) => a.to_scalar(), _ => break, };
                let deriv = (fph - fval) / h;
                if deriv.abs() < 1e-15 { break; }
                let mut step = fval / deriv;
                if step.abs() > 1.0 { step = step.signum(); }
                x -= step;
                if step.abs() < 1e-14 { break; }
            }
            Ok(Value::Array(MxArray::scalar_double(x)))
        }
        "trapz" => {
            let x = a(0)?.to_double_vec();
            let y = a(1)?.to_double_vec();
            let n = x.len().min(y.len());
            if n < 2 { return Ok(Value::Array(MxArray::scalar_double(0.0))); }
            let mut sum = 0.0;
            for i in 1..n { sum += (x[i] - x[i - 1]) * (y[i] + y[i - 1]) / 2.0; }
            Ok(Value::Array(MxArray::scalar_double(sum)))
        }
        "integral" => {
            let fun_name = match args.first() { Some(Value::String(s)) => s.clone(), _ => return Err(EngineError::Msg("integral: first argument must be a function name".into())) };
            let a_val = a(1)?.to_scalar();
            let b_val = a(2)?.to_scalar();
            let n = 1000;
            let h = (b_val - a_val) / n as f64;
            let mut sum = 0.0;
            for i in 0..n {
                let x1 = a_val + i as f64 * h;
                let x2 = a_val + (i + 1) as f64 * h;
                let y1 = engine.dispatch_call(&fun_name, vec![Value::Array(MxArray::scalar_double(x1))]);
                let y2 = engine.dispatch_call(&fun_name, vec![Value::Array(MxArray::scalar_double(x2))]);
                let f1 = match y1 { Ok(Value::Array(ref a)) => a.to_scalar(), _ => 0.0 };
                let f2 = match y2 { Ok(Value::Array(ref a)) => a.to_scalar(), _ => 0.0 };
                sum += (f1 + f2) / 2.0 * h;
            }
            Ok(Value::Array(MxArray::scalar_double(sum)))
        }
        // ── Special functions ──
        "erf" => {
            let arr = a(0)?;
            let out: Vec<f64> = arr.to_double_vec().iter().map(|&v| erf_approx(v)).collect();
            Ok(Value::Array(MxArray::from_double_colmajor(out, &arr.shape)))
        }
        "erfc" => {
            let arr = a(0)?;
            let out: Vec<f64> = arr.to_double_vec().iter().map(|&v| 1.0 - erf_approx(v)).collect();
            Ok(Value::Array(MxArray::from_double_colmajor(out, &arr.shape)))
        }
        "erfinv" => {
            let arr = a(0)?;
            let out: Vec<f64> = arr.to_double_vec().iter().map(|&v| {
                let w = -((1.0 - v) * (1.0 + v)).ln();
                let p = 0.933996 / w + 0.027878 / w.powi(2);
                let q = (v.signum()) * (p * p * w - p + 1.0 / 6.0 * w).sqrt();
                q
            }).collect();
            Ok(Value::Array(MxArray::from_double_colmajor(out, &arr.shape)))
        }
        "gamma" => {
            let arr = a(0)?;
            let out: Vec<f64> = arr.to_double_vec().iter().map(|&v| gamma_func(v)).collect();
            Ok(Value::Array(MxArray::from_double_colmajor(out, &arr.shape)))
        }
        "gammaln" => {
            let arr = a(0)?;
            let out: Vec<f64> = arr.to_double_vec().iter().map(|&v| gamma_func(v).ln()).collect();
            Ok(Value::Array(MxArray::from_double_colmajor(out, &arr.shape)))
        }
        // ── Type conversion ──
        "double" => {
            let x = a(0)?;
            Ok(Value::Array(MxArray::from_double_colmajor(x.to_double_vec(), &x.shape)))
        }
        "single" => {
            let x = a(0)?;
            Ok(Value::Array(MxArray::from_double_colmajor(x.to_double_vec(), &x.shape)))
        }
        "char" => {
            match &args[0] {
                Value::String(s) => Ok(Value::String(s.clone())),
                Value::Array(a) => {
                    let chars: String = a.to_double_vec().iter().map(|&v| v as u8 as char).collect();
                    Ok(Value::String(chars))
                }
                _ => Ok(Value::String(String::new())),
            }
        }
        "cellstr" => {
            match &args[0] {
                Value::String(s) => Ok(Value::Cell(vec![vec![Value::String(s.clone())]])),
                Value::Array(a) => {
                    let mut rows = Vec::new();
                    for i in 0..a.shape[0] {
                        let mut row = Vec::new();
                        for j in 0..a.shape.get(1).copied().unwrap_or(1) {
                            let c = a.get_double(&[i, j]) as u8 as char;
                            row.push(Value::String(c.to_string()));
                        }
                        rows.push(row);
                    }
                    Ok(Value::Cell(rows))
                }
                _ => Ok(Value::Cell(vec![])),
            }
        }
        "mat2cell" => {
            let x = a(0)?;
            let rows = if args.len() > 1 { a(1)?.to_scalar() as usize } else { 1 };
            let cols = if args.len() > 2 { a(2)?.to_scalar() as usize } else { 1 };
            let mut cells = Vec::new();
            for i in 0..rows.min(x.shape[0]) {
                let mut row = Vec::new();
                for j in 0..cols.min(x.shape.get(1).copied().unwrap_or(1)) {
                    let mut sub = MxArray::new_double(&[1, 1]);
                    sub.set_double(&[0, 0], x.get_double(&[i, j]));
                    row.push(Value::Array(sub));
                }
                cells.push(row);
            }
            Ok(Value::Cell(cells))
        }
        // ── Execution ──
        "eval" => {
            let code = match args.first() { Some(Value::String(s)) => s.as_str(), _ => "" };
            match engine.eval(code) {
                Ok(Some(v)) => Ok(v),
                Ok(None) => Ok(Value::Array(MxArray::new_double(&[1, 0]))),
                Err(e) => Err(e),
            }
        }
        "evalc" => {
            let code = match args.first() { Some(Value::String(s)) => s.as_str(), _ => "" };
            match engine.eval(code) {
                Ok(Some(v)) => Ok(Value::String(format!("{}", format_value(&v)))),
                Ok(None) => Ok(Value::String(String::new())),
                Err(e) => Ok(Value::String(format!("Error: {}", e))),
            }
        }
        // ── File system ──
        "pwd" => {
            match std::env::current_dir() {
                Ok(p) => Ok(Value::String(p.to_string_lossy().to_string())),
                Err(_) => Ok(Value::String(String::new())),
            }
        }
        "ls" | "dir" => {
            let path = match args.first() { Some(Value::String(s)) => s.as_str(), _ => "." };
            match std::fs::read_dir(path) {
                Ok(entries) => {
                    let names: Vec<String> = entries.filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string())).collect();
                    let joined = names.join("    ");
                    let out = MxArray::new_double(&[1, 0]);
                    let _ = writeln!(std::io::stdout(), "{}", joined);
                    Ok(Value::Array(out))
                }
                Err(_) => Ok(Value::Array(MxArray::new_double(&[1, 0]))),
            }
        }
        // ── Graphics stubs ──
        "figure" | "hold" | "xlabel" | "ylabel" | "title" | "grid" | "legend" | "bar" | "histogram" | "scatter" => {
            Ok(Value::Array(MxArray::scalar_double(1.0)))
        }
        "plot" => {
            Ok(Value::Array(MxArray::scalar_double(1.0)))
        }
        "axis" => {
            Ok(Value::Array(MxArray::from_double_colmajor(vec![0.0, 1.0, 0.0, 1.0], &[1, 4])))
        }
        "subplot" => {
            Ok(Value::Array(MxArray::scalar_double(1.0)))
        }
        "meshgrid" => {
            let x = a(0)?.to_double_vec();
            let y = if args.len() > 1 { a(1)?.to_double_vec() } else { x.clone() };
            let nx = x.len(); let ny = y.len();
            let mut xx = vec![0.0; nx * ny];
            let mut yy = vec![0.0; nx * ny];
            for i in 0..ny {
                for j in 0..nx {
                    xx[i + j * ny] = x[j];
                    yy[i + j * ny] = y[i];
                }
            }
            Ok(Value::Multi(vec![
                Value::Array(MxArray::from_double_colmajor(xx, &[ny, nx])),
                Value::Array(MxArray::from_double_colmajor(yy, &[ny, nx])),
            ]))
        }
        _ => Err(EngineError::Msg(format!("not a builtin: '{}'", name))),
    }
}

// Helper functions for special math

fn erf_approx(x: f64) -> f64 {
    // Maclaurin series erf(x) = (2/√π) Σ (-1)^n x^(2n+1) / (n!(2n+1)).
    // Exact at 0 and full double precision for moderate |x|; erf saturates to
    // ±1 within f64 precision beyond ~6, so short-circuit there.
    if x == 0.0 { return 0.0; }
    let sign = if x >= 0.0 { 1.0 } else { -1.0 };
    let ax = x.abs();
    if ax > 6.0 { return sign; }
    let x2 = ax * ax;
    let mut term = ax; // n = 0
    let mut sum = ax;
    for n in 1..200 {
        let n = n as f64;
        // a_n = a_{n-1} * (-x²/n) * (2n-1)/(2n+1)
        term *= -x2 / n * (2.0 * n - 1.0) / (2.0 * n + 1.0);
        sum += term;
        if term.abs() < 1e-18 * sum.abs() { break; }
    }
    sign * (2.0 / std::f64::consts::PI.sqrt()) * sum
}

fn gamma_func(x: f64) -> f64 {
    // Lanczos approximation for Gamma function
    if x <= 0.0 && x == x.trunc() { return std::f64::NAN; }
    if x < 0.5 {
        let pi = std::f64::consts::PI;
        let sin_pi_x = (pi * x).sin();
        if sin_pi_x.abs() < 1e-15 { return std::f64::NAN; }
        return pi / (sin_pi_x * gamma_func(1.0 - x));
    }
    let g = 7.0;
    let c: [f64; 9] = [
        0.99999999999980993, 676.5203681218851, -1259.1392167224028,
        771.32342877765313, -176.61502916214059, 12.507343278686905,
        -0.13857109526572012, 9.9843695780195716e-6, 1.5056327351493116e-7,
    ];
    let xm1 = x - 1.0;
    let mut sum = c[0];
    for i in 1..9 { sum += c[i] / (xm1 + i as f64); }
    let tmp = xm1 + g + 0.5;
    (tmp).powf(xm1 + 0.5) * (-tmp).exp() * (2.0 * std::f64::consts::PI).sqrt() * sum
}

fn conv(e: matbox_math::MathError) -> EngineError { EngineError::Msg(e.to_string()) }

fn round_half_away(x: f64) -> f64 {
    let f = x.floor();
    let r = x - f;
    if r < 0.5 { f } else if r > 0.5 { f + 1.0 } else if x >= 0.0 { f + 1.0 } else { f }
}
fn mod_f(x: f64, y: f64) -> f64 { x - (x / y).floor() * y }
fn rem_f(x: f64, y: f64) -> f64 { x - (x / y).trunc() * y }

fn arg_str(a: &MxArray) -> String {
    // single-quoted char vector or string
    if a.is_scalar() {
        // numeric code? treat as string from char codes
        let c = a.to_scalar() as u8;
        return (c as char).to_string();
    }
    String::new()
}

fn logical_scalar(b: bool) -> MxArray {
    let mut a = MxArray::new_logical(&[1, 1]);
    if let MxData::Logical(d) = &mut a.data { d[0] = b; }
    a
}

fn linear_of(arr: &MxArray, k: usize) -> Vec<usize> {
    let mut rem = k;
    let mut mi = vec![0usize; arr.ndims()];
    for d in 0..arr.ndims() {
        mi[d] = rem % arr.shape[d];
        rem /= arr.shape[d];
    }
    mi
}

fn flatten(m: &[Vec<Complex64>]) -> Vec<Complex64> {
    let cols = if !m.is_empty() { m[0].len() } else { 0 };
    let mut v = Vec::new();
    for j in 0..cols {
        for i in 0..m.len() { v.push(m[i][j]); }
    }
    v
}

fn parse_size(args: &[Value]) -> Vec<usize> {
    let mut shape = Vec::new();
    for arg in args {
        if let Value::Array(a) = arg {
            if a.ndims() == 2 && a.shape[1] >= 1 && a.shape[0] == 1 {
                for j in 0..a.shape[1] { shape.push(a.get_double(&[0, j]) as usize); }
            } else if a.is_scalar() {
                shape.push(a.to_scalar() as usize);
            } else {
                for c in a.iter_complex() { shape.push(c.re as usize); }
            }
        }
    }
    if shape.is_empty() { shape = vec![1, 1]; }
    shape
}

fn reduce_along(x: &MxArray, dim_arg: Option<&Value>, f: impl Fn(Complex64, Complex64) -> Complex64, init: f64) -> Result<Value, EngineError> {
    let dim = match dim_arg {
        Some(v) => v.as_array().to_scalar() as usize,
        None => {
            x.shape.iter().enumerate().find(|(_, &s)| s != 1).map(|(i, _)| i + 1).unwrap_or(1)
        }
    };
    let r = reduce(x, dim, Complex64::new(init, 0.0), f);
    Ok(Value::Array(r))
}

fn logical_reduce(x: &MxArray, dim_arg: Option<&Value>, init_val: bool, f: impl Fn(bool, bool) -> bool) -> Result<Value, EngineError> {
    let dim = match dim_arg {
        Some(v) => v.as_array().to_scalar() as usize,
        None => x.shape.iter().enumerate().find(|(_, &s)| s != 1).map(|(i, _)| i + 1).unwrap_or(1),
    };
    let r = reduce(x, dim, Complex64::new(init_val as i32 as f64, 0.0), |acc, y| {
        Complex64::new(if f(acc.norm() != 0.0, y.norm() != 0.0) { 1.0 } else { 0.0 }, 0.0)
    });
    Ok(Value::Array(r))
}

fn minmax(x: &MxArray, dim_arg: Option<&Value>, is_max: bool) -> Result<Value, EngineError> {
    let dim = match dim_arg {
        Some(v) => v.as_array().to_scalar() as usize,
        None => x.shape.iter().enumerate().find(|(_, &s)| s != 1).map(|(i, _)| i + 1).unwrap_or(1),
    };
    let init = if is_max { Complex64::new(f64::NEG_INFINITY, 0.0) } else { Complex64::new(f64::INFINITY, 0.0) };
    let f: fn(Complex64, Complex64) -> Complex64 = if is_max {
        |a, b| if a.re >= b.re { a } else { b }
    } else {
        |a, b| if a.re <= b.re { a } else { b }
    };
    let r = reduce(x, dim, init, f);
    Ok(Value::Array(r))
}

fn magic_square(n: usize) -> MxArray {
    if n == 1 { return MxArray::from_double_colmajor(vec![1.0], &[1, 1]); }
    let mut m = vec![vec![0.0; n]; n];
    let mut i = 0; let mut j = n / 2;
    for k in 1..=n * n {
        m[i][j] = k as f64;
        let ni = if i == 0 { n - 1 } else { i - 1 };
        let nj = (j + 1) % n;
        if m[ni][nj] != 0.0 { i = (i + 1) % n; } else { i = ni; j = nj; }
    }
    let mut v = Vec::with_capacity(n * n);
    for j in 0..n { for i in 0..n { v.push(m[i][j]); } }
    MxArray::from_double_colmajor(v, &[n, n])
}

fn diag(x: &MxArray) -> MxArray {
    if x.shape.len() == 2 && x.shape[0] > 1 && x.shape[1] > 1 {
        // matrix -> extract main diagonal as column vector
        let n = x.shape[0].min(x.shape[1]);
        let v: Vec<f64> = (0..n).map(|i| x.get_double(&[i, i])).collect();
        MxArray::from_double_colmajor(v, &[n, 1])
    } else {
        // vector -> build diagonal matrix
        let n = x.numel();
        let mut v = vec![0.0; n * n];
        for i in 0..n { v[i + i * n] = x.get_double(&[i, 0]); }
        MxArray::from_double_colmajor(v, &[n, n])
    }
}

/// Dimensions of any value, for size/numel/length/ndims/isempty. Cells report
/// their [rows, cols], a char row reports [1, len] (empty string is [0, 0]),
/// and scalars like structs/handles report [1, 1].
/// Flatten a cellfun/arrayfun data argument into a column-major list of the
/// per-element Values to pass to the mapped function, plus its 2-D shape.
/// cellfun unwraps cell contents; arrayfun walks array/char elements.
fn flatten_fun_arg(name: &str, v: &Value) -> Result<(Vec<Value>, [usize; 2]), EngineError> {
    let cell_cm = |rows: &Vec<Vec<Value>>| {
        let m = rows.len();
        let n = if m > 0 { rows[0].len() } else { 0 };
        let mut out = Vec::with_capacity(m * n);
        for j in 0..n { for i in 0..m { out.push(rows[i][j].clone()); } }
        (out, [m, n])
    };
    match (name, v) {
        ("cellfun", Value::Cell(rows)) => Ok(cell_cm(rows)),
        ("cellfun", _) => Err(EngineError::Msg("cellfun: input #2 must be a cell array".into())),
        (_, Value::Array(a)) => {
            let m = a.shape.first().copied().unwrap_or(1);
            let n = a.shape.get(1).copied().unwrap_or(1);
            let out = a.iter_complex().into_iter().map(|c| Value::Array(MxArray::scalar_complex(c))).collect();
            Ok((out, [m, n]))
        }
        (_, Value::String(s)) => {
            let chars: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
            let n = chars.len();
            Ok((chars, [1, n]))
        }
        (_, Value::Cell(rows)) => Ok(cell_cm(rows)),
        _ => Err(EngineError::Msg(format!("{}: unsupported input type", name))),
    }
}

fn value_shape(v: &Value) -> Vec<usize> {
    match v {
        Value::Array(a) => a.shape.clone(),
        Value::String(s) => {
            let len = s.chars().count();
            if len == 0 { vec![0, 0] } else { vec![1, len] }
        }
        Value::Cell(rows) => {
            let m = rows.len();
            let n = if m > 0 { rows[0].len() } else { 0 };
            vec![m, n]
        }
        Value::Struct(_) | Value::Func(_, _, _) | Value::FuncRef(_) => vec![1, 1],
        Value::Multi(_) => vec![1, 1],
    }
}

fn repmat(x: &MxArray, reps: &[Value]) -> MxArray {
    // reps may be given as separate scalars — repmat(A, m, n) — or as a single
    // vector — repmat(A, [m n]). A single scalar n means an n×n tiling.
    let r: Vec<usize> = if reps.len() == 1 {
        let arr = reps[0].as_array();
        if arr.numel() > 1 {
            arr.iter_complex().iter().map(|c| c.re as usize).collect()
        } else {
            let k = arr.to_scalar() as usize;
            vec![k, k]
        }
    } else {
        reps.iter().map(|v| v.as_array().to_scalar() as usize).collect()
    };
    let mut out = x.clone();
    for (d, &rep) in r.iter().enumerate() {
        let mut tiled = Vec::new();
        for _ in 0..rep { tiled.push(out.clone()); }
        out = crate::eval::concat(&tiled, d).expect("repmat concat");
    }
    out
}

fn kron(x: &MxArray, y: &MxArray) -> MxArray {
    let (m, n) = (x.shape[0], x.shape.get(1).copied().unwrap_or(1));
    let (p, q) = (y.shape[0], y.shape.get(1).copied().unwrap_or(1));
    let rm = m * p; let rn = n * q;
    let mut out = MxArray::new_complex(&[rm, rn]);
    for i in 0..m {
        for j in 0..n {
            let a = x.get_complex(&[i, j]);
            for ii in 0..p {
                for jj in 0..q {
                    let b = y.get_complex(&[ii, jj]);
                    out.set_complex(&[i * p + ii, j * q + jj], a * b);
                }
            }
        }
    }
    out
}

fn flip_dim(x: &MxArray, dim: usize) -> MxArray {
    let mut out = x.clone();
    let ndim = x.ndims();
    let size = x.shape[dim];
    let total = x.numel();
    for lin in 0..total {
        let mut mi = vec![0usize; ndim];
        let mut rem = lin;
        for d in 0..ndim { mi[d] = rem % x.shape[d]; rem /= x.shape[d]; }
        let mut mi2 = mi.clone();
        mi2[dim] = size - 1 - mi[dim];
        let mut lin2 = 0usize;
        let mut strd = 1usize;
        for d in 0..ndim { lin2 += mi2[d] * strd; strd *= x.shape[d]; }
        match &mut out.data {
            MxData::Double(d) => d[lin2] = x.get_double(&mi),
            MxData::Complex(d) => d[lin2] = x.get_complex(&mi),
            MxData::Logical(d) => d[lin2] = x.get_complex(&mi).norm() != 0.0,
            MxData::Int64(d) => d[lin2] = x.get_complex(&mi).re as i64,
        }
    }
    out
}

pub fn format_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => format_array(a),
        Value::Cell(rows) => {
            if rows.is_empty() { return "{}".into(); }
            let mut s = String::from("{\n");
            for row in rows {
                for (i, val) in row.iter().enumerate() {
                    if i > 0 { s.push_str(", "); }
                    s.push_str(&format_value(val));
                }
                s.push_str(";\n");
            }
            s.push('}');
            s
        }
        Value::Struct(map) => {
            let mut s = String::from("struct with fields:\n");
            for k in map.keys() {
                s.push_str(&format!("    {}: {}\n", k, ""));
            }
            s
        }
        Value::Multi(vals) => {
            if vals.is_empty() { return "[]".into(); }
            vals.iter().map(format_value).collect::<Vec<_>>().join(", ")
        }
        Value::Func(params, _body, _captured) => {
            format!("@({}) ...", params.join(", "))
        }
        Value::FuncRef(name) => {
            format!("@{}", name)
        }
    }
}

fn fmt_val(c: num_complex::Complex64) -> String {
    let re = c.re;
    let im = c.im;
    if im.abs() < 1e-10 {
        format!("{:8.4}", re)
    } else if re.abs() < 1e-10 {
        format!("{:8.4}i", im)
    } else if im < 0.0 {
        format!("{:8.4} - {:7.4}i", re, -im)
    } else {
        format!("{:8.4} + {:7.4}i", re, im)
    }
}

fn format_array(a: &MxArray) -> String {
    if a.is_scalar() {
        let c = a.get_complex(&[]);
        return fmt_val(c);
    }
    let m = a.shape[0];
    let n = a.shape.get(1).copied().unwrap_or(1);
    if n == 1 {
        let mut s = String::new();
        for i in 0..m {
            s.push_str(&fmt_val(a.get_complex(&[i, 0])));
            s.push('\n');
        }
        return s;
    }
    let mut s = String::new();
    for i in 0..m {
        for j in 0..n {
            s.push_str(&fmt_val(a.get_complex(&[i, j])));
            s.push(' ');
        }
        s.push('\n');
    }
    s
}
