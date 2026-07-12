use matbox_math::array::{MxArray, MxData};
use matbox_math::linalg;
use matbox_math::ops::{binary_elementwise, broadcast_shapes, matmul, reduce, unary_elementwise};
use matbox_math::matrix_rm;
use num_complex::Complex64;

fn almost_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-10
}
fn almost_eq_eps(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() < eps
}

fn almost_eq_c(a: Complex64, b: Complex64) -> bool {
    (a - b).norm() < 1e-10
}

fn assert_arr_eq(a: &MxArray, expected: &[f64], shape: &[usize]) {
    assert_eq!(a.shape, shape, "shape mismatch");
    assert_eq!(a.numel(), expected.len(), "numel mismatch");
    for (i, &exp) in expected.iter().enumerate() {
        let got = a.iter_complex()[i].re;
        assert!(almost_eq(got, exp), "idx {}: got {}, expected {}", i, got, exp);
    }
}

// ==================== MxArray constructors ====================

#[test]
fn test_new_double() {
    let a = MxArray::new_double(&[2, 3]);
    assert_eq!(a.shape, vec![2, 3]);
    assert_eq!(a.numel(), 6);
    assert_eq!(a.ndims(), 2);
    assert!(a.is_scalar() == false);
    assert!(a.is_empty() == false);
}

#[test]
fn test_scalar_properties() {
    let a = MxArray::scalar_double(42.0);
    assert!(a.is_scalar());
    assert_eq!(a.to_scalar(), 42.0);
    assert_eq!(a.ndims(), 2);
}

#[test]
fn test_empty_array() {
    let a = MxArray::new_double(&[0, 0]);
    assert!(a.is_empty());
    assert_eq!(a.numel(), 0);
}

#[test]
fn test_from_double_colmajor() {
    let a = MxArray::from_double_colmajor(vec![1.0, 2.0, 3.0, 4.0], &[2, 2]);
    assert_eq!(a.get_double(&[0, 0]), 1.0);
    assert_eq!(a.get_double(&[1, 0]), 2.0);
    assert_eq!(a.get_double(&[0, 1]), 3.0);
    assert_eq!(a.get_double(&[1, 1]), 4.0);
}

#[test]
fn test_matrix_rm_helper() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    assert_eq!(a.shape, vec![2, 2]);
    // row-major input: rows [1,2] and [3,4], stored col-major => [1,3,2,4]
    assert_eq!(a.get_double(&[0, 0]), 1.0);
    assert_eq!(a.get_double(&[1, 0]), 3.0);
    assert_eq!(a.get_double(&[0, 1]), 2.0);
    assert_eq!(a.get_double(&[1, 1]), 4.0);
}

#[test]
fn test_reshape() {
    let a = MxArray::from_double_colmajor(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]);
    let b = a.reshape(&[3, 2]);
    assert_eq!(b.shape, vec![3, 2]);
    assert_eq!(b.get_double(&[0, 0]), 1.0);
    assert_eq!(b.get_double(&[1, 0]), 2.0);
    assert_eq!(b.get_double(&[2, 0]), 3.0);
    assert_eq!(b.get_double(&[0, 1]), 4.0);
    assert_eq!(b.get_double(&[1, 1]), 5.0);
    assert_eq!(b.get_double(&[2, 1]), 6.0);
}

#[test]
#[should_panic(expected = "reshape: number of elements must not change")]
fn test_reshape_mismatch() {
    let a = MxArray::from_double_colmajor(vec![1.0, 2.0, 3.0, 4.0], &[2, 2]);
    let _b = a.reshape(&[3, 3]);
}

#[test]
fn test_transpose_2x2() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    let t = a.transpose();
    // original col-major: [1, 3, 2, 4]; transpose -> [1, 2, 3, 4]
    assert_arr_eq(&t, &[1.0, 2.0, 3.0, 4.0], &[2, 2]);
}

#[test]
fn test_transpose_3x2() {
    let a = matrix_rm(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    let t = a.transpose();
    assert_eq!(t.shape, vec![3, 2]);
    assert_eq!(t.get_double(&[0, 0]), 1.0);
    assert_eq!(t.get_double(&[1, 0]), 2.0);
    assert_eq!(t.get_double(&[2, 0]), 3.0);
    assert_eq!(t.get_double(&[0, 1]), 4.0);
    assert_eq!(t.get_double(&[1, 1]), 5.0);
    assert_eq!(t.get_double(&[2, 1]), 6.0);
}

#[test]
fn test_transpose_vector() {
    let a = MxArray::from_double_colmajor(vec![1.0, 2.0, 3.0], &[3, 1]);
    let t = a.transpose();
    assert_eq!(t.shape, vec![1, 3]);
    assert_eq!(t.get_double(&[0, 0]), 1.0);
    assert_eq!(t.get_double(&[0, 1]), 2.0);
    assert_eq!(t.get_double(&[0, 2]), 3.0);
}

#[test]
fn test_transpose_scalar() {
    let a = MxArray::scalar_double(5.0);
    let t = a.transpose();
    assert_eq!(t.to_scalar(), 5.0);
}

#[test]
fn test_is_vector_true() {
    let a = MxArray::from_double_colmajor(vec![1.0, 2.0, 3.0], &[1, 3]);
    assert!(a.is_vector());
    let b = MxArray::from_double_colmajor(vec![1.0, 2.0, 3.0], &[3, 1]);
    assert!(b.is_vector());
}

#[test]
fn test_is_vector_false() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    assert!(!a.is_vector());
}

#[test]
fn test_linear_index() {
    let a = matrix_rm(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    assert_eq!(a.linear_index(&[0, 0]), 0);
    assert_eq!(a.linear_index(&[1, 0]), 1);
    assert_eq!(a.linear_index(&[0, 1]), 2);
    assert_eq!(a.linear_index(&[1, 1]), 3);
    assert_eq!(a.linear_index(&[0, 2]), 4);
    assert_eq!(a.linear_index(&[1, 2]), 5);
}

#[test]
fn test_set_by_linear() {
    let mut a = MxArray::new_double(&[3, 1]);
    a.set_by_linear(0, Complex64::new(10.0, 0.0));
    a.set_by_linear(1, Complex64::new(20.0, 0.0));
    a.set_by_linear(2, Complex64::new(30.0, 0.0));
    assert_eq!(a.get_double(&[0, 0]), 10.0);
    assert_eq!(a.get_double(&[1, 0]), 20.0);
    assert_eq!(a.get_double(&[2, 0]), 30.0);
}

#[test]
fn test_is_complex() {
    let a = MxArray::scalar_double(1.0);
    assert!(!a.is_complex());
    let b = MxArray::scalar_complex(Complex64::new(1.0, 2.0));
    assert!(b.is_complex());
}

#[test]
fn test_to_double_vec() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    assert_eq!(a.to_double_vec(), vec![1.0, 3.0, 2.0, 4.0]);
}

#[test]
fn test_get_complex_promotion() {
    let a = MxArray::scalar_double(3.5);
    let c = a.get_complex(&[]);
    assert_eq!(c.re, 3.5);
    assert_eq!(c.im, 0.0);
}

#[test]
fn test_logical_scalar() {
    let mut a = MxArray::new_logical(&[1, 1]);
    assert!(a.is_logical_like());
    if let MxData::Logical(d) = &mut a.data {
        d[0] = true;
    }
    assert_eq!(a.get_double(&[]), 1.0);
    assert_eq!(a.get_complex(&[]).re, 1.0);
}

#[test]
fn test_int64_storage() {
    let a = MxArray::from_double_colmajor(vec![1.0, 2.0], &[2, 1]);
    // test via get_double which works with Int64 too
    // but we don't have int64 creation in public API directly
    // at least verify double creation works
    assert_eq!(a.shape, vec![2, 1]);
}

// ==================== broadcast_shapes ====================

#[test]
fn test_broadcast_same_shape() {
    assert_eq!(broadcast_shapes(&[2, 3], &[2, 3]), Some(vec![2, 3]));
}

#[test]
fn test_broadcast_scalar_and_vec() {
    assert_eq!(broadcast_shapes(&[1, 1], &[1, 3]), Some(vec![1, 3]));
    assert_eq!(broadcast_shapes(&[1, 3], &[1, 1]), Some(vec![1, 3]));
}

#[test]
fn test_broadcast_col_row() {
    assert_eq!(broadcast_shapes(&[3, 1], &[1, 4]), Some(vec![3, 4]));
}

#[test]
fn test_broadcast_different_ndim() {
    assert_eq!(broadcast_shapes(&[3, 1], &[4]), Some(vec![3, 4]));
    assert_eq!(broadcast_shapes(&[5], &[5, 1]), Some(vec![5, 5]));
}

#[test]
fn test_broadcast_incompatible() {
    assert_eq!(broadcast_shapes(&[2, 3], &[2, 4]), None);
    assert_eq!(broadcast_shapes(&[5, 2], &[3, 2]), None);
}

#[test]
fn test_broadcast_same_3d() {
    assert_eq!(broadcast_shapes(&[2, 3, 4], &[2, 3, 4]), Some(vec![2, 3, 4]));
}

#[test]
fn test_broadcast_3d_with_scalar() {
    assert_eq!(broadcast_shapes(&[1, 1, 1], &[2, 3, 4]), Some(vec![2, 3, 4]));
}

#[test]
fn test_broadcast_both_ones() {
    assert_eq!(broadcast_shapes(&[1], &[1]), Some(vec![1]));
}

// ==================== binary_elementwise ====================

#[test]
fn test_binary_add_same_shape() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    let b = matrix_rm(&[vec![5.0, 6.0], vec![7.0, 8.0]]);
    let r = binary_elementwise(&a, &b, |x, y| x + y).unwrap();
    assert_arr_eq(&r, &[6.0, 10.0, 8.0, 12.0], &[2, 2]);
}

#[test]
fn test_binary_add_broadcast() {
    let a = matrix_rm(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    let b = MxArray::scalar_double(10.0);
    let r = binary_elementwise(&a, &b, |x, y| x + y).unwrap();
    assert_arr_eq(&r, &[11.0, 14.0, 12.0, 15.0, 13.0, 16.0], &[2, 3]);
}

#[test]
fn test_binary_mul_same() {
    let a = MxArray::from_double_colmajor(vec![2.0, 3.0, 4.0, 5.0], &[2, 2]);
    let b = MxArray::from_double_colmajor(vec![10.0, 10.0, 10.0, 10.0], &[2, 2]);
    let r = binary_elementwise(&a, &b, |x, y| x * y).unwrap();
    assert_arr_eq(&r, &[20.0, 30.0, 40.0, 50.0], &[2, 2]);
}

#[test]
fn test_binary_div() {
    let a = MxArray::from_double_colmajor(vec![10.0, 20.0, 30.0, 40.0], &[2, 2]);
    let b = MxArray::scalar_double(5.0);
    let r = binary_elementwise(&a, &b, |x, y| x / y).unwrap();
    assert_arr_eq(&r, &[2.0, 4.0, 6.0, 8.0], &[2, 2]);
}

#[test]
fn test_binary_pow() {
    let a = MxArray::from_double_colmajor(vec![2.0, 3.0], &[1, 2]);
    let b = MxArray::scalar_double(3.0);
    let r = binary_elementwise(&a, &b, |x, y| Complex64::new(x.re.powf(y.re), 0.0)).unwrap();
    assert_eq!(r.iter_complex()[0].re, 8.0);
    assert_eq!(r.iter_complex()[1].re, 27.0);
}

#[test]
fn test_binary_dim_mismatch_err() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    let b = MxArray::from_double_colmajor(vec![1.0, 2.0, 3.0], &[3, 1]);
    let r = binary_elementwise(&a, &b, |x, y| x + y);
    assert!(r.is_err());
}

#[test]
fn test_binary_complex_op() {
    let a = MxArray::scalar_complex(Complex64::new(1.0, 2.0));
    let b = MxArray::scalar_complex(Complex64::new(3.0, 4.0));
    let r = binary_elementwise(&a, &b, |x, y| x * y).unwrap();
    // (1+2i)*(3+4i) = -5+10i
    let c = r.get_complex(&[]);
    assert!(almost_eq_c(c, Complex64::new(-5.0, 10.0)));
}

#[test]
fn test_binary_col_row_broadcast() {
    let col = MxArray::from_double_colmajor(vec![1.0, 2.0, 3.0], &[3, 1]);
    let row = MxArray::from_double_colmajor(vec![10.0, 20.0, 30.0], &[1, 3]);
    let r = binary_elementwise(&col, &row, |x, y| x + y).unwrap();
    assert_eq!(r.shape, vec![3, 3]);
    assert_eq!(r.get_double(&[0, 0]), 11.0);
    assert_eq!(r.get_double(&[1, 0]), 12.0);
    assert_eq!(r.get_double(&[2, 0]), 13.0);
    assert_eq!(r.get_double(&[0, 1]), 21.0);
    assert_eq!(r.get_double(&[1, 1]), 22.0);
    assert_eq!(r.get_double(&[2, 1]), 23.0);
}

// ==================== unary_elementwise ====================

#[test]
fn test_unary_negate() {
    let a = MxArray::from_double_colmajor(vec![1.0, -2.0, 3.0], &[1, 3]);
    let r = unary_elementwise(&a, |c| -c);
    assert_arr_eq(&r, &[-1.0, 2.0, -3.0], &[1, 3]);
}

#[test]
fn test_unary_abs() {
    let a = MxArray::from_double_colmajor(vec![-3.0, 4.0, -5.0], &[1, 3]);
    let r = unary_elementwise(&a, |c| Complex64::new(c.norm(), 0.0));
    assert_arr_eq(&r, &[3.0, 4.0, 5.0], &[1, 3]);
}

#[test]
fn test_unary_complex_abs() {
    let a = MxArray::scalar_complex(Complex64::new(3.0, 4.0));
    let r = unary_elementwise(&a, |c| Complex64::new(c.norm(), 0.0));
    assert!(almost_eq(r.to_scalar(), 5.0));
}

#[test]
fn test_unary_sqrt() {
    let a = MxArray::from_double_colmajor(vec![4.0, 9.0, 16.0], &[1, 3]);
    let r = unary_elementwise(&a, |c| c.sqrt());
    assert_arr_eq(&r, &[2.0, 3.0, 4.0], &[1, 3]);
}

#[test]
fn test_unary_sqrt_negative() {
    let a = MxArray::scalar_double(-1.0);
    let r = unary_elementwise(&a, |c| c.sqrt());
    let c = r.get_complex(&[]);
    assert!(almost_eq_c(c, Complex64::new(0.0, 1.0)));
}

// ==================== matmul ====================

#[test]
fn test_matmul_2x2() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    let b = matrix_rm(&[vec![5.0, 6.0], vec![7.0, 8.0]]);
    let r = matmul(&a, &b).unwrap();
    assert_eq!(r.shape, vec![2, 2]);
    // A*B = [19 22; 43 50]
    assert!(almost_eq(r.get_double(&[0, 0]), 19.0));
    assert!(almost_eq(r.get_double(&[1, 0]), 43.0));
    assert!(almost_eq(r.get_double(&[0, 1]), 22.0));
    assert!(almost_eq(r.get_double(&[1, 1]), 50.0));
}

#[test]
fn test_matmul_1x1() {
    let a = MxArray::scalar_double(5.0);
    let b = MxArray::scalar_double(3.0);
    let r = matmul(&a, &b).unwrap();
    assert_eq!(r.to_scalar(), 15.0);
}

#[test]
fn test_matmul_1x3_times_3x1() {
    let a = MxArray::from_double_colmajor(vec![1.0, 2.0, 3.0], &[1, 3]);
    let b = MxArray::from_double_colmajor(vec![4.0, 5.0, 6.0], &[3, 1]);
    let r = matmul(&a, &b).unwrap();
    // 1*4 + 2*5 + 3*6 = 4+10+18 = 32
    assert_eq!(r.shape, vec![1, 1]);
    assert!(almost_eq(r.to_scalar(), 32.0));
}

#[test]
fn test_matmul_3x1_times_1x3() {
    let a = MxArray::from_double_colmajor(vec![1.0, 2.0, 3.0], &[3, 1]);
    let b = MxArray::from_double_colmajor(vec![4.0, 5.0, 6.0], &[1, 3]);
    let r = matmul(&a, &b).unwrap();
    assert_eq!(r.shape, vec![3, 3]);
}

#[test]
fn test_matmul_dim_mismatch() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    let b = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]]);
    let r = matmul(&a, &b);
    assert!(r.is_err());
}

// ==================== reduce ====================

#[test]
fn test_sum_along_cols() {
    let a = matrix_rm(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    let r = reduce(&a, 1, Complex64::new(0.0, 0.0), |acc, x| acc + x);
    assert_eq!(r.shape[1], 3);
    assert!(almost_eq(r.get_double(&[0, 0]), 5.0)); // col0: 1+4
}

#[test]
fn test_sum_along_rows() {
    let a = matrix_rm(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    let r = reduce(&a, 2, Complex64::new(0.0, 0.0), |acc, x| acc + x);
    assert_eq!(r.shape[0], 2);
    assert!(almost_eq(r.get_double(&[0, 0]), 6.0));  // row0: 1+2+3
    assert!(almost_eq(r.get_double(&[1, 0]), 15.0)); // row1: 4+5+6
}

#[test]
fn test_prod_along_rows() {
    let a = MxArray::from_double_colmajor(vec![2.0, 3.0, 4.0, 5.0], &[2, 2]);
    let r = reduce(&a, 1, Complex64::new(1.0, 0.0), |acc, x| acc * x);
    assert_eq!(r.shape, vec![1, 2]);
}

#[test]
fn test_reduce_single_element() {
    let a = MxArray::scalar_double(42.0);
    let r = reduce(&a, 1, Complex64::new(1.0, 0.0), |acc, x| acc * x);
    assert!(almost_eq(r.to_scalar(), 42.0));
}

// ==================== LU ====================

#[test]
fn test_lu_2x2() {
    let a = matrix_rm(&[vec![4.0, 3.0], vec![6.0, 3.0]]);
    let (l, u, piv) = linalg::lu(&a).unwrap();
    assert_eq!(l.shape, vec![2, 2]);
    assert_eq!(u.shape, vec![2, 2]);
    // L should have 1 on diagonal
    assert!(almost_eq(l.get_double(&[0, 0]), 1.0));
    assert!(almost_eq(l.get_double(&[1, 1]), 1.0));
    // Check A = P * L * U (with P from piv)
    let pa = MxArray::from_double_colmajor(
        vec![a.get_double(&[piv[0], 0]), a.get_double(&[piv[1], 0]),
             a.get_double(&[piv[0], 1]), a.get_double(&[piv[1], 1])],
        &[2, 2],
    );
    let lu_mat = matmul(&l, &u).unwrap();
    assert!(almost_eq(pa.get_double(&[0, 0]), lu_mat.get_double(&[0, 0])));
    assert!(almost_eq(pa.get_double(&[1, 0]), lu_mat.get_double(&[1, 0])));
    assert!(almost_eq(pa.get_double(&[0, 1]), lu_mat.get_double(&[0, 1])));
    assert!(almost_eq(pa.get_double(&[1, 1]), lu_mat.get_double(&[1, 1])));
}

#[test]
fn test_lu_singular() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![2.0, 4.0]]);
    let (l, u, _piv) = linalg::lu(&a).unwrap();
    assert_eq!(l.shape, vec![2, 2]);
    assert_eq!(u.shape, vec![2, 2]);
    // U should have zero on second diagonal
    assert!(almost_eq(u.get_double(&[1, 1]), 0.0));
}

#[test]
fn test_lu_3x3() {
    let a = matrix_rm(&[vec![1.0, 1.0, 1.0], vec![0.0, 2.0, 5.0], vec![2.0, 5.0, -1.0]]);
    let (_l, u, _piv) = linalg::lu(&a).unwrap();
    assert_eq!(u.shape, vec![3, 3]);
}

#[test]
fn test_lu_rectangular() {
    let a = matrix_rm(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    let (_l, u, _piv) = linalg::lu(&a).unwrap();
    assert_eq!(u.shape, vec![2, 3]);
}

// ==================== det ====================

#[test]
fn test_det_2x2_identity() {
    let a = matrix_rm(&[vec![1.0, 0.0], vec![0.0, 1.0]]);
    let d = linalg::det(&a).unwrap();
    assert!(almost_eq(d.re, 1.0));
}

#[test]
fn test_det_2x2_neg() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    let d = linalg::det(&a).unwrap();
    // det = 1*4 - 2*3 = -2
    assert!(almost_eq(d.re, -2.0));
}

#[test]
fn test_det_2x2_zero() {
    let a = matrix_rm(&[vec![2.0, 4.0], vec![1.0, 2.0]]);
    let d = linalg::det(&a).unwrap();
    assert!(almost_eq(d.re, 0.0));
}

#[test]
fn test_det_1x1() {
    let a = MxArray::scalar_double(42.0);
    let d = linalg::det(&a).unwrap();
    assert!(almost_eq(d.re, 42.0));
}

#[test]
fn test_det_3x3() {
    let a = matrix_rm(&[vec![6.0, 1.0, 1.0], vec![4.0, -2.0, 5.0], vec![2.0, 8.0, 7.0]]);
    let d = linalg::det(&a).unwrap();
    // det = -306 (via numpy)
    assert!(almost_eq(d.re, -306.0));
}

#[test]
fn test_det_swap_parity() {
    // Matrix that requires row swap (pivot)
    let a = matrix_rm(&[vec![0.0, 1.0], vec![1.0, 0.0]]);
    let d = linalg::det(&a).unwrap();
    assert!(almost_eq(d.re, -1.0));
}

// ==================== solve ====================

#[test]
fn test_solve_2x2() {
    let a = matrix_rm(&[vec![2.0, 1.0], vec![1.0, 3.0]]);
    let b = MxArray::from_double_colmajor(vec![5.0, 6.0], &[2, 1]);
    let x = linalg::solve(&a, &b).unwrap();
    // 2*x1 + x2 = 5, x1 + 3*x2 = 6 => x1 = 9/5, x2 = 7/5
    assert!(almost_eq(x.get_double(&[0, 0]), 1.8));
    assert!(almost_eq(x.get_double(&[1, 0]), 1.4));
}

#[test]
fn test_solve_multiple_rhs() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    let b = matrix_rm(&[vec![5.0, 6.0], vec![7.0, 8.0]]);
    let x = linalg::solve(&a, &b).unwrap();
    assert_eq!(x.shape, vec![2, 2]);
    // Verify A*x = b
    let ax = matmul(&a, &x).unwrap();
    assert!(almost_eq(ax.get_double(&[0, 0]), b.get_double(&[0, 0])));
    assert!(almost_eq(ax.get_double(&[1, 1]), b.get_double(&[1, 1])));
}

#[test]
fn test_solve_singular() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![2.0, 4.0]]);
    let b = MxArray::from_double_colmajor(vec![1.0, 2.0], &[2, 1]);
    let x = linalg::solve(&a, &b);
    assert!(x.is_err());
}

// ==================== inv ====================

#[test]
fn test_inv_2x2_identity_result() {
    let a = matrix_rm(&[vec![4.0, 7.0], vec![2.0, 6.0]]);
    let inv_a = linalg::inv(&a).unwrap();
    let r = matmul(&a, &inv_a).unwrap();
    assert!(almost_eq(r.get_double(&[0, 0]), 1.0));
    assert!(almost_eq(r.get_double(&[1, 1]), 1.0));
    assert!(almost_eq(r.get_double(&[0, 1]), 0.0));
    assert!(almost_eq(r.get_double(&[1, 0]), 0.0));
}

#[test]
fn test_inv_identity() {
    let a = matrix_rm(&[vec![1.0, 0.0], vec![0.0, 1.0]]);
    let inv_a = linalg::inv(&a).unwrap();
    assert!(almost_eq(inv_a.get_double(&[0, 0]), 1.0));
    assert!(almost_eq(inv_a.get_double(&[1, 1]), 1.0));
}

// ==================== rank ====================

#[test]
fn test_rank_full() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    assert_eq!(linalg::rank(&a, 0.0), 2);
}

#[test]
fn test_rank_singular() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![2.0, 4.0]]);
    assert_eq!(linalg::rank(&a, 0.0), 1);
}

#[test]
fn test_rank_zero_matrix() {
    let a = matrix_rm(&[vec![0.0, 0.0], vec![0.0, 0.0]]);
    assert_eq!(linalg::rank(&a, 0.0), 0);
}

#[test]
fn test_rank_3x3() {
    let a = matrix_rm(&[vec![1.0, 2.0, 3.0], vec![0.0, 1.0, 4.0], vec![0.0, 0.0, 1.0]]);
    assert_eq!(linalg::rank(&a, 0.0), 3);
}

// ==================== norm ====================

#[test]
fn test_norm_fro_2x2() {
    let a = matrix_rm(&[vec![3.0, 4.0], vec![0.0, 0.0]]);
    let n = linalg::norm(&a, "fro").unwrap();
    assert!(almost_eq(n, 5.0));
}

#[test]
fn test_norm_1() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    let n = linalg::norm(&a, "1").unwrap();
    assert!(almost_eq(n, 6.0)); // max column sum: max(1+3=4, 2+4=6) = 6
}

#[test]
fn test_norm_inf() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    let n = linalg::norm(&a, "inf").unwrap();
    assert!(almost_eq(n, 7.0)); // max row sum: max(1+2=3, 3+4=7) = 7
}

#[test]
fn test_norm_vector() {
    let a = MxArray::from_double_colmajor(vec![3.0, 4.0], &[2, 1]);
    let n = linalg::norm(&a, "fro").unwrap();
    assert!(almost_eq(n, 5.0));
}

#[test]
fn test_norm_invalid_ord() {
    let a = MxArray::scalar_double(1.0);
    let n = linalg::norm(&a, "xyz");
    assert!(n.is_err());
}

// ==================== trace ====================

#[test]
fn test_trace_2x2() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0]]);
    let t = linalg::trace(&a);
    assert!(almost_eq(t.re, 5.0));
}

#[test]
fn test_trace_identity() {
    let a = matrix_rm(&[vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0], vec![0.0, 0.0, 1.0]]);
    let t = linalg::trace(&a);
    assert!(almost_eq(t.re, 3.0));
}

#[test]
fn test_trace_non_square() {
    let a = matrix_rm(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    let t = linalg::trace(&a);
    assert!(almost_eq(t.re, 6.0)); // min(2,3) = 2 => 1+5 = 6
}

// ==================== QR ====================

#[test]
fn test_qr_2x2() {
    let a = matrix_rm(&[vec![12.0, -51.0], vec![6.0, 167.0]]);
    let (q, r) = linalg::qr_complex(&a).unwrap();
    assert_eq!(q.shape, vec![2, 2]);
    assert_eq!(r.shape, vec![2, 2]);
    // Q^T * R ≈ A (since R = Q * A, so Q^T * R = Q^T * Q * A = A)
    let qt = q.transpose();
    let qtr = matmul(&qt, &r).unwrap();
    let n = a.numel();
    for lin in 0..n {
        assert!(almost_eq(qtr.iter_complex()[lin].re, a.iter_complex()[lin].re),
            "qtr[{}]: {} != {}", lin, qtr.iter_complex()[lin].re, a.iter_complex()[lin].re);
    }
    // Q should be orthogonal: Q^T * Q ≈ I
    let qtq = matmul(&qt, &q).unwrap();
    assert!(almost_eq(qtq.get_double(&[0, 0]), 1.0));
    assert!(almost_eq(qtq.get_double(&[1, 1]), 1.0));
}

#[test]
fn test_qr_3x2_tall() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]]);
    let (q, r) = linalg::qr_complex(&a).unwrap();
    assert_eq!(q.shape, vec![3, 3]);
    assert_eq!(r.shape, vec![3, 2]);
    let qt = q.transpose();
    let qtr = matmul(&qt, &r).unwrap();
    let n = a.numel();
    for lin in 0..n {
        assert!(almost_eq(qtr.iter_complex()[lin].re, a.iter_complex()[lin].re),
            "qtr[{}]: {} != {}", lin, qtr.iter_complex()[lin].re, a.iter_complex()[lin].re);
    }
}

#[test]
fn test_qr_2x3_wide() {
    let a = matrix_rm(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    let (_q, r) = linalg::qr_complex(&a).unwrap();
    assert_eq!(r.shape, vec![2, 3]);
}

// ==================== Cholesky ====================

#[test]
fn test_chol_2x2() {
    // Positive definite: [4, 2; 2, 5]
    let a = matrix_rm(&[vec![4.0, 2.0], vec![2.0, 5.0]]);
    let r = linalg::chol(&a).unwrap();
    // R^T * R should equal A
    let rt = r.transpose();
    let rtr = matmul(&rt, &r).unwrap();
    assert!(almost_eq(rtr.get_double(&[0, 0]), 4.0));
    assert!(almost_eq(rtr.get_double(&[1, 1]), 5.0));
    assert!(almost_eq(rtr.get_double(&[0, 1]), 2.0));
}

#[test]
fn test_chol_3x3() {
    let a = matrix_rm(&[vec![25.0, 15.0, -5.0], vec![15.0, 18.0, 0.0], vec![-5.0, 0.0, 11.0]]);
    let r = linalg::chol(&a).unwrap();
    let rt = r.transpose();
    let rtr = matmul(&rt, &r).unwrap();
    assert!(almost_eq(rtr.get_double(&[0, 0]), 25.0));
    assert!(almost_eq(rtr.get_double(&[1, 1]), 18.0));
    assert!(almost_eq(rtr.get_double(&[2, 2]), 11.0));
}

#[test]
fn test_chol_not_positive_definite() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![2.0, 1.0]]);
    let r = linalg::chol(&a);
    assert!(r.is_err());
}

// ==================== SVD ====================

#[test]
fn test_svd_2x2() {
    let a = matrix_rm(&[vec![2.0, 0.0], vec![0.0, 3.0]]);
    let (s, u, v) = linalg::svd(&a).unwrap();
    assert_eq!(s.len(), 2);
    assert_eq!(u.shape, vec![2, 2]);
    assert_eq!(v.shape, vec![2, 2]);
    // U * diag(S) * V^T ≈ A  (using original S order, no sorting)
    let vt = v.transpose();
    let mut usvt = MxArray::new_complex(&[2, 2]);
    for i in 0..2 {
        for j in 0..2 {
            let mut sum = Complex64::new(0.0, 0.0);
            for k in 0..2 {
                sum += u.get_complex(&[i, k]) * Complex64::new(s[k], 0.0) * vt.get_complex(&[k, j]);
            }
            usvt.set_complex(&[i, j], sum);
        }
    }
    for lin in 0..4 {
        assert!(almost_eq(usvt.iter_complex()[lin].re, a.iter_complex()[lin].re),
            "usvt[{}]: {} != {}", lin, usvt.iter_complex()[lin].re, a.iter_complex()[lin].re);
    }
}

#[test]
fn test_svd_rank_deficient() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![2.0, 4.0]]);
    let (s, _u, _v) = linalg::svd(&a).unwrap();
    assert_eq!(s.len(), 2);
    // A^T A eigenvalues are 0 and 25, so singular values are 0 and ~5
    let max_sv = s[0].max(s[1]);
    let min_sv = s[0].min(s[1]);
    assert!(almost_eq_eps(max_sv, 5.0, 1e-8),
        "max sv: {} != 5", max_sv);
    assert!(min_sv < 1e-10,
        "min sv: {} not near 0", min_sv);
}

#[test]
fn test_svd_3x2() {
    let a = matrix_rm(&[vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]]);
    let (s, _u, _v) = linalg::svd(&a).unwrap();
    assert_eq!(s.len(), 2);
}

// ==================== eig ====================

fn is_either(x: f64, a: f64, b: f64) -> bool { almost_eq(x, a) || almost_eq(x, b) }

#[test]
fn test_eig_2x2() {
    let a = matrix_rm(&[vec![5.0, 4.0], vec![1.0, 2.0]]);
    let (_v, d) = linalg::eig(&a).unwrap();
    assert_eq!(d.shape, vec![2, 2]);
    // eigenvalues should be on diagonal
    let lam1 = d.get_double(&[0, 0]);
    let lam2 = d.get_double(&[1, 1]);
    // characteristic: (5-λ)(2-λ) - 4 = λ^2 - 7λ + 6 = 0 => λ = 1, 6
    assert!(is_either(lam1, 1.0, 6.0) && is_either(lam2, 1.0, 6.0) && !almost_eq(lam1, lam2),
        "eigenvalues {:.4}, {:.4} not {{1,6}}", lam1, lam2);
}

#[test]
fn test_eig_symmetric() {
    let a = matrix_rm(&[vec![2.0, 1.0], vec![1.0, 2.0]]);
    let (_v, d) = linalg::eig(&a).unwrap();
    let lam1 = d.get_double(&[0, 0]);
    let lam2 = d.get_double(&[1, 1]);
    // eigenvalues: 1, 3
    assert!(almost_eq(lam1, 1.0) || almost_eq(lam1, 3.0));
    assert!(almost_eq(lam2, 1.0) || almost_eq(lam2, 3.0));
}

#[test]
fn test_eig_identity() {
    let a = matrix_rm(&[vec![1.0, 0.0], vec![0.0, 1.0]]);
    let (_v, d) = linalg::eig(&a).unwrap();
    assert!(almost_eq(d.get_double(&[0, 0]), 1.0));
    assert!(almost_eq(d.get_double(&[1, 1]), 1.0));
}



// need to access pinv from linalg module
// let's add this at module level
