/// Squared Euclidean distance. Both slices have already been validated.
pub(crate) fn squared_l2(a: &[f32], b: &[f32]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(&a, &b)| {
            let delta = f64::from(a) - f64::from(b);
            delta * delta
        })
        .sum()
}
