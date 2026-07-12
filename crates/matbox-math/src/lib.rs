//! MATBOX math: column-major array, elementwise ops, linear algebra.
//! Pure Rust. No platform dependencies.

#![allow(
    clippy::needless_range_loop,
    clippy::if_same_then_else
)]

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MathError {
    #[error("{0}")]
    Msg(String),
    #[error("dimension mismatch: {0}")]
    Dim(String),
}

pub mod array;
pub mod ops;
pub mod linalg;

pub use array::MxArray;
pub use ops::{binary_elementwise, matmul, reduce, unary_elementwise, broadcast_shapes};
pub use linalg::{chol, det, eig, inv, lu, norm, qr_complex, rank, solve, svd, trace};

/// Build a 2-D column-major double array from row-major nested input.
pub fn matrix_rm(rows: &[Vec<f64>]) -> MxArray {
    let m = rows.len();
    let n = rows.first().map(|r| r.len()).unwrap_or(0);
    let mut v = Vec::with_capacity(m * n);
    for j in 0..n {
        for i in 0..m {
            v.push(rows[i][j]);
        }
    }
    MxArray::from_double_colmajor(v, &[m, n])
}
