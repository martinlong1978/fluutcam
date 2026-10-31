// SPDX-License-Identifier: MIT OR Apache-2.0

//! B-spline basis functions (*The NURBS Book*, 2nd ed., algorithms A2.1 and A2.3).

/// Highest supported degree (D02; STEP allows more, but CAD exports stay far below).
pub const MAX_DEGREE: usize = 25;

/// One row of basis-function values, indexed `0..=degree`.
pub type BasisRow = [f64; MAX_DEGREE + 1];

/// Index of the knot span containing `u` (A2.1).
///
/// `n` is the index of the last pole and `p` the degree. `u` at or beyond the end
/// of the domain returns `n`; at or before the start returns `p`.
pub fn find_span(n: usize, p: usize, u: f64, knots: &[f64]) -> usize {
    if u >= knots[n + 1] {
        return n;
    }
    if u <= knots[p] {
        return p;
    }
    let (mut low, mut high) = (p, n + 1);
    let mut mid = (low + high) / 2;
    while u < knots[mid] || u >= knots[mid + 1] {
        if u < knots[mid] {
            high = mid;
        } else {
            low = mid;
        }
        mid = (low + high) / 2;
    }
    mid
}

/// Working size for degrees up to 7, which covers nearly all CAD data.
pub const SMALL: usize = 8;

/// Non-zero basis functions and their derivatives up to order `nd` at `u` (A2.3).
///
/// Returns `ders` with `ders[k][j]` the k-th derivative of `N_{span-p+j, p}(u)`.
/// Derivative orders above `p` are zero. `nd ≤ 3`.
pub fn ders_basis_funs(span: usize, u: f64, p: usize, nd: usize, knots: &[f64]) -> [BasisRow; 4] {
    ders_basis_funs_n::<{ MAX_DEGREE + 1 }>(span, u, p, nd, knots)
}

/// [`ders_basis_funs`] with working arrays of size `N` (requires `p < N`).
/// Callers use `N = SMALL` for low degrees to avoid initialising large arrays
/// on every evaluation.
pub fn ders_basis_funs_n<const N: usize>(
    span: usize,
    u: f64,
    p: usize,
    nd: usize,
    knots: &[f64],
) -> [[f64; N]; 4] {
    debug_assert!(p < N && nd <= 3);
    let mut ndu = [[0.0f64; N]; N];
    let mut left = [0.0f64; N];
    let mut right = [0.0f64; N];
    ndu[0][0] = 1.0;
    for j in 1..=p {
        left[j] = u - knots[span + 1 - j];
        right[j] = knots[span + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            // Lower triangle: knot differences.
            ndu[j][r] = right[r + 1] + left[j - r];
            let temp = ndu[r][j - 1] / ndu[j][r];
            // Upper triangle: basis functions.
            ndu[r][j] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        ndu[j][j] = saved;
    }

    let mut ders = [[0.0f64; N]; 4];
    for j in 0..=p {
        ders[0][j] = ndu[j][p];
    }
    let nd_eff = nd.min(p);
    let mut a = [[0.0f64; N]; 2];
    for r in 0..=p {
        let (mut s1, mut s2) = (0usize, 1usize);
        a[0][0] = 1.0;
        for k in 1..=nd_eff {
            let mut d = 0.0;
            let rk = r as isize - k as isize;
            let pk = p - k;
            if r >= k {
                a[s2][0] = a[s1][0] / ndu[pk + 1][r - k];
                d = a[s2][0] * ndu[r - k][pk];
            }
            let j1 = if rk >= -1 { 1 } else { (-rk) as usize };
            let j2 = if r as isize - 1 <= pk as isize {
                k - 1
            } else {
                p - r
            };
            for j in j1..=j2 {
                let idx = (rk + j as isize) as usize;
                a[s2][j] = (a[s1][j] - a[s1][j - 1]) / ndu[pk + 1][idx];
                d += a[s2][j] * ndu[idx][pk];
            }
            if r <= pk {
                a[s2][k] = -a[s1][k - 1] / ndu[pk + 1][r];
                d += a[s2][k] * ndu[r][pk];
            }
            ders[k][r] = d;
            std::mem::swap(&mut s1, &mut s2);
        }
    }
    // Multiply through by the correct factors p!/(p-k)!.
    let mut factor = p as f64;
    for (k, row) in ders.iter_mut().enumerate().take(nd_eff + 1).skip(1) {
        for v in row.iter_mut().take(p + 1) {
            *v *= factor;
        }
        factor *= (p - k) as f64;
    }
    ders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_span_examples() {
        // NURBS Book example 2.3: U = {0,0,0,1,2,3,4,4,5,5,5}, p = 2, n = 7.
        let u = [0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0, 5.0, 5.0, 5.0];
        assert_eq!(find_span(7, 2, 2.5, &u), 4);
        assert_eq!(find_span(7, 2, 0.0, &u), 2);
        assert_eq!(find_span(7, 2, 5.0, &u), 7);
        assert_eq!(find_span(7, 2, 4.0, &u), 7);
    }

    #[test]
    fn basis_partition_of_unity_and_example() {
        let u = [0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0, 5.0, 5.0, 5.0];
        let span = find_span(7, 2, 2.5, &u);
        let d = ders_basis_funs(span, 2.5, 2, 2, &u);
        // NURBS Book example 2.4: N(2,2)=1/8, N(3,2)=6/8, N(4,2)=1/8.
        assert!((d[0][0] - 0.125).abs() < 1e-15);
        assert!((d[0][1] - 0.75).abs() < 1e-15);
        assert!((d[0][2] - 0.125).abs() < 1e-15);
        // Derivatives of a partition of unity sum to zero.
        assert!((d[1][0] + d[1][1] + d[1][2]).abs() < 1e-14);
        assert!((d[2][0] + d[2][1] + d[2][2]).abs() < 1e-14);
        // Example 2.4 first derivatives: -0.5, 0, 0.5.
        assert!((d[1][0] + 0.5).abs() < 1e-15);
        assert!(d[1][1].abs() < 1e-15);
        assert!((d[1][2] - 0.5).abs() < 1e-15);
    }
}
