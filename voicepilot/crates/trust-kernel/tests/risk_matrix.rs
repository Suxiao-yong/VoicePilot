use trust_kernel::policy::risk_matrix::classify;
use trust_kernel::policy::types::{DLevel::*, Effect::*, ELevel::*};

#[test]
fn d3_row_is_all_deny_red_line() {
    for e in [E0, E1, E2, E3] {
        assert_eq!(classify(e, D3), Deny, "D3×{:?} must be deny", e);
    }
}

#[test]
fn d2_row_confirms_except_e3_which_denies() {
    assert_eq!(classify(E0, D2), Confirm);
    assert_eq!(classify(E1, D2), Confirm);
    assert_eq!(classify(E2, D2), Confirm);
    assert_eq!(classify(E3, D2), Deny);
}

#[test]
fn d1_row_allows_low_e_confirms_high_e() {
    assert_eq!(classify(E0, D1), Allow);
    assert_eq!(classify(E1, D1), Allow);
    assert_eq!(classify(E2, D1), Confirm);
    assert_eq!(classify(E3, D1), Confirm);
}

#[test]
fn d0_row_allows_except_e3_confirms() {
    assert_eq!(classify(E0, D0), Allow);
    assert_eq!(classify(E1, D0), Allow);
    assert_eq!(classify(E2, D0), Allow);
    assert_eq!(classify(E3, D0), Confirm);
}

#[test]
fn all_16_cells_covered() {
    // Sanity: every (E, D) combination returns a valid Effect.
    for e in [E0, E1, E2, E3] {
        for d in [D0, D1, D2, D3] {
            let _ = classify(e, d); // panics if not covered
        }
    }
}
