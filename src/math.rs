use crate::Metric;

pub(crate) fn inverse_norm(vector: &[f32]) -> f64 {
    let norm = vector
        .iter()
        .map(|&x| f64::from(x).powi(2))
        .sum::<f64>()
        .sqrt();
    if norm == 0.0 { 0.0 } else { norm.recip() }
}

pub(crate) struct Query<'a> {
    pub vector: &'a [f32],
    pub inverse_norm: f64,
}

impl<'a> Query<'a> {
    pub fn new(vector: &'a [f32], metric: Metric) -> Self {
        Self {
            vector,
            inverse_norm: if metric == Metric::Cosine {
                inverse_norm(vector)
            } else {
                0.0
            },
        }
    }
}

pub(crate) fn distance(metric: Metric, query: &Query<'_>, vector: &[f32], norm: f64) -> f64 {
    match metric {
        Metric::SquaredL2 => squared_l2(query.vector, vector),
        Metric::Cosine | Metric::InnerProduct => {
            let dot = query
                .vector
                .iter()
                .zip(vector)
                .map(|(&a, &b)| f64::from(a) * f64::from(b))
                .sum::<f64>();
            if metric == Metric::Cosine {
                (1.0 - dot * query.inverse_norm * norm).clamp(0.0, 2.0)
            } else {
                -dot
            }
        }
    }
}

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
