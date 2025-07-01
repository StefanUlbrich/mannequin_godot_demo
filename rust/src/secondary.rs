//! Implements secondary goals in the null space of the inverse kinematics

use faer::{Col, ColMut, ColRef, Mat, MatRef, Scale, linalg::solvers::DenseSolveCore};

pub fn pseudo_inverse_underdetermined(matrix: MatRef<f32>) -> Mat<f32> {
    let (rows, _) = matrix.shape();
    let square = matrix * matrix.transpose() + Mat::<f32>::identity(rows, rows) * 1e-5;
    let lu = square.partial_piv_lu();
    matrix.transpose() * lu.inverse()
}

pub fn limit(col: &mut Col<f32>, factor: f32) {
    let norm = col.norm_l2();

    if norm > 1e-5 {
        *col /= Scale(norm);

        let limited = factor.min(norm);

        *col *= Scale(limited);

        dbg!(limited, col.norm_l2());
    }
}

pub fn solve_secondary_goals(
    matrix: &[f32],
    rows: usize,
    cols: usize,
    vector_1: &[f32],
    vector_2: &[f32],
    parameters: &mut [f32],
    limit_radians: f32,
) {
    let jacobians = MatRef::from_column_major_slice(matrix, rows, cols);
    let vector_1 = ColRef::from_slice(vector_1);
    let vector_2 = ColRef::from_slice(vector_2);

    // Assuming underdetermination for secondary goals!

    let jacobian_1 = jacobians.get(3..6, 0..cols);
    let jacobian_2 = jacobians.get(0..3, 0..cols);

    let pseudo_inverse_1 = pseudo_inverse_underdetermined(jacobian_1);

    let mut update = &pseudo_inverse_1 * vector_1;

    let projection_1 = Mat::<f32>::identity(cols, cols) - (pseudo_inverse_1 * jacobian_1);

    // FIXME Unable to make this version work!!
    // update += pseudo_inverse_underdetermined((jacobian_2 * projection_1).as_dyn())
    //     * (vector_2 - (jacobian_2 * &update));

    update += projection_1 * pseudo_inverse_underdetermined(jacobian_2) * vector_2;

    limit(&mut update, limit_radians);
    let mut result = ColMut::from_slice_mut(parameters);
    result.copy_from(update);
}
