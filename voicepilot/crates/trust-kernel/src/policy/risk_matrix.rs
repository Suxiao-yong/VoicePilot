//! E×D risk matrix — V1.1 §4.1.

use crate::policy::types::{DLevel, Effect, ELevel};

/// Classify (E, D) → Effect per V1.1 §4.1 matrix.
/// D3 row: all deny (red line).
/// D2: confirm (except E0 is confirm per spec, E3 is deny).
/// D1: allow for E0/E1, confirm for E2/E3.
/// D0: allow for E0/E1/E2, confirm for E3.
pub fn classify(e: ELevel, d: DLevel) -> Effect {
    use DLevel::*;
    use ELevel::*;
    use Effect::*;
    match (e, d) {
        // D3 row — entire row deny (red line)
        (E0 | E1 | E2 | E3, D3) => Deny,
        // D2 row — confirm except E3 deny
        (E0 | E1 | E2, D2) => Confirm,
        (E3, D2) => Deny,
        // D1 row — allow E0/E1, confirm E2/E3
        (E0 | E1, D1) => Allow,
        (E2 | E3, D1) => Confirm,
        // D0 row — allow E0/E1/E2, confirm E3
        (E0 | E1 | E2, D0) => Allow,
        (E3, D0) => Confirm,
    }
}
