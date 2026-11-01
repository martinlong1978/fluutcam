// SPDX-License-Identifier: MIT OR Apache-2.0

//! Pass and level layout (D08 §6, spec 04 §4.12.4): positions come from the
//! region's exact limits, never from a lattice anchored at the origin.

/// `even`: n = ceil(W / s) gaps of W / n, both limits included. The step
/// never exceeds `s`. A zero-width range gives one position.
pub fn layout_even(a: f64, b: f64, s: f64) -> Vec<f64> {
    let w = b - a;
    if w <= 0.0 || s <= 0.0 {
        return vec![a];
    }
    let n = (w / s - 1e-9).ceil().max(1.0) as usize;
    let mut out: Vec<f64> = (0..n).map(|i| a + w * i as f64 / n as f64).collect();
    out.push(b);
    out
}

/// `fixed_from_edge`: a, a + s, a + 2s, …, then b if not already there.
pub fn layout_fixed(a: f64, b: f64, s: f64) -> Vec<f64> {
    let w = b - a;
    if w <= 0.0 || s <= 0.0 {
        return vec![a];
    }
    let n = (w / s + 1e-9).floor() as usize;
    let mut out: Vec<f64> = (0..=n).map(|i| a + s * i as f64).collect();
    if b - out[out.len() - 1] > 1e-9 * s.max(1.0) {
        out.push(b);
    } else {
        let k = out.len() - 1;
        out[k] = b;
    }
    out
}

/// Cutting levels below `top` down to `bottom`, top first, the last exactly
/// `bottom`: `count` even steps, or the fewest even steps no deeper than
/// `step` (D15 §3.2, D09 §2).
pub fn levels(top: f64, bottom: f64, step: Option<f64>, count: Option<usize>) -> Vec<f64> {
    if top <= bottom {
        return vec![bottom];
    }
    let n = match (count, step) {
        (Some(c), _) => c.max(1),
        (None, Some(s)) if s > 0.0 => ((top - bottom) / s - 1e-9).ceil().max(1.0) as usize,
        _ => 1,
    };
    let d = top - bottom;
    (1..=n)
        .map(|i| {
            if i == n {
                bottom
            } else {
                top - d * i as f64 / n as f64
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn even_layout_hits_both_limits() {
        let v = layout_even(0.3, 10.3, 3.0);
        assert_eq!(v.len(), 5);
        assert_eq!(v[0], 0.3);
        assert_eq!(v[4], 10.3);
        assert!(v.windows(2).all(|w| w[1] - w[0] <= 3.0));
        assert_eq!(layout_even(1.0, 7.0, 3.0), vec![1.0, 4.0, 7.0]);
        assert_eq!(layout_even(2.0, 2.0, 1.0), vec![2.0]);
    }

    #[test]
    fn fixed_layout_steps_from_the_edge() {
        assert_eq!(layout_fixed(0.0, 7.0, 3.0), vec![0.0, 3.0, 6.0, 7.0]);
        assert_eq!(layout_fixed(0.0, 6.0, 3.0), vec![0.0, 3.0, 6.0]);
    }

    #[test]
    fn levels_end_exactly_at_bottom() {
        assert_eq!(levels(20.0, 11.9, Some(3.0), None), {
            let d = 20.0 - 11.9;
            vec![20.0 - d / 3.0, 20.0 - 2.0 * d / 3.0, 11.9]
        });
        assert_eq!(levels(20.0, 12.0, Some(4.0), None), vec![16.0, 12.0]);
        assert_eq!(levels(20.0, 12.0, Some(100.0), None), vec![12.0]);
        assert_eq!(
            levels(20.0, 12.0, None, Some(4)),
            vec![18.0, 16.0, 14.0, 12.0]
        );
        assert_eq!(levels(5.0, 5.0, Some(1.0), None), vec![5.0]);
    }
}
