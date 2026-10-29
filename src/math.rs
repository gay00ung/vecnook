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

/// Score four independent vectors while retaining each scalar reduction's order.
/// Interleaving candidates reuses query loads and exposes independent additions;
/// it never reassociates dimensions or reduces the f64 accumulation precision.
pub(crate) fn distance_four(
    metric: Metric,
    query: &Query<'_>,
    vectors: [&[f32]; 4],
    norms: [f64; 4],
) -> [f64; 4] {
    debug_assert!(vectors.iter().all(|v| v.len() == query.vector.len()));
    // Match Iterator::sum's floating-point identity, including signed zero.
    let mut sums = [-0.0_f64; 4];
    let coordinates = query
        .vector
        .iter()
        .zip(vectors[0])
        .zip(vectors[1])
        .zip(vectors[2])
        .zip(vectors[3]);
    match metric {
        Metric::SquaredL2 => {
            for ((((&q, &a), &b), &c), &d) in coordinates {
                let q = f64::from(q);
                let a = q - f64::from(a);
                let b = q - f64::from(b);
                let c = q - f64::from(c);
                let d = q - f64::from(d);
                sums[0] += a * a;
                sums[1] += b * b;
                sums[2] += c * c;
                sums[3] += d * d;
            }
            sums
        }
        Metric::Cosine | Metric::InnerProduct => {
            for ((((&q, &a), &b), &c), &d) in coordinates {
                let q = f64::from(q);
                sums[0] += q * f64::from(a);
                sums[1] += q * f64::from(b);
                sums[2] += q * f64::from(c);
                sums[3] += q * f64::from(d);
            }
            std::array::from_fn(|i| {
                if metric == Metric::Cosine {
                    (1.0 - sums[i] * query.inverse_norm * norms[i]).clamp(0.0, 2.0)
                } else {
                    -sums[i]
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaved_distances_match_scalar_bits_on_tails_and_extremes() {
        let values = [
            0.0,
            -0.0,
            f32::MIN_POSITIVE,
            f32::from_bits(1),
            -f32::from_bits(1),
            1.0,
            -1.0,
            1e-20,
            -1e20,
            f32::MAX,
            -f32::MAX,
        ];
        for dimensions in [1, 2, 3, 4, 7, 8, 15, 32, 63, 128, 767, 768, 1536, 4096] {
            for offset in 0..values.len() {
                let q: Vec<_> = (0..dimensions)
                    .map(|i| values[(i + offset) % values.len()])
                    .collect();
                let vectors: [Vec<f32>; 4] = std::array::from_fn(|lane| {
                    (0..dimensions)
                        .map(|i| values[(i * (lane + 1) + offset + lane) % values.len()])
                        .collect()
                });
                let refs = vectors.each_ref().map(Vec::as_slice);
                let norms = refs.map(inverse_norm);
                for metric in [Metric::SquaredL2, Metric::Cosine, Metric::InnerProduct] {
                    let query = Query::new(&q, metric);
                    let actual = distance_four(metric, &query, refs, norms);
                    for i in 0..4 {
                        assert_eq!(
                            actual[i].to_bits(),
                            distance(metric, &query, refs[i], norms[i]).to_bits(),
                            "metric={metric:?} dimensions={dimensions} offset={offset} lane={i}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn interleaved_dot_preserves_cancellation_and_negative_zero() {
        for vector in [
            vec![-0.0; 33],
            vec![0.0; 33],
            vec![1e20, 1.0, -1e20, 3.0, -3.0, 1e-20, -1e-20],
        ] {
            let q = vec![1.0; vector.len()];
            for metric in [Metric::SquaredL2, Metric::Cosine, Metric::InnerProduct] {
                let query = Query::new(&q, metric);
                let norm = inverse_norm(&vector);
                let actual = distance_four(metric, &query, [&vector; 4], [norm; 4]);
                let expected = distance(metric, &query, &vector, norm).to_bits();
                assert!(actual.iter().all(|v| v.to_bits() == expected));
            }
        }
    }
}
