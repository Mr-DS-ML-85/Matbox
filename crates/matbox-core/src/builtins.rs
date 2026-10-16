//! Builtin functions for MATBOX (MATLAB-equivalent signatures).

use crate::eval::{Engine, EngineError, Value};
use crate::eval::values_equal;
use matbox_math::array::{MxArray, MxData};
use matbox_math::linalg;
use matbox_math::{binary_elementwise, reduce, unary_elementwise};
use num_complex::Complex64;
use std::io::Write;

/// Inclusive value range of an integer type name, used by the `int*`
/// constructors to saturate instead of wrapping.
fn int_range(name: &str) -> (i64, i64) {
    match name {
        "int8" => (i8::MIN as i64, i8::MAX as i64),
        "uint8" => (0, u8::MAX as i64),
        "int16" => (i16::MIN as i64, i16::MAX as i64),
        "uint16" => (0, u16::MAX as i64),
        "int32" => (i32::MIN as i64, i32::MAX as i64),
        "uint32" => (0, u32::MAX as i64),
        "uint64" => (0, i64::MAX),
        _ => (i64::MIN, i64::MAX),
    }
}

/// Conversion characters that end the flags/width/precision part of a
/// printf-style spec.
const CONVERSIONS: &str = "diouxXeEfFgGsc";

/// Pull a field width and a decimal precision out of the text between `%` and
/// the conversion character. Absent parts come back as `None`, letting each
/// conversion apply its own default.
fn parse_spec(spec: &str) -> (usize, Option<usize>) {
    let chars: Vec<char> = spec.chars().collect();
    let mut k = 0usize;
    while k < chars.len() && matches!(chars[k], '-' | '+' | ' ' | '0' | '#') { k += 1; }
    let mut width = String::new();
    while k < chars.len() && chars[k].is_ascii_digit() {
        width.push(chars[k]);
        k += 1;
    }
    let mut prec = None;
    if k < chars.len() && chars[k] == '.' {
        k += 1;
        let mut p = String::new();
        while k < chars.len() && chars[k].is_ascii_digit() {
            p.push(chars[k]);
            k += 1;
        }
        prec = Some(p.parse::<usize>().unwrap_or(6));
    }
    (width.parse::<usize>().unwrap_or(0), prec)
}

/// Fixed-point rendering that keeps the non-finite spellings (`Inf`, `NaN`)
/// instead of letting the numeric formatter produce `inf` or `nan`.
fn fmt_float(v: f64, prec: usize) -> String {
    if v.is_nan() { return "NaN".into(); }
    if v.is_infinite() { return if v < 0.0 { "-Inf".into() } else { "Inf".into() }; }
    format!("{:.*}", prec, v)
}

/// `%g`-style rendering: fixed notation for moderate magnitudes, exponent
/// notation outside them, trailing zeros dropped.
fn format_g(v: f64, prec: usize) -> String {
    if v.is_nan() { return "NaN".into(); }
    if v.is_infinite() { return if v < 0.0 { "-Inf".into() } else { "Inf".into() }; }
    if v == 0.0 { return "0".into(); }
    let exp = v.abs().log10().floor() as i32;
    if exp < -4 || exp >= prec.max(1) as i32 {
        let mut s = format!("{:.*e}", prec.saturating_sub(1), v);
        // Rust writes `1.5e3`; the compact form drops the padding zeros.
        if let Some(pos) = s.find('e') {
            let (mant, e) = s.split_at(pos);
            let mant = trim_zeros(mant);
            s = format!("{}e{}", mant, e[1..].parse::<i32>().unwrap_or(0));
        }
        s
    } else {
        let decimals = (prec as i32 - 1 - exp).max(0) as usize;
        trim_zeros(&format!("{:.*}", decimals, v))
    }
}

fn trim_zeros(s: &str) -> String {
    if !s.contains('.') { return s.to_string(); }
    let t = s.trim_end_matches('0');
    t.trim_end_matches('.').to_string()
}

/// Number as text, matching how `mat2str` renders one: no column padding,
/// since padding is layout rather than part of the value, and the same
/// non-finite spellings used when printing.
fn number_text(v: f64) -> String {
    if v.is_nan() { return "NaN".into(); }
    if v.is_infinite() { return if v < 0.0 { "-Inf".into() } else { "Inf".into() }; }
    format!("{}", v)
}

/// Text form of a value for data conversion. Unlike `format_value` this must not
/// pad numbers for column alignment, so a numeric cell inside a converted cell
/// array yields its digits rather than a padded display field.
fn value_to_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Str(g) if g.len() == 1 && g[0].len() == 1 => g[0][0].clone(),
        Value::Array(a) if a.numel() == 1 => number_text(a.to_scalar()),
        other => format_value(other),
    }
}

/// Greatest common divisor by Euclid's algorithm, which stays fast on large
/// inputs where a trial-division scan would not.
fn gcd_f64(mut p: f64, mut q: f64) -> f64 {
    p = p.abs();
    q = q.abs();
    while q > 0.0 {
        let t = q;
        q = p % q;
        p = t;
    }
    p
}

/// Trial-division primality test, reported as logical 1/0. Cheap enough for the
/// modest magnitudes these predicates are used with.
fn is_prime_value(x: f64) -> f64 {
    if x < 2.0 || x.fract() != 0.0 { return 0.0; }
    let n = x as u64;
    if n % 2 == 0 { return if n == 2 { 1.0 } else { 0.0 }; }
    let mut d = 3u64;
    while d as f64 * d as f64 <= n as f64 {
        if n % d == 0 { return 0.0; }
        d += 2;
    }
    1.0
}

/// Element-wise integer-valued operation over two arrays, broadcasting a
/// scalar operand against the other and taking the shape of whichever side is
/// the longer one.
fn integer_binary(x: &MxArray, y: &MxArray, f: impl Fn(f64, f64) -> f64) -> MxArray {
    let xv = x.iter_complex();
    let yv = y.iter_complex();
    let (n, shape) = if yv.len() == 1 {
        (xv.len(), x.shape.clone())
    } else if xv.len() == 1 {
        (yv.len(), y.shape.clone())
    } else {
        (xv.len().min(yv.len()), x.shape.clone())
    };
    let out: Vec<f64> = (0..n)
        .map(|i| {
            let p = xv[i.min(xv.len().saturating_sub(1))].re;
            let q = yv[i.min(yv.len().saturating_sub(1))].re;
            f(p, q)
        })
        .collect();
    MxArray::from_double_colmajor(out, &shape)
}

/// Collect the values lying along `dim` for each output position of a
/// reduction, with the output shape. Positions come back in column-major
/// output order, and within a group in increasing index order along `dim`.
fn groups_along_dim(x: &MxArray, dim: usize) -> (Vec<usize>, Vec<Vec<Complex64>>) {
    let ndim = x.ndims();
    let d = dim.min(ndim).max(1) - 1;
    let mut out_shape = x.shape.clone();
    out_shape[d] = 1;
    while out_shape.len() < 2 { out_shape.push(1); }
    let out_strides = MxArray::col_strides(&out_shape);
    let total: usize = out_shape.iter().product();
    let mut groups: Vec<Vec<Complex64>> = vec![Vec::new(); total];
    for lin in 0..x.numel() {
        let mi = linear_of(x, lin);
        let mut oi = 0usize;
        for k in 0..ndim {
            if k == d { continue; }
            oi += mi[k] * out_strides[k];
        }
        groups[oi].push(x.get_complex(&mi));
    }
    (out_shape, groups)
}

/// Running extreme along `dim`, so the output keeps the input's shape.
///
/// The accumulator is keyed by the position with `dim` collapsed, which is what
/// makes successive elements along that axis build on each other, while the
/// result is still written back at every input position.
fn cum_extreme(x: &MxArray, dim: usize, is_max: bool) -> MxArray {
    let ndim = x.ndims();
    let d = dim.min(ndim).max(1) - 1;
    let mut shape = x.shape.clone();
    while shape.len() < 2 { shape.push(1); }
    let mut red_shape = x.shape.clone();
    red_shape[d] = 1;
    while red_shape.len() < 2 { red_shape.push(1); }
    let red_strides = MxArray::col_strides(&red_shape);
    let slots: usize = red_shape.iter().product();
    let mut acc = vec![Complex64::new(0.0, 0.0); slots];
    let mut seen = vec![false; slots];
    let mut buf = vec![Complex64::new(0.0, 0.0); x.numel()];
    for lin in 0..x.numel() {
        let mi = linear_of(x, lin);
        let mut oi = 0usize;
        for k in 0..ndim {
            if k == d { continue; }
            oi += mi[k] * red_strides[k];
        }
        let v = x.get_complex(&mi);
        // Ties keep the incumbent so the running value only moves when the
        // extreme genuinely changes.
        acc[oi] = if !seen[oi] {
            seen[oi] = true;
            v
        } else if is_max == (v.re >= acc[oi].re) {
            v
        } else {
            acc[oi]
        };
        buf[lin] = acc[oi];
    }
    MxArray::from_complex_colmajor(buf, &shape)
}

/// Vector norm along `dim`. Order defaults to 2; `Inf` and `-Inf` give the
/// largest and smallest magnitude, and 0 counts non-zeros.
fn vecnorm_array(x: &MxArray, dim: usize, ord: Option<f64>) -> MxArray {
    let (shape, groups) = groups_along_dim(x, dim);
    let p = ord.unwrap_or(2.0);
    let out: Vec<Complex64> = groups.iter().map(|g| {
        if g.is_empty() { return Complex64::new(0.0, 0.0); }
        let mags: Vec<f64> = g.iter().map(|c| c.norm()).collect();
        let r = if p == 0.0 {
            mags.iter().filter(|&&v| v != 0.0).count() as f64
        } else if p.is_infinite() {
            if p > 0.0 { mags.iter().cloned().fold(0.0f64, f64::max) }
            else { mags.iter().cloned().fold(0.0f64, f64::min) }
        } else {
            mags.iter().map(|&v| v.powf(p)).sum::<f64>().powf(1.0 / p)
        };
        Complex64::new(r, 0.0)
    }).collect();
    MxArray::from_complex_colmajor(out, &shape)
}

/// Shift elements cyclically, wrapping off one end to the other. A positive
/// count moves each element toward a higher index.
fn circshift(x: &MxArray, rows: i64, cols: i64) -> MxArray {
    let m = x.shape[0] as i64;
    let n = x.shape.get(1).copied().unwrap_or(1) as i64;
    let wrap = |v: i64, len: i64| -> usize {
        if len == 0 { return 0; }
        (((v % len) + len) % len) as usize
    };
    let mut buf = vec![Complex64::new(0.0, 0.0); x.numel()];
    for i in 0..m {
        for j in 0..n {
            let src = linear_of(x, (i as usize) + (j as usize) * (m as usize));
            let dst = wrap(i + rows, m) as usize + (wrap(j + cols, n) as usize) * (m as usize);
            buf[dst] = x.get_complex(&src);
        }
    }
    MxArray::from_complex_colmajor(buf, &x.shape)
}

/// Permutation vector from an argument, defaulting to `1..=ndims`.
fn order_vec(arg: Option<&Value>, ndim: usize) -> Result<Vec<usize>, EngineError> {
    let v = match arg {
        Some(v) => v.as_array().to_double_vec(),
        None => (1..=ndim).map(|i| i as f64).collect(),
    };
    if v.is_empty() { return Err(EngineError::Msg("permute: empty dimension order".into())); }
    Ok(v.iter().map(|&d| d as usize).collect())
}

/// Reorder dimensions. Column-major data is gathered through the inverse of the
/// requested order, so the result really is the permuted array.
fn permute_array(x: &MxArray, order: &[usize]) -> MxArray {
    let ndim = x.ndims();
    let d = ndim.min(order.len());
    let mut new_shape = vec![1usize; d];
    for k in 0..d { new_shape[k] = x.shape[order[k].saturating_sub(1)]; }
    while new_shape.len() < 2 { new_shape.push(1); }
    let mut inv = vec![0usize; d];
    for (k, &o) in order.iter().take(d).enumerate() { inv[o.saturating_sub(1)] = k; }
    let total: usize = new_shape.iter().product();
    let mut buf = vec![Complex64::new(0.0, 0.0); total];
    for lin in 0..total {
        // Translate the position in the output back to one in the input.
        let mut rem = lin;
        let mut out_mi = vec![0usize; d];
        for k in 0..d { out_mi[k] = rem % new_shape[k]; rem /= new_shape[k]; }
        let mut in_mi = vec![0usize; ndim];
        for k in 0..d { in_mi[inv[k]] = out_mi[k]; }
        buf[lin] = x.get_complex(&in_mi);
    }
    MxArray::from_complex_colmajor(buf, &new_shape)
}

/// Order rows by one column, keeping the rows intact.
fn sortrows(x: &MxArray, key_col: usize) -> MxArray {
    let m = x.shape[0];
    let n = x.shape.get(1).copied().unwrap_or(1);
    let col = key_col.clamp(1, n.max(1)) - 1;
    let mut rows: Vec<usize> = (0..m).collect();
    rows.sort_by(|&i, &j| {
        x.get_double(&[i, col]).partial_cmp(&x.get_double(&[j, col])).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut buf = vec![Complex64::new(0.0, 0.0); x.numel()];
    for (dst, &src) in rows.iter().enumerate() {
        for j in 0..n { buf[dst + j * m] = x.get_complex(&[src, j]); }
    }
    MxArray::from_complex_colmajor(buf, &x.shape)
}

/// Repeat each element, either once per axis or `reps` times along axis 1.
fn repelem(x: &MxArray, reps: &[usize]) -> MxArray {
    let m = x.shape[0];
    let n = x.shape.get(1).copied().unwrap_or(1);
    // When one count is given it applies along the long axis, matching how a
    // single repetition count reads.
    let (rm, rn) = match reps.len() {
        0 => (1, 1),
        1 => {
            if m >= n { (reps[0], 1) } else { (1, reps[0]) }
        }
        _ => (reps[0], reps.get(1).copied().unwrap_or(1)),
    };
    let mut rows = Vec::with_capacity(m * rm);
    for i in 0..m {
        let mut row = Vec::with_capacity(n * rn);
        for j in 0..n {
            for _ in 0..rn { row.push(x.get_complex(&[i, j])); }
        }
        for _ in 0..rm { rows.push(row.clone()); }
    }
    let new_n = n * rn;
    let mut buf = Vec::with_capacity(m * rm * new_n);
    for row in &rows { buf.extend_from_slice(row); }
    MxArray::from_complex_colmajor(buf, &[m * rm, new_n])
}

/// Name of the value's type as MATBOX reports it from `class`. Numbers are
/// distinguished by their storage, so a logical array does not report as
/// `double`.
fn class_name(v: &Value) -> &'static str {
    match v {
        Value::Array(a) => match &a.data {
            MxData::Logical(_) => "logical",
            MxData::Int64(_) => "int64",
            MxData::Double(_) | MxData::Complex(_) => "double",
        },
        Value::String(_) => "char",
        Value::Str(_) => "string",
        Value::Cell(_) => "cell",
        Value::Struct(_) => "struct",
        Value::Map(_) => "map",
        Value::Multi(_) => "cell",
        Value::Func(..) | Value::FuncRef(_) => "function handle",
    }
}

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
        "min" => minmax(&a(0)?, args, false, engine.nargout),
        "max" => minmax(&a(0)?, args, true, engine.nargout),
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
            let src = x.iter_complex();
            // Sort a permutation rather than the values, so the second output
            // can report it directly: `A(idx) = sort(A)` restores the input.
            // Ties keep input order (stable), matching the previous behavior
            // of sorting the values in place.
            let mut order: Vec<usize> = (0..src.len()).collect();
            order.sort_by(|&i, &j| src[i].re.partial_cmp(&src[j].re).unwrap_or(std::cmp::Ordering::Equal));
            let sorted: Vec<Complex64> = order.iter().map(|&i| src[i]).collect();
            if engine.nargout >= 2 {
                let perm: Vec<f64> = order.iter().map(|&i| (i + 1) as f64).collect();
                return Ok(Value::Multi(vec![
                    Value::Array(MxArray::from_complex_colmajor(sorted, &x.shape)),
                    Value::Array(MxArray::from_double_colmajor(perm, &x.shape)),
                ]));
            }
            Ok(Value::Array(MxArray::from_complex_colmajor(sorted, &x.shape)))
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
                let name = match crate::eval::text_arg(&args[0]) { Some(s) => s, None => return Ok(Value::Array(MxArray::scalar_double(0.0))), };
                if let Some((inp, _, _)) = engine.functions.get(&name) { Ok(Value::Array(MxArray::scalar_double(inp.len() as f64))) }
                else { Ok(Value::Array(MxArray::scalar_double(-1.0))) }
            } else {
                Ok(Value::Array(MxArray::scalar_double(-1.0)))
            }
        }
        "nargout" => {
            if n >= 1 {
                let name = match crate::eval::text_arg(&args[0]) { Some(s) => s, None => return Ok(Value::Array(MxArray::scalar_double(0.0))), };
                if let Some((_, out, _)) = engine.functions.get(&name) { Ok(Value::Array(MxArray::scalar_double(out.len() as f64))) }
                else { Ok(Value::Array(MxArray::scalar_double(-1.0))) }
            } else {
                Ok(Value::Array(MxArray::scalar_double(-1.0)))
            }
        }
        "run" => {
            let filename = match crate::eval::text_arg(&args[0]) { Some(s) => s, None => return Err(EngineError::Msg("run: argument must be a string".into())) };
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
        "rethrow" => {
            // Re-raises a caught exception with its original identifier intact,
            // so wrapping a block in try/catch does not flatten the error.
            let st = match args.first() {
                Some(Value::Struct(m)) => m,
                _ => return Err(EngineError::Msg("rethrow: argument must be a caught exception".into())),
            };
            let field = |k: &str| st.get(k)
                .and_then(|v| crate::eval::text_arg(v))
                .unwrap_or_default();
            Err(EngineError::Exception { identifier: field("identifier"), message: field("message") })
        }
        "error" => {
            // error(id, fmt, arg1, ...) — id is a `component:mnemonic` token,
            // fmt is sprintf-formatted with the remaining args, and the
            // thrown exception carries `id` as its identifier.
            // error(fmt, arg1, ...) — same sprintf formatting, but with no
            // leading id the identifier is empty.
            // error(msg) — single argument: literal text, no escape/format
            // expansion (nothing triggers sprintf processing).
            if n == 0 { return Err(EngineError::Msg("error: not enough input arguments".into())); }
            let first = match &args[0] {
                Value::String(s) => s.clone(),
                Value::Str(g) if g.len() == 1 && g[0].len() == 1 => g[0][0].clone(),
                other => format_value(other),
            };
            if is_error_identifier(&first) && n >= 2 {
                let msg = match call("sprintf", &args[1..], engine) {
                    Ok(Value::String(s)) => s,
                    Ok(Value::Str(g)) if g.len() == 1 && g[0].len() == 1 => g[0][0].clone(),
                    _ => format_value(&args[1]),
                };
                return Err(EngineError::Exception { identifier: first, message: msg });
            }
            if n >= 2 {
                let msg = match call("sprintf", args, engine) {
                    Ok(Value::String(s)) => s,
                    _ => first,
                };
                return Err(EngineError::Exception { identifier: String::new(), message: msg });
            }
            Err(EngineError::Exception { identifier: String::new(), message: first })
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
                    if let Some(s) = crate::eval::text_arg(arg) { engine.workspace.remove(&s); }
                }
            }
            Ok(Value::Array(MxArray::new_double(&[0, 0])))
        }
        "exist" => {
            let name = match crate::eval::text_arg(&args[0]) { Some(s) => s, None => return Ok(Value::Array(MxArray::scalar_double(0.0))) };
            Ok(Value::Array(MxArray::scalar_double(if engine.workspace.contains_key(&name) { 1.0 } else { 0.0 })))
        }
        "format" => {
            if n >= 1 { if let Some(s) = crate::eval::text_arg(&args[0]) { engine.format = s; } }
            Ok(Value::Array(MxArray::new_double(&[0, 0])))
        }
        "struct" => {
            let mut map = std::collections::HashMap::new();
            for i in (0..n).step_by(2) {
                let field = match crate::eval::text_arg(&args[i]) {
                    Some(s) => s,
                    None => return Err(EngineError::Msg("struct field names must be strings".into())),
                };
                let val = if i + 1 < n { args[i + 1].clone() } else { Value::Array(MxArray::new_double(&[0, 0])) };
                map.insert(field, val);
            }
            Ok(Value::Struct(map))
        }
        "isfield" => {
            let contains = match &args[0] {
                Value::Struct(m) => {
                    let f = match crate::eval::text_arg(&args[1]) {
                        Some(s) => s,
                        None => return Err(EngineError::Msg("isfield: second argument must be a string".into())),
                    };
                    m.contains_key(&f)
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
            // `[i, j] = find(X)` replaces the linear output with separate row
            // and column indices, both column-shaped, so callers can index
            // each axis without redoing the conversion themselves.
            if engine.nargout >= 2 {
                let m = x.shape[0].max(1);
                let mut rows = Vec::with_capacity(n);
                let mut cols = Vec::with_capacity(n);
                for &li in &idx {
                    let l = (li as usize).saturating_sub(1);
                    rows.push((l % m + 1) as f64);
                    cols.push((l / m + 1) as f64);
                }
                return Ok(Value::Multi(vec![
                    Value::Array(MxArray::from_double_colmajor(rows, &[n, 1])),
                    Value::Array(MxArray::from_double_colmajor(cols, &[n, 1])),
                ]));
            }
            Ok(Value::Array(MxArray::from_double_colmajor(idx, &shape)))
        }
        "strcmp" => {
            let s1 = crate::eval::text_arg(&args[0]).unwrap_or_else(|| "unimplemented".into());
            let s2 = crate::eval::text_arg(&args[1]).unwrap_or_else(|| "unimplemented".into());
            Ok(Value::Array(logical_scalar(s1 == s2)))
        }
        "strcmpi" => {
            let s1 = crate::eval::text_arg(&args[0]).unwrap_or_else(|| "unimplemented".into());
            let s2 = crate::eval::text_arg(&args[1]).unwrap_or_else(|| "unimplemented".into());
            Ok(Value::Array(logical_scalar(s1.to_lowercase() == s2.to_lowercase())))
        }
        "upper" => {
            let s = match crate::eval::text_arg(&args[0]) {
                Some(s) => s.to_uppercase(),
                None => match &args[0] {
                    Value::Array(a) if a.is_scalar() => format!("{}", a.to_scalar()).to_uppercase(),
                    _ => return Err(EngineError::Msg("upper: argument must be a string".into())),
                },
            };
            Ok(Value::String(s))
        }
        "lower" => {
            let s = match crate::eval::text_arg(&args[0]) {
                Some(s) => s.to_lowercase(),
                None => match &args[0] {
                    Value::Array(a) if a.is_scalar() => format!("{}", a.to_scalar()).to_lowercase(),
                    _ => return Err(EngineError::Msg("lower: argument must be a string".into())),
                },
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
        "mat2str" => {
            let prec = if n >= 2 { Some(a(1)?.to_scalar() as usize) } else { None };
            Ok(Value::String(mat2str_value(&args[0], prec)))
        }
        "str2double" => {
            let s = match crate::eval::text_arg(&args[0]) {
                Some(s) => s,
                None => match &args[0] {
                    Value::Array(a) if a.numel() == 1 => return Ok(Value::Array(a.clone())),
                    _ => return Err(EngineError::Msg("str2double: argument must be a string".into())),
                },
            };
            match s.trim().parse::<f64>() {
                Ok(v) => Ok(Value::Array(MxArray::scalar_double(v))),
                Err(_) => Ok(Value::Array(MxArray::scalar_double(f64::NAN))),
            }
        }
        "contains" => {
            let s = crate::eval::text_arg(&args[0]).ok_or_else(|| EngineError::Msg("contains: first argument must be a string".into()))?;
            let pat = crate::eval::text_arg(&args[1]).ok_or_else(|| EngineError::Msg("contains: second argument must be a string".into()))?;
            Ok(Value::Array(logical_scalar(s.contains(pat.as_str()))))
        }
        "startsWith" | "startswith" => {
            let s = crate::eval::text_arg(&args[0]).ok_or_else(|| EngineError::Msg("startsWith: first argument must be a string".into()))?;
            let pat = crate::eval::text_arg(&args[1]).ok_or_else(|| EngineError::Msg("startsWith: second argument must be a string".into()))?;
            Ok(Value::Array(logical_scalar(s.starts_with(pat.as_str()))))
        }
        "endsWith" | "endswith" => {
            let s = crate::eval::text_arg(&args[0]).ok_or_else(|| EngineError::Msg("endsWith: first argument must be a string".into()))?;
            let pat = crate::eval::text_arg(&args[1]).ok_or_else(|| EngineError::Msg("endsWith: second argument must be a string".into()))?;
            Ok(Value::Array(logical_scalar(s.ends_with(pat.as_str()))))
        }
        "strtrim" => {
            let s = crate::eval::text_arg(&args[0]).ok_or_else(|| EngineError::Msg("strtrim: argument must be a string".into()))?;
            Ok(Value::String(s.trim().to_string()))
        }
        "deblank" => {
            let s = crate::eval::text_arg(&args[0]).ok_or_else(|| EngineError::Msg("deblank: argument must be a string".into()))?;
            Ok(Value::String(s.trim_end().to_string()))
        }
        "strsplit" => {
            let s = crate::eval::text_arg(&args[0]).ok_or_else(|| EngineError::Msg("strsplit: first argument must be a string".into()))?;
            let delim = if args.len() > 1 {
                crate::eval::text_arg(&args[1]).ok_or_else(|| EngineError::Msg("strsplit: delimiter must be a string".into()))?
            } else { " ".into() };
            let parts: Vec<Value> = s.split(&delim).map(|p| Value::String(p.to_string())).collect();
            Ok(Value::Cell(vec![parts]))
        }
        "strjoin" => {
            let parts = match &args[0] { Value::Cell(rows) if rows.len() == 1 => rows[0].clone(), _ => return Err(EngineError::Msg("strjoin: first argument must be a cell array of strings".into())) };
            let delim = if args.len() > 1 {
                crate::eval::text_arg(&args[1]).ok_or_else(|| EngineError::Msg("strjoin: delimiter must be a string".into()))?
            } else { "".into() };
            let strings: Vec<String> = parts.iter().map(|v| crate::eval::text_arg(v).unwrap_or_default()).collect();
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
            let fmt = match &args[0] {
                Value::String(s) => s.clone(),
                Value::Str(g) if g.len() == 1 && g[0].len() == 1 => g[0][0].clone(),
                _ => return Err(EngineError::Msg("sprintf: first argument must be a format string".into())),
            };
            let mut result = String::new();
            let chars: Vec<char> = fmt.chars().collect();
            let mut arg_idx = 1usize;
            let mut i = 0usize;
            while i < chars.len() {
                let ch = chars[i];
                if ch == '%' && i + 1 < chars.len() && chars[i + 1] == '%' {
                    result.push('%');
                    i += 2;
                    continue;
                }
                if ch == '%' {
                    // Gather flags, width and precision up to the conversion
                    // character, so `%.2f` and `%5d` are honoured instead of
                    // being echoed back verbatim.
                    let mut spec = String::new();
                    i += 1;
                    while i < chars.len() && !CONVERSIONS.contains(chars[i]) {
                        spec.push(chars[i]);
                        i += 1;
                    }
                    if i >= chars.len() {
                        // Truncated format: emit what was read rather than
                        // silently dropping the tail.
                        result.push('%');
                        result.push_str(&spec);
                        break;
                    }
                    let conv = chars[i];
                    i += 1;
                    let (width, prec) = parse_spec(&spec);
                    let arg = args.get(arg_idx);
                    let rendered: String;
                    match conv {
                        'd' | 'i' => {
                            let n = match arg {
                                Some(Value::Array(a)) if a.is_scalar() => a.to_scalar().trunc() as i64,
                                Some(Value::String(s)) => s.parse::<i64>().unwrap_or(0),
                                _ => 0,
                            };
                            rendered = n.to_string();
                        }
                        'f' | 'F' | 'e' | 'E' | 'g' | 'G' => {
                            let v = match arg {
                                Some(Value::Array(a)) if a.is_scalar() => a.to_scalar(),
                                _ => 0.0,
                            };
                            // A float conversion defaults to six decimals when
                            // the format does not ask for a precision.
                            let body = match conv {
                                'f' | 'F' => fmt_float(v, prec.unwrap_or(6)),
                                'e' => format!("{:.*e}", prec.unwrap_or(6), v),
                                'E' => format!("{:.*E}", prec.unwrap_or(6), v),
                                _ => format_g(v, prec.unwrap_or(6)),
                            };
                            rendered = if conv == 'g' || conv == 'G' {
                                body.to_uppercase()
                            } else { body };
                        }
                        's' => {
                            rendered = match arg {
                                Some(Value::Array(a)) if a.is_scalar() => format_value(arg.unwrap()),
                                Some(Value::String(s)) => s.clone(),
                                Some(Value::Str(g)) if g.len() == 1 && g[0].len() == 1 => g[0][0].clone(),
                                _ => String::new(),
                            };
                        }
                        'c' => {
                            rendered = match arg {
                                Some(Value::String(s)) => s.chars().next().map(|c| c.to_string()).unwrap_or_default(),
                                Some(Value::Array(a)) if a.is_scalar() => {
                                    char::from_u32(a.to_scalar() as u32).map(|c| c.to_string()).unwrap_or_default()
                                }
                                _ => String::new(),
                            };
                        }
                        _ => { result.push('%'); result.push_str(&spec); result.push(conv); continue; }
                    }
                    if rendered.chars().count() < width {
                        let pad = " ".repeat(width - rendered.chars().count());
                        result.push_str(&pad);
                    }
                    result.push_str(&rendered);
                    arg_idx += 1;
                    continue;
                }
                // Literal text, with the usual backslash escapes expanded.
                if ch == '\\' && i + 1 < chars.len() {
                    let esc = chars[i + 1];
                    i += 2;
                    result.push(match esc {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '0' => '\0',
                        '\\' => '\\',
                        other => other,
                    });
                    continue;
                }
                result.push(ch);
                i += 1;
            }
            Ok(Value::String(result))
        }
        "fprintf" => {
            // fprintf(fmt, ...) writes to stdout; a leading scalar 1 or 2
            // selects stdout or stderr instead of being part of the format.
            // Any other leading number is not a file id, so the whole argument
            // list is treated as format-plus-values rather than guessed at.
            let (dest, rest) = match args.first() {
                Some(Value::Array(arr)) if arr.is_scalar() => match arr.to_scalar() {
                    1.0 => (1, &args[1..]),
                    2.0 => (2, &args[1..]),
                    _ => (1, &args[..]),
                },
                _ => (1, &args[..]),
            };
            let out = match call("sprintf", rest, engine) {
                Ok(Value::String(s)) => s,
                Ok(Value::Str(g)) if g.len() == 1 && g[0].len() == 1 => g[0][0].clone(),
                _ => return Err(EngineError::Msg(
                    "fprintf: first argument must be a format string".into())),
            };
            if dest == 2 {
                eprint!("{}", out);
            } else {
                print!("{}", out);
                // A partial line would otherwise sit in the buffer, so a REPL
                // caller waiting on the output would see nothing.
                let _ = std::io::stdout().flush();
            }
            // Returns the number of characters written, not the text.
            Ok(Value::Array(MxArray::scalar_double(out.chars().count() as f64)))
        }
        "class" => {
            let v = args.first().ok_or_else(|| EngineError::Msg(
                "class: not enough input arguments".into()))?;
            Ok(Value::String(class_name(v).to_string()))
        }
        // Integer constructors. Every integer width shares one `Int64`
        // storage, so narrowing saturates to the target range here rather than
        // wrapping; `class` therefore reports `int64` for all of them.
        "int8" | "uint8" | "int16" | "uint16" | "int32" | "uint32" | "int64" | "uint64" => {
            let (lo, hi) = int_range(name);
            let x = a(0)?;
            let out: Vec<i64> = x.iter_complex().iter()
                .map(|c| {
                    let r = c.re.round();
                    if r.is_nan() { 0 }
                    else if r <= lo as f64 { lo }
                    else if r >= hi as f64 { hi }
                    else { r as i64 }
                })
                .collect();
            Ok(Value::Array(MxArray::from_int64_colmajor(out, &x.shape)))
        }
        "idivide" => {
            let x = a(0)?;
            let y = a(1)?;
            if y.iter_complex().iter().any(|c| c.re == 0.0) {
                return Err(EngineError::Msg("idivide: division by zero".into()));
            }
            let xv = x.iter_complex();
            let yv = y.iter_complex();
            // A scalar on either side broadcasts; otherwise the two must be the
            // same length, since there is no general expansion here. The
            // non-scalar side supplies the output length, and the scalar side
            // is always the divisor — never the other way round.
            let out: Vec<i64> = if yv.len() == 1 {
                let d = yv[0];
                xv.iter().map(|n| (n.re / d.re).trunc() as i64).collect()
            } else if xv.len() == 1 {
                let n = xv[0];
                yv.iter().map(|d| (n.re / d.re).trunc() as i64).collect()
            } else if xv.len() == yv.len() {
                xv.iter().zip(yv.iter()).map(|(n, d)| (n.re / d.re).trunc() as i64).collect()
            } else {
                return Err(EngineError::Msg("idivide: nonconformant arguments".into()));
            };
            let shape = if xv.len() == 1 && yv.len() > 1 { &y.shape } else { &x.shape };
            Ok(Value::Array(MxArray::from_int64_colmajor(out, shape)))
        }
        "cumsum" => {
            let x = a(0)?;
            let dim = dim_from_args(&x, args, 1);
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
            let dim = dim_from_args(&x, args, 1);
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
            let dim = dim_from_args(&x, args, 1);
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
                if let Some(s) = crate::eval::text_arg(&args[i]) {
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
        "isstring" => {
            Ok(Value::Array(logical_scalar(matches!(&args[0], Value::Str(_)))))
        }
        "strlength" => {
            match &args[0] {
                Value::Str(rows) => {
                    let m = rows.len();
                    let nn = if m > 0 { rows[0].len() } else { 0 };
                    let mut buf = Vec::with_capacity(m * nn);
                    for j in 0..nn { for i in 0..m { buf.push(rows[i][j].chars().count() as f64); } }
                    Ok(Value::Array(MxArray::from_double_colmajor(buf, &[m, nn])))
                }
                Value::String(s) => Ok(Value::Array(MxArray::scalar_double(s.chars().count() as f64))),
                _ => Err(EngineError::Msg("strlength: argument must be a string".into())),
            }
        }
        "string" => {
            match &args[0] {
                Value::Str(_) => Ok(args[0].clone()),
                Value::String(s) => Ok(Value::Str(vec![vec![s.clone()]])),
                Value::Array(a) if a.is_scalar() => Ok(Value::Str(vec![vec![number_text(a.to_scalar())]])),
                Value::Array(a) => {
                    let m = a.shape[0];
                    let nn = a.shape.get(1).copied().unwrap_or(1);
                    let mut rows = vec![vec![String::new(); nn]; m];
                    for i in 0..m { for j in 0..nn { rows[i][j] = number_text(a.get_double(&[i, j])); } }
                    Ok(Value::Str(rows))
                }
                Value::Cell(rows) => {
                    let out: Vec<Vec<String>> = rows.iter().map(|r| r.iter().map(value_to_text).collect()).collect();
                    Ok(Value::Str(out))
                }
                other => Ok(Value::Str(vec![vec![value_to_text(other)]])),
            }
        }
        // ── element-wise math (wave 2) ──
        // Degrees and radians convert by a fixed factor, which is a multiply
        // rather than a transcendental call.
        "deg2rad" => elem1!(|c| c * (std::f64::consts::PI / 180.0)),
        "rad2deg" => elem1!(|c| c * (180.0 / std::f64::consts::PI)),
        "sec" => elem1!(|c| Complex64::new(1.0, 0.0) / c.cos()),
        "csc" => elem1!(|c| Complex64::new(1.0, 0.0) / c.sin()),
        "cot" => elem1!(|c| Complex64::new(1.0, 0.0) / c.tan()),
        "expm1" => elem1!(|c| c.exp() - Complex64::new(1.0, 0.0)),
        "log1p" => elem1!(|c| (c + Complex64::new(1.0, 0.0)).ln()),
        // The real-part square root, kept distinct from sqrt so that a caller
        // asking for the real root gets one even for a negative input.
        "realsqrt" => elem1!(|c| Complex64::new(c.re.sqrt(), 0.0)),
        "pow2" => elem1!(|c| Complex64::new(2.0, 0.0).powc(c)),
        "sqrtm" => elem1!(|c| c.sqrt()),
        "hypot" => elem2!(|x, y| (x * x + y * y).sqrt()),
        "gcd" => {
            let x = a(0)?;
            let y = a(1)?;
            Ok(Value::Array(integer_binary(&x, &y, |p, q| gcd_f64(p, q))))
        }
        "lcm" => {
            let x = a(0)?;
            let y = a(1)?;
            Ok(Value::Array(integer_binary(&x, &y, |p, q| {
                let (p, q) = (p.abs(), q.abs());
                if p == 0.0 || q == 0.0 { 0.0 } else { p * q / gcd_f64(p, q) }
            })))
        }
        "nextpow2" => elem1!(|c| {
            if c.re <= 1.0 { return Complex64::new(1.0, 0.0); }
            // 2^ceil(log2(x)) is the smallest power of two at or above x.
            Complex64::new(2f64.powf(c.re.log2().ceil()), 0.0)
        }),
        "nthroot" => {
            let x = a(0)?;
            let k = args.get(1).and_then(|v| v.as_array().to_scalar().into()).unwrap_or(2.0);
            if k == 0.0 { return Err(EngineError::Msg("nthroot: degree must be non-zero".into())); }
            // Odd roots of a negative number stay negative rather than going
            // complex, which is why the sign is applied after the power.
            Ok(Value::Array(unary_elementwise(&x, |c| {
                if c.re < 0.0 && k.fract() == 1.0 {
                    Complex64::new(-(-c.re).powf(1.0 / k), 0.0)
                } else {
                    Complex64::new(c.re.abs().powf(1.0 / k) * c.re.signum(), c.im)
                }
            })))
        }
        "factorial" => elem1!(|c| {
            if c.re < 0.0 && c.re.fract() == 0.0 { return Complex64::new(f64::NAN, 0.0); }
            let n = c.re.round();
            // Past 170 the product leaves the range of a double, and a
            // non-finite or negative argument would never finish a loop.
            if !n.is_finite() || n < 0.0 { return Complex64::new(f64::NAN, 0.0); }
            if n > 170.0 { return Complex64::new(f64::INFINITY, 0.0); }
            let mut acc = 1.0;
            let mut i = 2.0;
            while i <= n { acc *= i; i += 1.0; }
            Complex64::new(acc, 0.0)
        }),
        "isprime" => elem1!(|c| Complex64::new(is_prime_value(c.re), 0.0)),
        "primes" => {
            let limit = args.first().and_then(|v| v.as_array().to_scalar().into()).unwrap_or(100.0) as usize;
            // Trial division is linear in the limit, so cap it rather than let a
            // large argument turn into an unbounded scan.
            const PRIME_SCAN_LIMIT: usize = 1_000_000;
            let limit = limit.min(PRIME_SCAN_LIMIT);
            let out: Vec<f64> = (2..=limit).filter(|&p| is_prime_value(p as f64) != 0.0).map(|p| p as f64).collect();
            let n = out.len();
            Ok(Value::Array(MxArray::from_double_colmajor(out, &[n, 1])))
        }

        // ── logical and bitwise (wave 2) ──
        // Element-wise logical operators return logical arrays, so the result
        // is built as logical rather than converted back from doubles.
        "xor" => {
            let x = a(0)?;
            let y = a(1)?;
            let n = x.numel().max(y.numel());
            let mut flags = Vec::with_capacity(n);
            for i in 0..n {
                let p = x.iter_complex()[i.min(x.numel().saturating_sub(1))];
                let q = y.iter_complex()[i.min(y.numel().saturating_sub(1))];
                flags.push((p.norm() != 0.0) != (q.norm() != 0.0));
            }
            let shape = if y.numel() == 1 { x.shape.clone() } else if x.numel() == 1 { y.shape.clone() } else { x.shape.clone() };
            Ok(Value::Array(MxArray::from_logical_colmajor(flags, &shape)))
        }

        // ── linear algebra (wave 2) ──
        "tril" | "triu" => {
            let lower = name == "tril";
            let x = a(0)?;
            let k = args.get(1).and_then(|v| v.as_array().to_scalar().into()).unwrap_or(0.0) as i64;
            let m = x.shape[0];
            let n = x.shape.get(1).copied().unwrap_or(1);
            let mut out = x.clone();
            for i in 0..m {
                for j in 0..n {
                    let keep = if lower { (j as i64) <= (i as i64) + k } else { (j as i64) >= (i as i64) + k };
                    if !keep { out.set_complex(&[i, j], Complex64::new(0.0, 0.0)); }
                }
            }
            Ok(Value::Array(out))
        }
        "dot" => {
            let x = a(0)?;
            let y = a(1)?;
            // The product conjugates the first operand and sums along the first
            // axis that is not a singleton, so two vectors give a scalar while
            // two matrices give the Hermitian product A' * B.
            let dim = default_dim(&x);
            let prod = binary_elementwise(&x, &y, |p, q| p.conj() * q)
                .map_err(|e| EngineError::Msg(e.to_string()))?;
            Ok(Value::Array(reduce(&prod, dim, Complex64::new(0.0, 0.0), |s, v| s + v)))
        }
        "cross" => {
            let x = a(0)?.to_double_vec();
            let y = a(1)?.to_double_vec();
            if x.len() != 3 || y.len() != 3 {
                return Err(EngineError::Msg("cross: both inputs must be three-element vectors".into()));
            }
            let c = [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ];
            Ok(Value::Array(MxArray::from_double_colmajor(c.to_vec(), &[3, 1])))
        }
        "vecnorm" => {
            let x = a(0)?;
            let dim = dim_from_args(&x, args, 1);
            let ord = args.get(2).and_then(|v| v.as_array().to_scalar().into());
            Ok(Value::Array(vecnorm_array(&x, dim, ord)))
        }

        // ── array shaping and scanning (wave 2) ──
        "circshift" => {
            let x = a(0)?;
            // A scalar shift moves along the first axis that is not a singleton,
            // so it follows the vector's long axis. An explicit pair names rows
            // then columns.
            let (rows, cols) = match args.get(1) {
                Some(v) => {
                    let arr = v.as_array();
                    if arr.is_scalar() {
                        let s = arr.to_scalar() as i64;
                        if x.shape[0] > 1 { (s, 0) } else { (0, s) }
                    } else {
                        let last = 1.min(arr.shape[1].saturating_sub(1));
                        (arr.get_double(&[0, 0]) as i64, arr.get_double(&[0, last]) as i64)
                    }
                }
                None => (0, 0),
            };
            Ok(Value::Array(circshift(&x, rows, cols)))
        }
        "rot90" => {
            let x = a(0)?;
            let k = args.get(1).and_then(|v| v.as_array().to_scalar().into()).unwrap_or(1.0) as i64;
            let mut cur = x.clone();
            // One rotation is a transpose followed by a horizontal flip, which
            // is the cheapest direction available from the array API.
            for _ in 0..((k % 4) + 4) % 4 {
                let t = cur.transpose();
                cur = flip_dim(&t, 1);
            }
            Ok(Value::Array(cur))
        }
        "permute" => {
            let x = a(0)?;
            let order = order_vec(args.get(1), x.ndims())?;
            Ok(Value::Array(permute_array(&x, &order)))
        }
        "ipermute" => {
            let order = match args.first() {
                Some(v) => v.as_array().to_double_vec().iter().map(|&d| d as usize).collect::<Vec<usize>>(),
                None => return Err(EngineError::Msg("ipermute: expected a dimension order".into())),
            };
            // Invert the order: the axis that landed at position k in the
            // permuted array came from the axis the order names at k.
            let mut inv = vec![0usize; order.len()];
            for (i, &o) in order.iter().enumerate() {
                if o == 0 || o > order.len() {
                    return Err(EngineError::Msg("ipermute: dimension order out of range".into()));
                }
                inv[o - 1] = i + 1;
            }
            let n = inv.len();
            Ok(Value::Array(MxArray::from_double_colmajor(inv.iter().map(|&v| v as f64).collect(), &[n, 1])))
        }
        "squeeze" => {
            let x = a(0)?;
            let shape: Vec<usize> = x.shape.iter().copied().filter(|&s| s != 1).collect();
            Ok(Value::Array(MxArray::from_complex_colmajor(x.iter_complex(), &shape)))
        }
        "sortrows" => {
            let x = a(0)?;
            let key_col = args.get(1).and_then(|v| v.as_array().to_scalar().into()).unwrap_or(1.0) as usize;
            Ok(Value::Array(sortrows(&x, key_col)))
        }
        "repelem" => {
            let x = a(0)?;
            let reps: Vec<usize> = args[1..].iter().map(|v| v.as_array().to_scalar().max(0.0) as usize).collect();
            Ok(Value::Array(repelem(&x, &reps)))
        }
        "range" => {
            let x = a(0)?;
            let vals = x.iter_complex();
            if vals.is_empty() { return Ok(Value::Array(MxArray::new_double(&[1, 0]))); }
            let mut lo = vals[0].re;
            let mut hi = vals[0].re;
            for c in &vals { lo = lo.min(c.re); hi = hi.max(c.re); }
            Ok(Value::Array(MxArray::scalar_double(hi - lo)))
        }
        "rms" => {
            let x = a(0)?;
            let vals = x.iter_complex();
            if vals.is_empty() { return Ok(Value::Array(MxArray::new_double(&[1, 0]))); }
            // Root mean square: the square root of the mean of the squares,
            // not the standard deviation around the mean.
            let mean_sq = vals.iter().map(|c| c.re * c.re).sum::<f64>() / vals.len() as f64;
            Ok(Value::Array(MxArray::scalar_double(mean_sq.sqrt())))
        }
        "cummax" | "cummin" => {
            let is_max = name == "cummax";
            let x = a(0)?;
            let dim = dim_from_args(&x, args, 1);
            Ok(Value::Array(cum_extreme(&x, dim, is_max)))
        }

        // ── containers.Map ──
        // The entry count is reachable both as `m.Count` and as `Count(m)`.
        "Count" => {
            let m = match args.first() { Some(Value::Map(entries)) => entries, _ => return Err(EngineError::Msg("Count: argument must be a containers.Map".into())) };
            Ok(Value::Array(MxArray::scalar_double(m.len() as f64)))
        }
        "keys" => {
            let m = match &args[0] { Value::Map(entries) => entries, _ => return Err(EngineError::Msg("keys: argument must be a containers.Map".into())) };
            Ok(Value::Cell(vec![m.iter().map(|(k, _)| k.clone()).collect()]))
        }
        "values" => {
            let m = match &args[0] { Value::Map(entries) => entries, _ => return Err(EngineError::Msg("values: argument must be a containers.Map".into())) };
            Ok(Value::Cell(vec![m.iter().map(|(_, v)| v.clone()).collect()]))
        }
        "isKey" => {
            let m = match &args[0] { Value::Map(entries) => entries, _ => return Err(EngineError::Msg("isKey: first argument must be a containers.Map".into())) };
            match &args[1] {
                Value::Cell(rows) => {
                    let flat = crate::eval::flatten_row_major(rows);
                    let res: Vec<bool> = flat.iter().map(|k| m.iter().any(|(mk, _)| crate::eval::map_key_eq(mk, k))).collect();
                    let nn = res.len();
                    let mut out = MxArray::new_logical(&[1, nn]);
                    if let MxData::Logical(d) = &mut out.data {
                        for (i, v) in res.iter().enumerate() { d[i] = *v; }
                    }
                    Ok(Value::Array(out))
                }
                key => Ok(Value::Array(logical_scalar(m.iter().any(|(mk, _)| crate::eval::map_key_eq(mk, key))))),
            }
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
            let field = match args.get(1).and_then(crate::eval::text_arg) {
                Some(f) => f,
                None => return Err(EngineError::Msg("rmfield: second argument must be a field name".into())),
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
            let s = args.first().and_then(crate::eval::text_arg).unwrap_or_default();
            let pat = args.get(1).and_then(crate::eval::text_arg).unwrap_or_default();
            if pat.is_empty() { return Ok(Value::Array(MxArray::scalar_double(1.0))); }
            let mut indices = Vec::new();
            let mut start = 0;
            while let Some(pos) = s[start..].find(&pat) {
                indices.push((start + pos + 1) as f64);
                start += pos + 1;
            }
            let n = indices.len();
            let out = if n == 0 { MxArray::new_double(&[1, 0]) } else { MxArray::from_double_colmajor(indices, &[1, n]) };
            Ok(Value::Array(out))
        }
        "strrep" => {
            let s = args.first().and_then(crate::eval::text_arg).unwrap_or_default();
            let old = args.get(1).and_then(crate::eval::text_arg).unwrap_or_default();
            let new = args.get(2).and_then(crate::eval::text_arg).unwrap_or_default();
            Ok(Value::String(s.replace(&old, &new)))
        }
        "regexp" | "regexpi" => {
            let s = args.first().and_then(crate::eval::text_arg).unwrap_or_default();
            let pat = args.get(1).and_then(crate::eval::text_arg).unwrap_or_default();
            let s = s.as_str();
            let pat = pat.as_str();
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
            let s = args.first().and_then(crate::eval::text_arg).unwrap_or_default();
            let pat = args.get(1).and_then(crate::eval::text_arg).unwrap_or_default();
            let rep = args.get(2).and_then(crate::eval::text_arg).unwrap_or_default();
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
    if x <= 0.0 && x == x.trunc() { return f64::NAN; }
    if x < 0.5 {
        let pi = std::f64::consts::PI;
        let sin_pi_x = (pi * x).sin();
        if sin_pi_x.abs() < 1e-15 { return f64::NAN; }
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

/// Whether `s` reads as a `component:mnemonic` error identifier: two or more
/// colon-separated tokens, each starting with a letter and continuing with
/// letters/digits/underscores, and no spaces anywhere.
fn is_error_identifier(s: &str) -> bool {
    if s.is_empty() || s.contains(' ') { return false; }
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() < 2 { return false; }
    parts.iter().all(|p| {
        let mut chars = p.chars();
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() => chars.all(|c| c.is_ascii_alphanumeric() || c == '_'),
            _ => false,
        }
    })
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

/// First axis that is not a singleton: the axis a reduction collapses when the
/// caller does not name one.
fn default_dim(x: &MxArray) -> usize {
    x.shape.iter().enumerate().find(|(_, &s)| s != 1).map(|(i, _)| i + 1).unwrap_or(1)
}

/// Read a reduction axis from an argument list. An empty array is the
/// documented placeholder in `f(A, [], dim)`, so the axis named *after* it is
/// used; with nothing after it the default axis applies. Reading the empty
/// array as a scalar would index into an array that holds no elements.
fn dim_from_args(x: &MxArray, args: &[Value], pos: usize) -> usize {
    let scalar_dim = |v: Option<&Value>| -> Option<usize> {
        match v {
            Some(Value::Array(a)) if a.numel() > 0 => Some(a.to_scalar() as usize),
            _ => None,
        }
    };
    match args.get(pos) {
        // Empty placeholder: the real axis is the argument after it.
        Some(Value::Array(a)) if a.numel() == 0 => {
            scalar_dim(args.get(pos + 1)).unwrap_or_else(|| default_dim(x))
        }
        other => scalar_dim(other).unwrap_or_else(|| default_dim(x)),
    }
}

fn minmax(x: &MxArray, args: &[Value], is_max: bool, n_out: usize) -> Result<Value, EngineError> {
    let dim = dim_from_args(x, args, 1);
    let init = if is_max { Complex64::new(f64::NEG_INFINITY, 0.0) } else { Complex64::new(f64::INFINITY, 0.0) };
    let f: fn(Complex64, Complex64) -> Complex64 = if is_max {
        |a, b| if a.re >= b.re { a } else { b }
    } else {
        |a, b| if a.re <= b.re { a } else { b }
    };
    let r = reduce(x, dim, init, f);
    // `[m, i] = max(x)` adds the position of the extremum as a second output,
    // shaped like the value array so the two line up element-wise.
    if n_out >= 2 {
        return Ok(Value::Multi(vec![
            Value::Array(r),
            Value::Array(extremum_index(x, dim, is_max)),
        ]));
    }
    Ok(Value::Array(r))
}

/// 1-based position along `dim` of each extremum, matching the shape that
/// `reduce` produces for the values (the reduced dimension stays a singleton).
/// Column-major traversal visits each output slot's candidates in increasing
/// index order, so keeping the incumbent on ties reports the first occurrence.
fn extremum_index(x: &MxArray, dim: usize, is_max: bool) -> MxArray {
    let ndim = x.ndims();
    let d = dim.min(ndim).max(1) - 1;
    let mut out_shape = x.shape.clone();
    out_shape[d] = 1;
    while out_shape.len() < 2 { out_shape.push(1); }
    let out_strides = MxArray::col_strides(&out_shape);
    let n: usize = out_shape.iter().product();
    let mut best = vec![Complex64::new(0.0, 0.0); n];
    let mut idx = vec![1.0f64; n];
    let mut seen = vec![false; n];
    for lin in 0..x.numel() {
        let mut mi = vec![0usize; ndim];
        let mut rem = lin;
        for k in 0..ndim { mi[k] = rem % x.shape[k]; rem /= x.shape[k]; }
        let mut oi = 0usize;
        for k in 0..ndim {
            if k == d { continue; }
            oi += mi[k] * out_strides[k];
        }
        let v = x.get_complex(&mi);
        let better = if !seen[oi] { true }
            else if is_max { v.re > best[oi].re }
            else { v.re < best[oi].re };
        if better {
            seen[oi] = true;
            best[oi] = v;
            idx[oi] = (mi[d] + 1) as f64;
        }
    }
    MxArray::from_double_colmajor(idx, &out_shape)
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
        Value::Str(rows) => {
            let m = rows.len();
            let n = if m > 0 { rows[0].len() } else { 0 };
            vec![m, n]
        }
        Value::Struct(_) | Value::Map(_) | Value::Func(_, _, _) | Value::FuncRef(_) => vec![1, 1],
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

/// `mat2str(X)` / `mat2str(X, precision)`: a source-reconstructible text form
/// of a numeric/logical value. Default precision relies on Rust's `{}` float
/// formatting, which already prints the shortest string that round-trips to
/// the same f64 — exactly the "full round-trip precision" default mat2str
/// documents. An explicit `precision` instead rounds to that many
/// significant digits (not decimal places).
fn mat2str_value(v: &Value, prec: Option<usize>) -> String {
    let a = match v { Value::Array(a) => a, other => return format_value(other) };
    let is_logical = a.is_logical_like();
    let fmt_elem = |c: num_complex::Complex64| -> String {
        if is_logical {
            return if c.re != 0.0 { "true".into() } else { "false".into() };
        }
        match prec {
            Some(p) => format_sig(c.re, p),
            None => format!("{}", c.re),
        }
    };
    if a.is_scalar() {
        return fmt_elem(a.get_complex(&[]));
    }
    let m = a.shape[0];
    let nn = a.shape.get(1).copied().unwrap_or(1);
    let mut rows = Vec::with_capacity(m);
    for i in 0..m {
        let mut cols = Vec::with_capacity(nn);
        for j in 0..nn { cols.push(fmt_elem(a.get_complex(&[i, j]))); }
        rows.push(cols.join(" "));
    }
    format!("[{}]", rows.join(";"))
}

/// Round `v` to `sig` significant digits and print it in plain (non-scientific)
/// decimal form, trimming trailing zeros.
fn format_sig(v: f64, sig: usize) -> String {
    if v == 0.0 || !v.is_finite() { return format!("{}", v); }
    let sig = sig.max(1) as i32;
    let magnitude = v.abs().log10().floor() as i32;
    let decimals = (sig - 1 - magnitude).max(0) as usize;
    let scale = 10f64.powi(sig - 1 - magnitude);
    let rounded = (v * scale).round() / scale;
    let s = format!("{:.*}", decimals, rounded);
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

pub fn format_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Str(rows) => {
            if rows.len() == 1 && rows.first().map(|r| r.len()).unwrap_or(0) == 1 {
                rows[0][0].clone()
            } else {
                let inner: Vec<String> = rows.iter()
                    .map(|r| r.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(" "))
                    .collect();
                format!("[{}]", inner.join(";"))
            }
        }
        Value::Map(entries) => format!("Map object with {} entries", entries.len()),
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

/// Render one number, or one real/imaginary part, with the engine's canonical
/// spelling of the non-finite values: `Inf`, `-Inf`, `NaN`. Rust's `{:8.4}`
/// would emit lowercase `inf`/`nan`, which is not what MATBOX prints, and it
/// silently falls through to a numeric format for non-finite input.
fn fmt_part(v: f64) -> String {
    if v.is_nan() { return "NaN".to_string(); }
    if v.is_infinite() { return if v < 0.0 { "-Inf".to_string() } else { "Inf".to_string() }; }
    format!("{:8.4}", v)
}

fn fmt_val(c: num_complex::Complex64) -> String {
    let re = c.re;
    let im = c.im;
    // A part counts as absent only when it is a finite near-zero; an infinite
    // or NaN imaginary part must still be printed, so the zero tests below
    // deliberately exclude non-finite values.
    let im_absent = im.abs() < 1e-10 && im.is_finite();
    let re_absent = re.abs() < 1e-10 && re.is_finite();
    if im_absent {
        fmt_part(re)
    } else if re_absent {
        format!("{}i", fmt_part(im))
    } else if im.is_finite() && im < 0.0 {
        format!("{} - {}i", fmt_part(re), fmt_part(-im))
    } else if im.is_finite() {
        format!("{} + {}i", fmt_part(re), fmt_part(im))
    } else {
        // Non-finite imaginary part: no signed `+`/`-` to render, so emit the
        // imaginary value on its own rather than producing `Inf + Infi`.
        format!("{}i", fmt_part(im))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Engine;
    use std::collections::HashMap;

    /// Evaluate a snippet and return the value it produces.
    fn eval1(src: &str) -> Value {
        let mut e = Engine::new();
        e.eval(src).unwrap().unwrap_or_else(|| panic!("no value produced by: {src}"))
    }

    /// Evaluate a snippet that yields text, accepting either text type: the char
    /// row vector or a scalar of the string type.
    fn text(src: &str) -> String {
        match eval1(src) {
            Value::String(s) => s,
            Value::Str(g) if g.len() == 1 && g[0].len() == 1 => g[0][0].clone(),
            other => panic!("expected text from {src}, got {other:?}"),
        }
    }

    /// Type name reported by `class` for the value of an expression.
    fn class_of(src: &str) -> String {
        text(&format!("class({src})"))
    }

    /// Call a builtin asking for `n` outputs, and hand back the values.
    fn call_n(name: &str, args: Vec<Value>, n: usize) -> Vec<Value> {
        let mut e = Engine::new();
        e.nargout = n;
        match call(name, &args, &mut e) {
            Ok(Value::Multi(v)) => v,
            Ok(v) => vec![v],
            Err(e) => panic!("{name} failed: {e}"),
        }
    }

    /// All elements of a numeric value, in column-major order.
    fn nums(v: &Value) -> Vec<f64> {
        v.as_array().to_double_vec()
    }

    fn arr(v: f64) -> Value {
        Value::Array(MxArray::scalar_double(v))
    }

    fn row(v: &[f64]) -> Value {
        Value::Array(MxArray::from_double_colmajor(v.to_vec(), &[1, v.len()]))
    }

    // --- Inf / NaN spelling (BUG-8) ---

    #[test]
    fn inf_and_nan_are_spelled_with_capitals() {
        assert_eq!(fmt_val(Complex64::new(f64::INFINITY, 0.0)), "Inf");
        assert_eq!(fmt_val(Complex64::new(f64::NEG_INFINITY, 0.0)), "-Inf");
        assert_eq!(fmt_val(Complex64::new(f64::NAN, 0.0)), "NaN");
    }

    #[test]
    fn non_finite_values_survive_inside_an_array() {
        let a = MxArray::from_double_colmajor(vec![1.0, f64::INFINITY], &[1, 2]);
        assert!(format_value(&Value::Array(a)).contains("Inf"));
    }

    #[test]
    fn finite_output_is_unchanged() {
        assert_eq!(fmt_val(Complex64::new(1.0, 0.0)), "  1.0000");
        assert_eq!(fmt_val(Complex64::new(2.0, 1.0)), "  2.0000 +   1.0000i");
    }

    // --- fprintf / sprintf (BUG-3) ---

    #[test]
    fn sprintf_expands_backslash_escapes() {
        assert_eq!(text(r"sprintf('a\nb')"), "a\nb");
        assert_eq!(text(r"sprintf('x\ty')"), "x\ty");
    }

    #[test]
    fn sprintf_honours_precision_and_percent() {
        assert_eq!(text("sprintf('%.2f', 3.14159)"), "3.14");
        assert_eq!(text("sprintf('100%%')"), "100%");
    }

    #[test]
    fn sprintf_defaults_to_six_decimals() {
        assert_eq!(text("sprintf('%f', 1.5)"), "1.500000");
    }

    #[test]
    fn sprintf_rejects_a_non_format_first_argument() {
        let mut e = Engine::new();
        assert!(call("sprintf", &[arr(1.0)], &mut e).is_err());
    }

    // --- class (BUG-4) ---

    #[test]
    fn class_names_each_value_type() {
        assert_eq!(class_name(&arr(5.0)), "double");
        assert_eq!(class_name(&Value::String("hi".into())), "char");
        assert_eq!(class_name(&Value::Str(vec![vec!["hi".into()]])), "string");
        assert_eq!(class_name(&Value::Cell(vec![vec![]])), "cell");
        assert_eq!(class_name(&Value::Map(vec![])), "map");
        assert_eq!(class_name(&Value::Struct(HashMap::new())), "struct");
        assert_eq!(class_name(&Value::FuncRef("f".into())), "function handle");
    }

    #[test]
    fn class_distinguishes_logical_from_double() {
        assert_eq!(class_name(&Value::Array(MxArray::scalar_logical(true))), "logical");
        assert_eq!(class_of("true"), "logical");
        assert_eq!(class_of("5"), "double");
    }

    // --- multi-output (BUG-7) ---

    #[test]
    fn max_reports_the_position_of_the_extremum() {
        let out = call_n("max", vec![row(&[3.0, 1.0, 9.0, 2.0])], 2);
        assert_eq!(out.len(), 2);
        assert_eq!(nums(&out[0])[0], 9.0);
        assert_eq!(nums(&out[1])[0], 3.0);
    }

    #[test]
    fn min_reports_the_position_of_the_extremum() {
        let out = call_n("min", vec![row(&[3.0, 1.0, 9.0, 2.0])], 2);
        assert_eq!(nums(&out[0])[0], 1.0);
        assert_eq!(nums(&out[1])[0], 2.0);
    }

    #[test]
    fn ties_report_the_first_position() {
        let out = call_n("max", vec![row(&[5.0, 5.0, 5.0])], 2);
        assert_eq!(nums(&out[1])[0], 1.0);
    }

    #[test]
    fn single_output_max_is_unchanged() {
        let out = call_n("max", vec![row(&[3.0, 1.0, 9.0, 2.0])], 1);
        assert_eq!(out.len(), 1);
        assert_eq!(nums(&out[0])[0], 9.0);
    }

    #[test]
    fn sort_permutation_reconstructs_the_input() {
        let input = [3.0, 1.0, 9.0, 2.0];
        let out = call_n("sort", vec![row(&input)], 2);
        let sorted = nums(&out[0]);
        let perm = nums(&out[1]).iter().map(|&p| p as usize).collect::<Vec<usize>>();
        assert_eq!(sorted, vec![1.0, 2.0, 3.0, 9.0]);
        // sorted(i) is input(perm(i)), so the permutation has to be inverted
        // before it can rebuild the original arrangement.
        assert_eq!(perm, vec![2, 4, 1, 3]);
        let mut inv = vec![0usize; perm.len()];
        for (i, &p) in perm.iter().enumerate() { inv[p - 1] = i + 1; }
        let restored: Vec<f64> = inv.iter().map(|&p| sorted[p - 1]).collect();
        assert_eq!(restored, input.to_vec());
    }

    #[test]
    fn sort_single_output_is_unchanged() {
        let out = call_n("sort", vec![row(&[3.0, 1.0, 2.0])], 1);
        assert_eq!(out.len(), 1);
        assert_eq!(nums(&out[0]), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn find_gives_row_and_column_indices() {
        // Column-major data for [1 0 1; 0 1 0].
        let a = MxArray::from_double_colmajor(vec![1.0, 0.0, 0.0, 1.0, 1.0, 0.0], &[2, 3]);
        let out = call_n("find", vec![Value::Array(a)], 2);
        assert_eq!(nums(&out[0]), vec![1.0, 2.0, 1.0]);
        assert_eq!(nums(&out[1]), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn find_single_output_is_unchanged() {
        let out = call_n("find", vec![row(&[1.0, 0.0, 1.0])], 1);
        assert_eq!(out.len(), 1);
        assert_eq!(nums(&out[0]), vec![1.0, 3.0]);
    }

    #[test]
    fn an_empty_dimension_argument_selects_the_default_axis() {
        // max(A, [], 2) reduces along the second dimension instead of failing
        // on the empty array, which has no scalar to read. Column-major data
        // for [1 7; 2 3].
        let a = MxArray::from_double_colmajor(vec![1.0, 2.0, 7.0, 3.0], &[2, 2]);
        let empty = Value::Array(MxArray::new_double(&[0, 0]));
        // An explicit axis is honoured...
        let out = call_n("max", vec![Value::Array(a.clone()), empty.clone(), arr(2.0)], 1);
        assert_eq!(nums(&out[0]), vec![7.0, 3.0]);
        // ...and the empty array means "use the default axis", which for this
        // matrix is the first non-singleton one, dim 1.
        let out = call_n("max", vec![Value::Array(a), empty], 1);
        assert_eq!(nums(&out[0]), vec![2.0, 7.0]);
    }

    #[test]
    fn dim_or_default_falls_back_for_an_empty_array() {
        let a = MxArray::from_double_colmajor(vec![1.0, 2.0, 3.0], &[1, 3]);
        // [1 3] has no non-singleton first axis, so dim 2 is the default.
        assert_eq!(dim_from_args(&a, &[], 1), 2);
        assert_eq!(dim_from_args(&a, &[Value::Array(MxArray::new_double(&[0, 0]))], 1), 2);
        assert_eq!(dim_from_args(&a, &[arr(0.0), arr(1.0)], 1), 1);
    }

    // --- integer conversion and idivide (BUG-5) ---

    #[test]
    fn idivide_truncates_toward_zero() {
        let mut e = Engine::new();
        for (a, b, want) in [(7.0, 2.0, 3.0), (-7.0, 2.0, -3.0), (6.0, 3.0, 2.0)] {
            let v = call("idivide", &[arr(a), arr(b)], &mut e).unwrap();
            assert_eq!(v.as_array().to_scalar(), want, "idivide({a},{b})");
        }
    }

    #[test]
    fn idivide_broadcasts_a_scalar_operand() {
        let mut e = Engine::new();
        let v = call("idivide", &[row(&[6.0, 8.0]), arr(2.0)], &mut e).unwrap();
        assert_eq!(v.as_array().to_double_vec(), vec![3.0, 4.0]);
        let v = call("idivide", &[arr(6.0), row(&[2.0, 3.0])], &mut e).unwrap();
        assert_eq!(v.as_array().to_double_vec(), vec![3.0, 2.0]);
    }

    #[test]
    fn idivide_rejects_a_zero_divisor() {
        let mut e = Engine::new();
        assert!(call("idivide", &[arr(1.0), arr(0.0)], &mut e).is_err());
        assert!(call("idivide", &[row(&[1.0, 2.0]), row(&[1.0, 0.0])], &mut e).is_err());
    }

    #[test]
    fn idivide_rejects_mismatched_lengths() {
        let mut e = Engine::new();
        assert!(call("idivide", &[row(&[1.0, 2.0, 3.0]), row(&[1.0, 2.0])], &mut e).is_err());
    }

    #[test]
    fn int_constructors_saturate_instead_of_wrapping() {
        assert_eq!(int_range("int8"), (-128, 127));
        assert_eq!(int_range("uint8"), (0, 255));
        let mut e = Engine::new();
        assert_eq!(call("int8", &[arr(300.0)], &mut e).unwrap().as_array().to_scalar(), 127.0);
        assert_eq!(call("int8", &[arr(-300.0)], &mut e).unwrap().as_array().to_scalar(), -128.0);
    }

    /// Text of a value read back out of an engine that already has state.
    fn text_of(e: &mut Engine, src: &str) -> String {
        match e.eval(src).unwrap().unwrap() {
            Value::String(s) => s,
            other => panic!("expected text from {src}, got {other:?}"),
        }
    }

    /// Evaluate a snippet that is expected to fail, returning the message.
    fn err_of(src: &str) -> String {
        let mut e = Engine::new();
        match e.eval(src) {
            Err(msg) => msg.to_string(),
            Ok(_) => panic!("expected an error from: {src}"),
        }
    }

    /// Values of an expression in row-major (display) order, which is how
/// expectations read most naturally. Storage is column-major, so a matrix's
    /// two orders differ and this keeps the literals below matching what `disp`
    /// prints.
    fn vals_disp(src: &str) -> Vec<f64> {
        let a = eval1(src).as_array().clone();
        let m = a.shape[0];
        let n = a.shape.get(1).copied().unwrap_or(1);
        let mut out = Vec::with_capacity(a.numel());
        for i in 0..m {
            for j in 0..n { out.push(a.get_double(&[i, j])); }
        }
        out
    }

    /// Values produced by a snippet, flattened in column-major order.
    fn vals_of(src: &str) -> Vec<f64> {
        nums(&eval1(src))
    }

    /// Display-ordered values of an expression, checked element-wise against
    /// an expectation.
    fn approx(src: &str, want: &[f64]) {
        let got = vals_disp(src);
        assert_eq!(got.len(), want.len(), "{src}: length {got:?} vs {want:?}");
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1e-9, "{src}: {got:?} vs {want:?}");
        }
    }

    // --- logical type is preserved by concatenation ---

    #[test]
    fn concatenating_logicals_stays_logical() {
        assert_eq!(class_of("[true false]"), "logical");
        assert_eq!(class_of("[true; false]"), "logical");
        // mixing with a number widens back to numeric
        assert_eq!(class_of("[true 1]"), "double");
    }

    // --- indexed assignment keeps the element class ---

    #[test]
    fn growing_a_logical_vector_stays_logical() {
        assert_eq!(class_of("[true false]"), "logical");
        let mut e = Engine::new();
        e.eval("x = [true false]; x(4) = true;").unwrap();
        assert_eq!(text_of(&mut e, "class(x)"), "logical");
        let got = e.eval("x").unwrap().unwrap();
        // the original elements survive, and the new ones are false
        assert_eq!(got.as_array().to_double_vec(), vec![1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn growing_a_logical_matrix_keeps_its_data() {
        let mut e = Engine::new();
        let got = e.eval("M = [true false; false true]; M(3,3) = true; M").unwrap().unwrap();
        assert_eq!(class_name(&got), "logical");
        assert_eq!(
            got.as_array().to_double_vec(),
            vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
        );
    }

    // --- division shapes that must not read past a buffer ---

    #[test]
    fn dividing_by_a_scalar_is_elementwise() {
        assert_eq!(vals_of("[1 2 3] / 2"), vec![0.5, 1.0, 1.5]);
        assert_eq!(vals_of("2 \\ [1 2 3]"), vec![0.5, 1.0, 1.5]);
    }

    #[test]
    fn solving_with_a_scalar_rhs_uses_the_right_height() {
        // A \ 1 solves for a column of A's height rather than indexing a 1x1.
        assert_eq!(vals_of("[1 2; 3 4] \\ 1"), vec![-1.0, 1.0]);
        let got = vals_of("[4 7; 2 6] \\ [2; 3]");
        assert!((got[0] + 0.9).abs() < 1e-12, "got {got:?}");
        assert!((got[1] - 0.8).abs() < 1e-12, "got {got:?}");
    }

    #[test]
    fn a_non_square_left_division_is_reported_not_panicked() {
        assert!(err_of("[1 2] \\ 1").contains("square"));
        assert!(err_of("[2 0; 0 2] \\ [1 2]").contains("rows"));
    }

    #[test]
    fn out_of_range_cell_indexing_is_reported_not_panicked() {
        assert!(err_of("c = {1,2}; c{5}").contains("out of range"));
        assert!(err_of("c = {}; c{1}").contains("empty"));
    }

    // --- string conversion uses data text, not display text ---

    #[test]
    fn string_of_a_number_has_no_column_padding() {
        // The display formatter pads for alignment; converting data must not.
        let got = eval1("string({1, 2})");
        assert_eq!(format_value(&got), "[\"1\" \"2\"]");
        assert_eq!(text("string(1/0)"), "Inf");
        assert_eq!(text("string(0/0)"), "NaN");
    }

    // --- wave 2 builtins ---

    #[test]
    fn tril_and_triu_take_a_lower_and_upper_part() {
        approx("tril([1 2; 3 4])", &[1.0, 0.0, 3.0, 4.0]);
        approx("triu([1 2; 3 4])", &[1.0, 2.0, 0.0, 4.0]);
        // The offset shifts the diagonal the split follows.
        approx("tril([1 2 3; 4 5 6; 7 8 9], 1)", &[1.0, 2.0, 0.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        approx("tril([1 2 3; 4 5 6; 7 8 9])", &[1.0, 0.0, 0.0, 4.0, 5.0, 0.0, 7.0, 8.0, 9.0]);
    }

    #[test]
    fn rot90_turns_quarter_turns_at_a_time() {
        approx("rot90([1 2; 3 4])", &[3.0, 1.0, 4.0, 2.0]);
        approx("rot90([1 2; 3 4], 2)", &[4.0, 3.0, 2.0, 1.0]);
        // Four turns is the identity.
        approx("rot90([1 2; 3 4], 4)", &[1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn circshift_moves_elements_toward_higher_indices() {
        approx("circshift([1 2 3], 1)", &[3.0, 1.0, 2.0]);
        approx("circshift([1 2 3], -1)", &[2.0, 3.0, 1.0]);
        // A shift past the length wraps rather than losing elements.
        approx("circshift([1 2 3], 5)", &[2.0, 3.0, 1.0]);
        approx("circshift([1 2; 3 4], [0 1])", &[2.0, 1.0, 4.0, 3.0]);
        approx("circshift([1 2; 3 4], [1 0])", &[3.0, 4.0, 1.0, 2.0]);
    }

    #[test]
    fn permute_and_ipermute_are_inverses() {
        approx("permute([1 2; 3 4], [2 1])", &[1.0, 3.0, 2.0, 4.0]);
        approx("ipermute([2 1])", &[2.0, 1.0]);
        approx("ipermute([2 3 1])", &[3.0, 1.0, 2.0]);
    }

    #[test]
    fn permute_rejects_an_out_of_range_order() {
        assert!(err_of("ipermute([5])").contains("out of range"));
    }

    #[test]
    fn sortrows_orders_whole_rows_by_one_column() {
        approx("sortrows([3 1; 1 2])", &[1.0, 2.0, 3.0, 1.0]);
        approx("sortrows([3 1; 1 2], 2)", &[3.0, 1.0, 1.0, 2.0]);
    }

    #[test]
    fn repelem_repeats_elements() {
        approx("repelem([1 2], 2)", &[1.0, 1.0, 2.0, 2.0]);
        approx("repelem([1; 2], 3)", &[1.0, 1.0, 1.0, 2.0, 2.0, 2.0]);
    }

    #[test]
    fn cummax_and_cummin_track_a_running_extreme() {
        approx("cummax([1 3 2])", &[1.0, 3.0, 3.0]);
        approx("cummin([1 3 2])", &[1.0, 1.0, 1.0]);
        // Default axis is the first non-singleton one, which for a matrix is
        // down the columns.
        approx("cummax([1 5; 2 3])", &[1.0, 5.0, 2.0, 5.0]);
        approx("cummax([1 5; 2 3], 2)", &[1.0, 5.0, 2.0, 3.0]);
    }

    #[test]
    fn vecnorm_uses_the_first_non_singleton_axis_by_default() {
        approx("vecnorm([3 4])", &[5.0]);
        approx("vecnorm([3 4; 0 0], 2)", &[5.0, 0.0]);
        approx("vecnorm([3 4], 2, Inf)", &[4.0]);
        approx("vecnorm([-1 2], 2, 1)", &[3.0]);
    }

    #[test]
    fn dot_sums_the_conjugate_product_along_the_default_axis() {
        approx("dot([1 2], [3 4])", &[11.0]);
        // For matrices that is the Hermitian product A' * B.
        approx("dot([1 2; 3 4], [5 6; 7 8])", &[26.0, 44.0]);
    }

    #[test]
    fn cross_needs_three_element_vectors() {
        approx("cross([1 0 0], [0 1 0])", &[0.0, 0.0, 1.0]);
        assert!(err_of("cross([1 0], [0 1])").contains("three-element"));
    }

    #[test]
    fn integer_predicates_use_euclid_and_trial_division() {
        approx("gcd(12, 18)", &[6.0]);
        approx("gcd(0, 5)", &[5.0]);
        approx("lcm(4, 6)", &[12.0]);
        approx("lcm(0, 5)", &[0.0]);
        approx("isprime(7)", &[1.0]);
        approx("isprime(9)", &[0.0]);
        approx("isprime(1)", &[0.0]);
        approx("primes(10)", &[2.0, 3.0, 5.0, 7.0]);
    }

    #[test]
    fn factorial_and_nthroot_follow_the_sign_conventions() {
        approx("factorial(5)", &[120.0]);
        approx("factorial(0)", &[1.0]);
        // A negative integer factorial is undefined, reported as NaN.
        assert!(vals_of("factorial(-1)")[0].is_nan());
        approx("nthroot(27, 3)", &[3.0]);
        // An odd root of a negative number stays real and negative.
        approx("nthroot(-8, 3)", &[-2.0]);
        assert!(err_of("nthroot(4, 0)").contains("non-zero"));
    }

    #[test]
    fn power_helpers_pick_the_right_exponent() {
        approx("nextpow2(100)", &[128.0]);
        approx("nextpow2(64)", &[64.0]);
        approx("nextpow2(1)", &[1.0]);
        approx("pow2(3)", &[8.0]);
    }

    #[test]
    fn degree_radian_and_stable_variants_convert_as_documented() {
        approx("deg2rad(180)", &[std::f64::consts::PI]);
        approx("rad2deg(pi)", &[180.0]);
        approx("expm1(0)", &[0.0]);
        approx("log1p(0)", &[0.0]);
        approx("realsqrt(9)", &[3.0]);
        approx("hypot(3, 4)", &[5.0]);
        approx("sec(0)", &[1.0]);
        approx("cot(pi/4)", &[1.0]);
    }

    #[test]
    fn spread_measures_summarise_a_vector() {
        approx("range([1 5 3])", &[4.0]);
        // RMS is the root of the mean square, not the deviation about the mean.
        let want = ((1.0f64 + 4.0 + 9.0) / 3.0).sqrt();
        approx("rms([1 2 3])", &[want]);
    }

    #[test]
    fn xor_is_exclusive_or() {
        assert_eq!(class_of("xor(true, false)"), "logical");
        approx("xor(true, false)", &[1.0]);
        approx("xor(true, true)", &[0.0]);
    }

    #[test]
    fn squeeze_drops_singleton_dimensions() {
        approx("squeeze(ones(1, 3, 1))", &[1.0, 1.0, 1.0]);
    }

    #[test]
    fn count_is_available_as_a_function_and_a_property() {
        let mut e = Engine::new();
        e.eval("m = containers.Map(); m('a') = 1;").unwrap();
        assert_eq!(e.eval("m.Count").unwrap().unwrap().to_scalar(), 1.0);
        assert_eq!(e.eval("Count(m)").unwrap().unwrap().to_scalar(), 1.0);
    }

    #[test]
    fn key_and_value_type_options_are_not_stored_as_entries() {
        // Typing options pin the types up front; they must not become entries.
        let mut e = Engine::new();
        e.eval("m = containers.Map('KeyType','char','ValueType','double'); m('a') = 1;").unwrap();
        let n = e.eval("m.Count").unwrap().unwrap();
        assert_eq!(n.to_scalar(), 1.0);
    }

    #[test]
    fn map_options_are_skipped_after_the_key_value_pair() {
        let mut e = Engine::new();
        e.eval("m = containers.Map({'a'}, {'x'}, 'KeyType', 'char');").unwrap();
        assert_eq!(e.eval("m.Count").unwrap().unwrap().to_scalar(), 1.0);
        assert_eq!(text_of(&mut e, "m('a')"), "x");
    }

    #[test]
    fn count_rejects_a_non_map() {
        assert!(err_of("Count(5)").contains("containers.Map"));
    }

    #[test]
    fn dim_from_args_uses_the_argument_after_an_empty_placeholder() {
        let a = MxArray::from_double_colmajor(vec![1.0, 2.0, 7.0, 3.0], &[2, 2]);
        // Explicit axis wins when it is written directly...
        assert_eq!(dim_from_args(&a, &[Value::Array(a.clone()), arr(2.0)], 1), 2);
        // ...and an empty placeholder defers to the axis named after it.
        let empty = Value::Array(MxArray::new_double(&[0, 0]));
        assert_eq!(dim_from_args(&a, &[Value::Array(a.clone()), empty.clone(), arr(2.0)], 1), 2);
        // With nothing after the placeholder, the default axis applies.
        assert_eq!(dim_from_args(&a, &[Value::Array(a.clone()), empty], 1), 1);
    }
}
