//! Elementwise operations with implicit expansion (broadcasting), type
//! promotion, reductions, and matrix multiplication — all column-major.

use crate::array::{MxData, MxArray};
use crate::MathError;
use num_complex::Complex64;


/// Determine result shape by broadcasting two shapes (MATLAB implicit expansion).
pub fn broadcast_shapes(a: &[usize], b: &[usize]) -> Option<Vec<usize>> {
    let n = a.len().max(b.len());
    let mut out = vec![1usize; n];
    for k in 0..n {
        let da = if k < a.len() { a[a.len() - 1 - k] } else { 1 };
        let db = if k < b.len() { b[b.len() - 1 - k] } else { 1 };
        let d = if da == 1 { db } else if db == 1 { da } else if da == db { da } else { return None; };
        out[n - 1 - k] = d;
    }
    Some(out)
}

fn project_index(out_idx: &[usize], op_shape: &[usize], op_strides: &[usize]) -> usize {
    let n = op_shape.len();
    let mut lin = 0usize;
    for k in 0..n {
        let d = op_shape[k];
        let i = if d == 1 { 0 } else { out_idx[k] };
        lin += i * op_strides[k];
    }
    let _ = n;
    lin
}

/// Binary elementwise apply with broadcasting. Operates on complex values;
/// result is complex if either operand is complex.
pub fn binary_elementwise<F>(a: &MxArray, b: &MxArray, f: F) -> Result<MxArray, MathError>
where
    F: Fn(Complex64, Complex64) -> Complex64,
{
    let out_shape = broadcast_shapes(&a.shape, &b.shape)
        .ok_or_else(|| MathError::Msg(format!("matrix dimensions {}x{} and {}x{} mismatch for binary op", a.shape[0], a.shape.get(1).copied().unwrap_or(1), b.shape[0], b.shape.get(1).copied().unwrap_or(1))))?;
    let _out_strides = MxArray::col_strides(&out_shape);
    let n = out_shape.iter().product();
    let mut da = a.iter_complex();
    let mut db = b.iter_complex();
    // Precompute operand linear indices per output position via multi-index.
    let mut out = Vec::with_capacity(n);
    let mut idx = vec![0usize; out_shape.len()];
    for _ in 0..n {
        let la = project_index(&idx, &a.shape, &a.strides);
        let lb = project_index(&idx, &b.shape, &b.strides);
        out.push(f(da[la], db[lb]));
        // advance multi-index (column-major)
        for k in 0..out_shape.len() {
            idx[k] += 1;
            if idx[k] < out_shape[k] { break; }
            idx[k] = 0;
        }
    }
    let _ = (&mut da, &mut db);
    Ok(MxArray::from_complex_colmajor(out, &out_shape))
}

pub fn unary_elementwise<F>(a: &MxArray, f: F) -> MxArray
where
    F: Fn(Complex64) -> Complex64,
{
    let data = a.iter_complex().into_iter().map(f).collect();
    MxArray::from_complex_colmajor(data, &a.shape)
}

/// Matrix multiply. Supports 2-D matrices; result is column-major.
pub fn matmul(a: &MxArray, b: &MxArray) -> Result<MxArray, MathError> {
    let (ma, na) = (a.shape[0], a.shape.get(1).copied().unwrap_or(1));
    let (mb, nb) = (b.shape[0], b.shape.get(1).copied().unwrap_or(1));
    if na != mb {
        return Err(MathError::Msg(format!("inner matrix dimensions must agree: {}x{} times {}x{}", ma, na, mb, nb)));
    }
    let mut out = MxArray::new_complex(&[ma, nb]);
    for i in 0..ma {
        for j in 0..nb {
            let mut s = Complex64::new(0.0, 0.0);
            for k in 0..na {
                s += a.get_complex(&[i, k]) * b.get_complex(&[k, j]);
            }
            out.set_complex(&[i, j], s);
        }
    }
    Ok(out)
}

/// Sum (or other reduction) along a dimension. dim is 1-based MATLAB dimension.
pub fn reduce<F>(a: &MxArray, dim: usize, init: Complex64, f: F) -> MxArray
where
    F: Fn(Complex64, Complex64) -> Complex64,
{
    let ndim = a.ndims();
    let d = dim.min(ndim).max(1) - 1;
    let mut out_shape = a.shape.clone();
    out_shape[d] = 1;
    while out_shape.len() < 2 { out_shape.push(1); }
    let out_strides = MxArray::col_strides(&out_shape);
    let n = out_shape.iter().product();
    let mut out = vec![init; n];
    for lin in 0..a.numel() {
        let mut mi = vec![0usize; ndim];
        let mut rem = lin;
        for k in 0..ndim { mi[k] = rem % a.shape[k]; rem /= a.shape[k]; }
        let mut oi = 0usize;
        for k in 0..ndim {
            if k == d { continue; }
            oi += mi[k] * out_strides[k];
        }
        let v = match &a.data {
            MxData::Double(ref d) => Complex64::new(d[lin], 0.0),
            MxData::Complex(ref d) => d[lin],
            MxData::Logical(ref d) => Complex64::new(d[lin] as i32 as f64, 0.0),
            MxData::Int64(ref d) => Complex64::new(d[lin] as f64, 0.0),
        };
        out[oi] = f(out[oi], v);
    }
    MxArray::from_complex_colmajor(out, &out_shape)
}
