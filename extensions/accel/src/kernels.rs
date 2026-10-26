use vecnook::generation::{DistanceKernel, ScalarKernel};

/// Arithmetic backend selected after runtime CPU detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Acceleration {
    /// Portable original-f32/f64 arithmetic.
    Scalar,
    /// Four f64 products/differences through x86_64 AVX2.
    Avx2,
    /// Two f64 products/differences through AArch64 NEON.
    Neon,
}

/// Explicit SIMD opt-in with a scalar fallback on unsupported CPUs.
pub struct AutoKernel {
    backend: Acceleration,
}
impl Default for AutoKernel {
    fn default() -> Self {
        Self::detect()
    }
}
impl AutoKernel {
    /// Detect the current CPU before permitting any target-feature function.
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        if std::is_x86_feature_detected!("avx2") {
            return Self {
                backend: Acceleration::Avx2,
            };
        }
        #[cfg(target_arch = "aarch64")]
        if std::arch::is_aarch64_feature_detected!("neon") {
            return Self {
                backend: Acceleration::Neon,
            };
        }
        Self::scalar()
    }
    /// Force the portable reference for comparisons/fallback tests.
    pub fn scalar() -> Self {
        Self {
            backend: Acceleration::Scalar,
        }
    }
    /// Report the actual selected backend.
    pub fn acceleration(&self) -> Acceleration {
        self.backend
    }
    fn calculate(&self, left: &[f32], right: &[f32], l2: bool) -> f64 {
        // Trait callers normally supply equal dimensions. Direct calls are safe
        // for mismatched slices too: use the same zipped prefix as the reference.
        let length = left.len().min(right.len());
        let (left, right) = (&left[..length], &right[..length]);
        match self.backend {
            #[cfg(target_arch = "x86_64")]
            Acceleration::Avx2 => {
                // SAFETY: only detect() constructs this private backend, after
                // AVX2 detection. Slices have equal length and loads are bounded.
                unsafe { avx2(left, right, l2) }
            }
            #[cfg(target_arch = "aarch64")]
            Acceleration::Neon => {
                // SAFETY: detect() checked NEON; equal bounded slices are retained.
                unsafe { neon(left, right, l2) }
            }
            _ => {
                if l2 {
                    ScalarKernel.squared_l2(left, right)
                } else {
                    ScalarKernel.dot(left, right)
                }
            }
        }
    }
}
impl DistanceKernel for AutoKernel {
    fn dot(&self, left: &[f32], right: &[f32]) -> f64 {
        self.calculate(left, right, false)
    }
    fn squared_l2(&self, left: &[f32], right: &[f32]) -> f64 {
        self.calculate(left, right, true)
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn avx2(left: &[f32], right: &[f32], l2: bool) -> f64 {
    use std::arch::x86_64::*;
    let mut sum = -0.0;
    let mut offset = 0;
    while offset + 4 <= left.len() {
        let mut products = [0.0; 4];
        // SAFETY: four f32 values remain in both slices; stack storage has four
        // f64 elements. Unaligned loads/stores require no additional alignment.
        unsafe {
            let a = _mm256_cvtps_pd(_mm_loadu_ps(left.as_ptr().add(offset)));
            let b = _mm256_cvtps_pd(_mm_loadu_ps(right.as_ptr().add(offset)));
            let values = if l2 {
                let d = _mm256_sub_pd(a, b);
                _mm256_mul_pd(d, d)
            } else {
                _mm256_mul_pd(a, b)
            };
            _mm256_storeu_pd(products.as_mut_ptr(), values);
        }
        // Do not horizontally reduce/reassociate or use FMA: match scalar bits.
        for value in products {
            sum += value;
        }
        offset += 4;
    }
    for (&a, &b) in left[offset..].iter().zip(&right[offset..]) {
        let (a, b) = (f64::from(a), f64::from(b));
        sum += if l2 {
            let d = a - b;
            d * d
        } else {
            a * b
        };
    }
    sum
}
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn neon(left: &[f32], right: &[f32], l2: bool) -> f64 {
    use std::arch::aarch64::*;
    let mut sum = -0.0;
    let mut offset = 0;
    while offset + 2 <= left.len() {
        let mut products = [0.0; 2];
        // SAFETY: two f32 values remain in both slices; stack output has two
        // f64 values. NEON was detected before entering this function.
        unsafe {
            let a = vcvt_f64_f32(vld1_f32(left.as_ptr().add(offset)));
            let b = vcvt_f64_f32(vld1_f32(right.as_ptr().add(offset)));
            let values = if l2 {
                let d = vsubq_f64(a, b);
                vmulq_f64(d, d)
            } else {
                vmulq_f64(a, b)
            };
            vst1q_f64(products.as_mut_ptr(), values);
        }
        for value in products {
            sum += value;
        }
        offset += 2;
    }
    for (&a, &b) in left[offset..].iter().zip(&right[offset..]) {
        let (a, b) = (f64::from(a), f64::from(b));
        sum += if l2 {
            let d = a - b;
            d * d
        } else {
            a * b
        };
    }
    sum
}
