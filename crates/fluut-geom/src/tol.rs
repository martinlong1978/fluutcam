// SPDX-License-Identifier: MIT OR Apache-2.0

//! Tolerances (D02 §2). Passed explicitly; never global.

/// The tolerances a geometric operation works to, in millimetres and radians.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Tol {
    /// Modelling tolerance: gaps below this are coincident (per body, S 01 §1.5).
    pub model: f64,
    /// Maximum deviation of the cutter-location path (per toolpath).
    pub machining: f64,
    /// Solver tolerance `ε_s = min(1e-6, machining / 1000)` (S 04 §4.12.1).
    pub solver: f64,
    /// Angular tolerance for parallel and perpendicular tests, in radians.
    pub angle: f64,
}

impl Tol {
    /// Default angular tolerance.
    pub const DEFAULT_ANGLE: f64 = 1e-9;

    /// Tolerances for machining with the given model and machining tolerances.
    pub fn for_machining(model: f64, machining: f64) -> Tol {
        Tol {
            model,
            machining,
            solver: (machining / 1000.0).min(1e-6),
            angle: Tol::DEFAULT_ANGLE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solver_from_machining() {
        assert_eq!(Tol::for_machining(1e-3, 0.01).solver, 1e-6);
        assert_eq!(Tol::for_machining(1e-3, 0.0001).solver, 0.0001 / 1000.0);
        assert_eq!(Tol::for_machining(1e-3, 0.1).solver, 1e-6);
        assert_eq!(Tol::for_machining(1e-3, 0.1).angle, 1e-9);
    }
}
