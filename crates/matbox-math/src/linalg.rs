//! Linear algebra: LU, QR, Cholesky, solve, inv, det, rank, norm, trace, eig, svd.
//! Pure Rust, column-major. Operates in complex arithmetic where needed.

use crate::array::MxArray;
use crate::MathError;
use num_complex::Complex64;

fn to_mat(a: &MxArray) -> Vec<Vec<Complex64>> {
    let m = a.shape[0];
    let n = a.shape.get(1).copied().unwrap_or(1);
    let mut mat = vec![vec![Complex64::new(0.0, 0.0); n]; m];
    for i in 0..m {
        for j in 0..n {
            mat[i][j] = a.get_complex(&[i, j]);
        }
    }
    mat
}

fn from_mat(mat: &[Vec<Complex64>]) -> MxArray {
    let m = mat.len();
    let n = if m > 0 { mat[0].len() } else { 0 };
    let mut v = Vec::with_capacity(m * n);
    for j in 0..n {
        for i in 0..m {
            v.push(mat[i][j]);
        }
    }
    MxArray::from_complex_colmajor(v, &[m, n])
}

pub fn trace(a: &MxArray) -> Complex64 {
    let n = a.shape[0].min(a.shape.get(1).copied().unwrap_or(1));
    let mut t = Complex64::new(0.0, 0.0);
    for i in 0..n {
        t += a.get_complex(&[i, i]);
    }
    t
}

pub fn norm(a: &MxArray, p: &str) -> Result<f64, MathError> {
    match p {
        "fro" | "2" => {
            let s: f64 = a.iter_complex().iter().map(|c| c.norm_sqr()).sum();
            Ok(s.sqrt())
        }
        "1" => {
            // max absolute column sum
            let n = a.shape.get(1).copied().unwrap_or(1);
            let mut best = 0.0f64;
            for j in 0..n {
                let mut s = 0.0;
                for i in 0..a.shape[0] {
                    s += a.get_complex(&[i, j]).norm();
                }
                best = best.max(s);
            }
            Ok(best)
        }
        "inf" => {
            let m = a.shape[0];
            let mut best = 0.0f64;
            for i in 0..m {
                let mut s = 0.0;
                for j in 0..a.shape.get(1).copied().unwrap_or(1) {
                    s += a.get_complex(&[i, j]).norm();
                }
                best = best.max(s);
            }
            Ok(best)
        }
        _ => Err(MathError::Msg(format!("norm: unsupported ord '{}'", p))),
    }
}

/// LU with partial pivoting. Returns (L, U, P) where P is a permutation vector
/// (P[i] = source row). A = L*U with row permutation.
pub fn lu(a: &MxArray) -> Result<(MxArray, MxArray, Vec<usize>), MathError> {
    let mut mat = to_mat(a);
    let m = mat.len();
    let n = if m > 0 { mat[0].len() } else { 0 };
    let k = m.min(n);
    let mut piv = (0..m).collect::<Vec<_>>();
    for j in 0..k {
        let mut p = j;
        let mut max = mat[j][j].norm();
        for i in (j + 1)..m {
            let v = mat[i][j].norm();
            if v > max { max = v; p = i; }
        }
        if max == 0.0 { continue; }
        if p != j {
            mat.swap(p, j);
            piv.swap(p, j);
        }
        for i in (j + 1)..m {
            let f = mat[i][j] / mat[j][j];
            mat[i][j] = f;
            for c in (j + 1)..n {
                let t = f * mat[j][c];
                mat[i][c] -= t;
            }
        }
    }
    let mut l = vec![vec![Complex64::new(0.0, 0.0); n]; m];
    let mut u = vec![vec![Complex64::new(0.0, 0.0); n]; m];
    for i in 0..m {
        for j in 0..n {
            if i > j {
                l[i][j] = mat[i][j];
            } else if i == j {
                l[i][j] = Complex64::new(1.0, 0.0);
                u[i][j] = mat[i][j];
            } else {
                u[i][j] = mat[i][j];
            }
        }
    }
    Ok((from_mat(&l), from_mat(&u), piv))
}

pub fn det(a: &MxArray) -> Result<Complex64, MathError> {
    let (_, u, piv) = lu(a)?;
    let mut d = Complex64::new(1.0, 0.0);
    let n = u.shape[0].min(u.shape.get(1).copied().unwrap_or(1));
    for i in 0..n {
        d *= u.get_complex(&[i, i]);
    }
    // Count row swaps: track cycles in pivot permutation
    let mut visited = vec![false; piv.len()];
    let mut swaps = 0usize;
    for i in 0..piv.len() {
        if !visited[i] && piv[i] != i {
            let mut j = i;
            while !visited[j] {
                visited[j] = true;
                j = piv[j];
                swaps += 1;
            }
            swaps -= 1; // each cycle of length k contributes k-1 swaps
        }
    }
    if swaps % 2 == 1 { d = -d; }
    Ok(d)
}

pub fn rank(a: &MxArray, tol: f64) -> usize {
    let (_, _, _) = match lu(a) { Ok(x) => x, Err(_) => return 0 };
    // Use singular values for rank; cheap eig of A^H A for small matrices.
    let sv = match svd(a) { Ok(s) => s.0, Err(_) => return 0 };
    let max_sv = sv.iter().cloned().fold(0.0f64, f64::max);
    let t = if tol <= 0.0 { max_sv * 1e-12 * sv.len().max(1) as f64 } else { tol };
    sv.iter().filter(|&&s| s > t).count()
}

/// Solve A x = B (B may be multiple RHS columns). Returns X.
pub fn solve(a: &MxArray, b: &MxArray) -> Result<MxArray, MathError> {
    let m = a.shape[0];
    let n = a.shape.get(1).copied().unwrap_or(1);
    // Shape validation up front: the substitution loops below index A, B and
    // the unknown vector by these extents, so a non-square or mismatched system
    // has to be rejected rather than read past the end of a buffer.
    if m != n {
        return Err(MathError::Msg("solve: matrix must be square".into()));
    }
    if m == 0 {
        return Err(MathError::Msg("solve: matrix must not be empty".into()));
    }
    // B supplies one right-hand side per column, except for a plain vector,
    // which is a single column of m rows.
    let b_is_vec = b.shape.len() <= 1 || (b.shape.len() == 2 && b.shape.iter().skip(1).all(|&s| s == 1));
    let b_rows = b.shape[0];
    let nrhs = if b_is_vec { 1 } else { b.shape.get(1).copied().unwrap_or(1) };
    if b_rows != m {
        return Err(MathError::Msg(
            "solve: right-hand side must have as many rows as the matrix".into()));
    }
    // Read one right-hand side at a time as a column of m entries.
    let rhs_at = |r: usize, i: usize| -> Complex64 {
        if b_is_vec { b.get_complex(&[i]) } else { b.get_complex(&[i, r]) }
    };
    let (l, u, piv) = lu(a)?;
    let mut x = vec![vec![Complex64::new(0.0, 0.0); nrhs]; n];
    for r in 0..nrhs {
        // permute b
        let mut pb = vec![Complex64::new(0.0, 0.0); m];
        for i in 0..m { pb[i] = rhs_at(r, piv[i]); }
        // forward substitute L y = pb
        let mut y = vec![Complex64::new(0.0, 0.0); m];
        for i in 0..m {
            let mut s = pb[i];
            for j in 0..i { s -= l.get_complex(&[i, j]) * y[j]; }
            y[i] = s; // L[i,i] = 1
        }
        // backward substitute U x = y
        for i in (0..n).rev() {
            let mut s = y[i];
            for j in (i + 1)..n { s -= u.get_complex(&[i, j]) * x[j][r]; }
            let diag = u.get_complex(&[i, i]);
            if diag.norm() == 0.0 { return Err(MathError::Msg("solve: singular matrix".into())); }
            x[i][r] = s / diag;
        }
    }
    Ok(from_mat(&x))
}

pub fn inv(a: &MxArray) -> Result<MxArray, MathError> {
    let n = a.shape[0];
    let eye = MxArray::new_complex(&[n, n]);
    let eye = {
        let mut e = eye;
        for i in 0..n { e.set_complex(&[i, i], Complex64::new(1.0, 0.0)); }
        e
    };
    solve(a, &eye)
}

pub fn chol(a: &MxArray) -> Result<MxArray, MathError> {
    let n = a.shape[0];
    let mut r = vec![vec![Complex64::new(0.0, 0.0); n]; n];
    for j in 0..n {
        for i in 0..=j {
            let mut s = a.get_complex(&[i, j]);
            for k in 0..i { s -= r[k][i].conj() * r[k][j]; }
            if i == j {
                if s.re <= 0.0 { return Err(MathError::Msg("chol: matrix not positive definite".into())); }
                r[i][j] = Complex64::new(s.re.sqrt(), 0.0);
            } else {
                r[i][j] = s / r[i][i];
            }
        }
    }
    Ok(from_mat(&r))
}

const EPS: f64 = 1e-14;

/// Householder QR for a complex matrix. Returns (Q, R).
pub fn qr_complex(a: &MxArray) -> Result<(MxArray, MxArray), MathError> {
    let mut mat = to_mat(a);
    let m = mat.len();
    let n = if m > 0 { mat[0].len() } else { 0 };
    let mut q = vec![vec![Complex64::new(0.0, 0.0); m]; m];
    for i in 0..m { q[i][i] = Complex64::new(1.0, 0.0); }
    for k in 0..n.min(m) {
        // column vector x = mat[k..][k]
        let mut x = vec![Complex64::new(0.0, 0.0); m - k];
        for i in k..m { x[i - k] = mat[i][k]; }
        let norm_x: f64 = x.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
        if norm_x < EPS { continue; }
        let sign = if x[0].re >= 0.0 { Complex64::new(1.0, 0.0) } else { Complex64::new(-1.0, 0.0) };
        let u0 = x[0] + sign * norm_x;
        let mut u = x.clone();
        u[0] = u0;
        let unorm2: f64 = u.iter().map(|c| c.norm_sqr()).sum::<f64>();
        if unorm2 < EPS { continue; }
        // apply H = I - 2 uu^H / (u^H u) to mat (columns k..n) and to q
        for c in k..n {
            let mut dot = Complex64::new(0.0, 0.0);
            for i in k..m { dot += u[i - k].conj() * mat[i][c]; }
            let factor = dot / (unorm2 / 2.0);
            for i in k..m { mat[i][c] -= factor * u[i - k]; }
        }
        for c in 0..m {
            let mut dot = Complex64::new(0.0, 0.0);
            for i in k..m { dot += u[i - k].conj() * q[i][c]; }
            let factor = dot / (unorm2 / 2.0);
            for i in k..m { q[i][c] -= factor * u[i - k]; }
        }
    }
    Ok((from_mat(&q), from_mat(&mat)))
}

/// QR on raw row-major matrix, returns (Q, R) as row-major matrices.
fn qr_mat(mat: &[Vec<Complex64>]) -> (Vec<Vec<Complex64>>, Vec<Vec<Complex64>>) {
    let m = mat.len();
    let n = if m > 0 { mat[0].len() } else { 0 };
    let mut a = mat.to_vec();
    let mut q = vec![vec![Complex64::new(0.0, 0.0); m]; m];
    for i in 0..m { q[i][i] = Complex64::new(1.0, 0.0); }
    for k in 0..n.min(m) {
        let mut x = vec![Complex64::new(0.0, 0.0); m - k];
        for i in k..m { x[i - k] = a[i][k]; }
        let nrm: f64 = x.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
        if nrm < EPS { continue; }
        let sgn = if x[0].re >= 0.0 { Complex64::new(1.0, 0.0) } else { Complex64::new(-1.0, 0.0) };
        let u0 = x[0] + sgn * nrm;
        let mut u = x;
        u[0] = u0;
        let u2: f64 = u.iter().map(|c| c.norm_sqr()).sum::<f64>();
        if u2 < EPS { continue; }
        for c in k..n {
            let mut dot = Complex64::new(0.0, 0.0);
            for i in k..m { dot += u[i - k].conj() * a[i][c]; }
            let f = dot / (u2 / 2.0);
            for i in k..m { a[i][c] -= f * u[i - k]; }
        }
        for c in 0..m {
            let mut dot = Complex64::new(0.0, 0.0);
            for i in k..m { dot += u[i - k].conj() * q[i][c]; }
            let f = dot / (u2 / 2.0);
            for i in k..m { q[i][c] -= f * u[i - k]; }
        }
    }
    (q, a)
}

/// Eigenvalues + eigenvectors via shifted QR with deflation + inverse iteration.
/// Returns (V, D) where A*V = V*D; D diagonal with eigenvalues; V columns are eigenvectors.
pub fn eig(a: &MxArray) -> Result<(MxArray, MxArray), MathError> {
    let n = a.shape[0];
    if n == 0 { return Err(MathError::Msg("eig: empty matrix".into())); }
    let a_orig = to_mat(a);
    let mut mat = a_orig.clone();
    let mut q_accum = vec![vec![Complex64::new(0.0, 0.0); n]; n];
    for i in 0..n { q_accum[i][i] = Complex64::new(1.0, 0.0); }

    let mut active = n;
    for _ in 0..500 {
        // Deflate converged subdiagonals (check from bottom)
        while active > 1 {
            let i = active - 1;
            let tol = EPS * (mat[i-1][i-1].norm() + mat[i][i].norm() + EPS);
            if mat[i][i-1].norm() < tol {
                mat[i][i-1] = Complex64::new(0.0, 0.0);
                active -= 1;
            } else {
                break;
            }
        }
        if active <= 1 { break; }

        // Wilkinson shift from bottom-right 2x2 of active submatrix
        let a11 = mat[active-1][active-1];
        let a22 = mat[active-2][active-2];
        let a12 = mat[active-2][active-1];
        let a21 = mat[active-1][active-2];
        let tr = a11 + a22;
        let det = a11 * a22 - a12 * a21;
        let disc = (tr * tr - 4.0 * det).sqrt();
        let e1 = (tr + disc) / 2.0;
        let e2 = (tr - disc) / 2.0;
        let shift = if (e1 - a11).norm() < (e2 - a11).norm() { e1 } else { e2 };

        // A - shift I (full matrix)
        for i in 0..n { mat[i][i] -= shift; }

        // QR of full shifted matrix: q_full * mat = r  (q_full = product of reflectors left-applied)
        let (q, r) = qr_mat(&mat);
        // Standard QR requires Q_std * R = mat => Q_std = q_full^T, R = r
        // A' = R * Q_std + shift*I = r * q^T + shift*I  = q^T * A * q  (similarity ✓)
        let mut qt = vec![vec![Complex64::new(0.0, 0.0); n]; n];
        for i in 0..n { for j in 0..n { qt[i][j] = q[j][i]; } }
        let rq = matmul_mat(&r, &qt);
        for i in 0..n {
            for j in 0..n {
                mat[i][j] = rq[i][j] + if i == j { shift } else { Complex64::new(0.0, 0.0) };
            }
        }
        // Accumulate Q = Q * q  (right-multiply to accumulate similarity transforms)
        let mut new_q = vec![vec![Complex64::new(0.0, 0.0); n]; n];
        for i in 0..n {
            for j in 0..n {
                let mut s = Complex64::new(0.0, 0.0);
                for k in 0..n { s += q_accum[i][k] * q[k][j]; }
                new_q[i][j] = s;
            }
        }
        q_accum = new_q;
    }

    // Extract eigenvalues from diagonal
    let mut eigvals = vec![Complex64::new(0.0, 0.0); n];
    for i in 0..n { eigvals[i] = mat[i][i]; }

    // Compute eigenvectors via inverse iteration: (A - λI) v^(k+1) = v^(k)
    let mut v = vec![vec![Complex64::new(0.0, 0.0); n]; n];
    for ev in 0..n {
        let lam = eigvals[ev];
        // (A - λI)
        let mut shifted = a_orig.clone();
        for i in 0..n { shifted[i][i] -= lam; }
        // Solve (A - λI) x = b with random-ish b, iterate a few times
        let mut x = vec![Complex64::new(0.0, 0.0); n];
        for i in 0..n { x[i] = Complex64::new(1.0 + i as f64 * 0.1, 0.3 * (i + 1) as f64); }
        // Normalize
        let nrm: f64 = x.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
        for xi in x.iter_mut() { *xi /= nrm; }

        for _iter in 0..20 {
            // Solve (A - λI) x_new = x
            let b = x.clone();
            // Use LU solve (crude, but works)
            let mut lu = shifted.clone();
            let m = n;
            let mut piv = (0..m).collect::<Vec<_>>();
            for j in 0..m {
                let mut p = j;
                let mut mx = lu[j][j].norm();
                for i in (j+1)..m {
                    let v = lu[i][j].norm();
                    if v > mx { mx = v; p = i; }
                }
                if mx < EPS { continue; }
                if p != j { lu.swap(p, j); piv.swap(p, j); }
                for i in (j+1)..m {
                    let f = lu[i][j] / lu[j][j];
                    lu[i][j] = f;
                    let luj = lu[j].clone();
                    for c in (j+1)..m { lu[i][c] -= f * luj[c]; }
                }
            }
            // forward/backward solve
            let mut pb = vec![Complex64::new(0.0, 0.0); m];
            for i in 0..m { pb[i] = b[piv[i]]; }
            let mut y = vec![Complex64::new(0.0, 0.0); m];
            for i in 0..m {
                let mut s = pb[i];
                for j in 0..i { s -= lu[i][j] * y[j]; }
                y[i] = s;
            }
            for i in (0..m).rev() {
                let mut s = y[i];
                for j in (i+1)..m { s -= lu[i][j] * x[j]; }
                if lu[i][i].norm() < EPS { x[i] = Complex64::new(1.0, 0.0); }
                else { x[i] = s / lu[i][i]; }
            }
            // normalize
            let nrm: f64 = x.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
            if nrm > EPS { for xi in x.iter_mut() { *xi /= nrm; } }
        }
        for i in 0..n { v[i][ev] = x[i]; }
    }

    // Build D as diagonal matrix
    let mut d = vec![vec![Complex64::new(0.0, 0.0); n]; n];
    for i in 0..n { d[i][i] = eigvals[i]; }

    Ok((from_mat(&v), from_mat(&d)))
}

pub fn svd(a: &MxArray) -> Result<(Vec<f64>, MxArray, MxArray), MathError> {
    // S = sqrt(eig of A^H A); V from eigvecs; U = A V S^{-1}
    let at = a.transpose();
    let mut ah = matmul_mat(&to_mat(&at), &to_mat(a));
    // ensure Hermitian
    let n = ah.len();
    for i in 0..n {
        for j in 0..n {
            ah[i][j] = (ah[i][j] + ah[j][i].conj()) / 2.0;
        }
    }
    let (v, d) = eig(&from_mat(&ah))?;
    let mut s = Vec::with_capacity(n);
    for i in 0..n {
        let lam = d.get_complex(&[i, i]);
        s.push(lam.re.max(0.0).sqrt());
    }
    // U = A V diag(1/s)
    let vt = to_mat(&v);
    let av = matmul_mat(&to_mat(a), &vt);
    let mut u = vec![vec![Complex64::new(0.0, 0.0); n]; n];
    for j in 0..n {
        let sj = if s[j] > 1e-12 { s[j] } else { 1.0 };
        for i in 0..n { u[i][j] = av[i][j] / sj; }
    }
    Ok((s, from_mat(&u), from_mat(&vt)))
}

fn matmul_mat(a: &[Vec<Complex64>], b: &[Vec<Complex64>]) -> Vec<Vec<Complex64>> {
    let m = a.len();
    let p = if m > 0 { a[0].len() } else { 0 };
    let n = if !b.is_empty() { b[0].len() } else { 0 };
    let mut c = vec![vec![Complex64::new(0.0, 0.0); n]; m];
    for i in 0..m {
        for k in 0..p {
            let ak = a[i][k];
            if ak.norm() == 0.0 { continue; }
            for j in 0..n {
                c[i][j] += ak * b[k][j];
            }
        }
    }
    c
}
