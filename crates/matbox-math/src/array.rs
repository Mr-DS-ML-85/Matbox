//! Column-major N-dimensional array (MATLAB-compatible storage).
//!
//! Linear index for a multi-index `(i0, i1, ..., i_{n-1})` is
//! `sum_k i_k * strides[k]` where `strides[k] = product(shape[0..k])`.
//! This is exactly MATLAB's column-major order, so `A(:)`, colon ranges,
//! and `reshape` preserve element order identically.

use num_complex::Complex64;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MxData {
    Double(Vec<f64>),
    Complex(Vec<Complex64>),
    Logical(Vec<bool>),
    Int64(Vec<i64>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MxArray {
    pub shape: Vec<usize>,
    pub strides: Vec<usize>,
    pub data: MxData,
}

impl MxArray {
    /// Column-major strides for a shape.
    pub fn col_strides(shape: &[usize]) -> Vec<usize> {
        let mut strides = vec![1usize; shape.len()];
        for k in 1..shape.len() {
            strides[k] = strides[k - 1] * shape[k - 1];
        }
        strides
    }

    pub fn numel(&self) -> usize {
        self.shape.iter().product()
    }

    pub fn ndims(&self) -> usize {
        self.shape.len()
    }

    pub fn is_scalar(&self) -> bool {
        self.numel() == 1
    }

    pub fn is_vector(&self) -> bool {
        self.shape.iter().filter(|&&d| d != 0).count() <= 1 || self.shape.contains(&1)
    }

    pub fn is_empty(&self) -> bool {
        self.numel() == 0
    }

    pub fn to_scalar(&self) -> f64 {
        self.get_double(&[])
    }

    pub fn new_double(shape: &[usize]) -> Self {
        let n = shape.iter().product();
        MxArray {
            shape: shape.to_vec(),
            strides: Self::col_strides(shape),
            data: MxData::Double(vec![0.0; n]),
        }
    }

    pub fn new_complex(shape: &[usize]) -> Self {
        let n = shape.iter().product();
        MxArray {
            shape: shape.to_vec(),
            strides: Self::col_strides(shape),
            data: MxData::Complex(vec![Complex64::new(0.0, 0.0); n]),
        }
    }

    pub fn new_logical(shape: &[usize]) -> Self {
        let n = shape.iter().product();
        MxArray {
            shape: shape.to_vec(),
            strides: Self::col_strides(shape),
            data: MxData::Logical(vec![false; n]),
        }
    }

    /// Build a column-major double array from a row/col-major source by
    /// supplying elements already in column-major order.
    pub fn from_logical_colmajor(elems: Vec<bool>, shape: &[usize]) -> Self {
        let mut shape = shape.to_vec();
        while shape.len() < 2 { shape.push(1); }
        let strides = MxArray::col_strides(&shape);
        MxArray { shape, strides, data: MxData::Logical(elems) }
    }

    pub fn from_double_colmajor(elems: Vec<f64>, shape: &[usize]) -> Self {
        MxArray {
            shape: shape.to_vec(),
            strides: Self::col_strides(shape),
            data: MxData::Double(elems),
        }
    }

    pub fn from_complex_colmajor(elems: Vec<Complex64>, shape: &[usize]) -> Self {
        MxArray {
            shape: shape.to_vec(),
            strides: Self::col_strides(shape),
            data: MxData::Complex(elems),
        }
    }

    pub fn scalar_double(v: f64) -> Self {
        MxArray {
            shape: vec![1, 1],
            strides: vec![1, 1],
            data: MxData::Double(vec![v]),
        }
    }

    pub fn scalar_complex(v: Complex64) -> Self {
        MxArray {
            shape: vec![1, 1],
            strides: vec![1, 1],
            data: MxData::Complex(vec![v]),
        }
    }

    pub fn scalar_logical(v: bool) -> Self {
        MxArray {
            shape: vec![1, 1],
            strides: vec![1, 1],
            data: MxData::Logical(vec![v]),
        }
    }

    pub fn scalar_int64(v: i64) -> Self {
        MxArray {
            shape: vec![1, 1],
            strides: vec![1, 1],
            data: MxData::Int64(vec![v]),
        }
    }

    /// Build an integer array from column-major element data. The engine stores
    /// every integer width in `MxData::Int64`, so narrowing conversions are the
    /// caller's responsibility (see the `int*` constructors in matbox-core,
    /// which saturate to the target range).
    pub fn from_int64_colmajor(v: Vec<i64>, shape: &[usize]) -> Self {
        let mut shape = shape.to_vec();
        while shape.len() < 2 { shape.push(1); }
        let strides = MxArray::col_strides(&shape);
        MxArray { shape, strides, data: MxData::Int64(v) }
    }

    /// Linear (0-based) index into the backing buffer for a 0-based multi-index.
    pub fn linear_index(&self, idx: &[usize]) -> usize {
        let mut lin = 0usize;
        for k in 0..self.ndims() {
            let i = if k < idx.len() { idx[k] } else { 0 };
            lin += i * self.strides[k];
        }
        lin
    }

    pub fn get_double(&self, idx: &[usize]) -> f64 {
        match &self.data {
            MxData::Double(d) => d[self.linear_index(idx)],
            MxData::Complex(d) => d[self.linear_index(idx)].re,
            MxData::Logical(d) => d[self.linear_index(idx)] as i32 as f64,
            MxData::Int64(d) => d[self.linear_index(idx)] as f64,
        }
    }

    pub fn get_complex(&self, idx: &[usize]) -> Complex64 {
        match &self.data {
            MxData::Double(d) => Complex64::new(d[self.linear_index(idx)], 0.0),
            MxData::Complex(d) => d[self.linear_index(idx)],
            MxData::Logical(d) => Complex64::new(d[self.linear_index(idx)] as i32 as f64, 0.0),
            MxData::Int64(d) => Complex64::new(d[self.linear_index(idx)] as f64, 0.0),
        }
    }

    pub fn set_double(&mut self, idx: &[usize], v: f64) {
        let lin = self.linear_index(idx);
        if let MxData::Double(d) = &mut self.data {
            d[lin] = v;
        }
    }

    pub fn set_complex(&mut self, idx: &[usize], v: Complex64) {
        let lin = self.linear_index(idx);
        if let MxData::Complex(d) = &mut self.data {
            d[lin] = v;
        }
    }

    pub fn is_complex(&self) -> bool {
        matches!(self.data, MxData::Complex(_))
    }

    pub fn is_logical_like(&self) -> bool {
        matches!(self.data, MxData::Logical(_))
    }

    /// Set element at a 0-based LINEAR buffer position.
    pub fn set_by_linear(&mut self, lin: usize, c: Complex64) {
        match &mut self.data {
            MxData::Double(d) => d[lin] = c.re,
            MxData::Complex(d) => d[lin] = c,
            MxData::Logical(d) => d[lin] = c.norm() != 0.0,
            MxData::Int64(d) => d[lin] = c.re as i64,
        }
    }

    /// Reshape preserving column-major element order (same backing buffer when
    /// possible). Falls back to a copy if the buffer is shared/owned separately.
    pub fn reshape(&self, new_shape: &[usize]) -> Self {
        assert_eq!(
            new_shape.iter().product::<usize>(),
            self.numel(),
            "reshape: number of elements must not change"
        );
        MxArray {
            shape: new_shape.to_vec(),
            strides: Self::col_strides(new_shape),
            data: self.data.clone(),
        }
    }

    /// Transpose (non-conjugate): swap shape and strides over the same buffer.
    pub fn transpose(&self) -> Self {
        let mut shape = self.shape.clone();
        shape.reverse();
        let _total = self.numel();
        let new_data = match &self.data {
            MxData::Double(d) => {
                let mut out = d.clone();
                let n = shape[0];
                let m = shape.get(1).copied().unwrap_or(1);
                for i in 0..n {
                    for j in 0..m {
                        let src = j + i * m;     // column-major in original (old shape [m,n])
                        let dst = i + j * n;     // column-major in result (new shape [n,m])
                        out[dst] = d[src];
                    }
                }
                MxData::Double(out)
            }
            MxData::Complex(d) => {
                let mut out = d.clone();
                let n = shape[0];
                let m = shape.get(1).copied().unwrap_or(1);
                for i in 0..n {
                    for j in 0..m {
                        let src = j + i * m;
                        let dst = i + j * n;
                        out[dst] = d[src];
                    }
                }
                MxData::Complex(out)
            }
            MxData::Logical(d) => {
                let mut out = d.clone();
                let n = shape[0];
                let m = shape.get(1).copied().unwrap_or(1);
                for i in 0..n {
                    for j in 0..m {
                        let src = j + i * m;
                        let dst = i + j * n;
                        out[dst] = d[src];
                    }
                }
                MxData::Logical(out)
            }
            MxData::Int64(d) => {
                let mut out = d.clone();
                let n = shape[0];
                let m = shape.get(1).copied().unwrap_or(1);
                for i in 0..n {
                    for j in 0..m {
                        let src = j + i * m;
                        let dst = i + j * n;
                        out[dst] = d[src];
                    }
                }
                MxData::Int64(out)
            }
        };
        let mut strides = shape.clone();
        let mut s = 1usize;
        for d in &mut strides { let t = *d; *d = s; s *= t; }
        MxArray { shape, strides, data: new_data }
    }

    /// Column-major flat iterator over complex values.
    pub fn iter_complex(&self) -> Vec<Complex64> {
        let n = self.numel();
        let mut out = Vec::with_capacity(n);
        for lin in 0..n {
            out.push(match &self.data {
                MxData::Double(d) => Complex64::new(d[lin], 0.0),
                MxData::Complex(d) => d[lin],
                MxData::Logical(d) => Complex64::new(d[lin] as i32 as f64, 0.0),
                MxData::Int64(d) => Complex64::new(d[lin] as f64, 0.0),
            });
        }
        out
    }

    pub fn to_double_vec(&self) -> Vec<f64> {
        match &self.data {
            MxData::Double(d) => d.clone(),
            MxData::Complex(d) => d.iter().map(|c| c.re).collect(),
            MxData::Logical(d) => d.iter().map(|b| *b as i32 as f64).collect(),
            MxData::Int64(d) => d.iter().map(|v| *v as f64).collect(),
        }
    }
}
