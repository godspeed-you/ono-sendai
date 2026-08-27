//! The exit-status contract of ADR-0008. Surrounding scripts depend on these values, so they
//! are asserted as numbers, not as names.

use ono_core::ExitStatus;

#[test]
fn should_use_the_bourne_family_status_values_when_reporting_an_outcome() {
    assert_eq!(ExitStatus::SUCCESS.code(), 0);
    assert_eq!(ExitStatus::FAILURE.code(), 1);
    assert_eq!(ExitStatus::USAGE.code(), 2);
    assert_eq!(ExitStatus::NOT_EXECUTABLE.code(), 126);
    assert_eq!(ExitStatus::NOT_FOUND.code(), 127);
    assert_eq!(ExitStatus::INTERRUPTED.code(), 130);
}

#[test]
fn should_report_a_signalled_process_as_128_plus_the_signal_when_it_is_killed() {
    assert_eq!(ExitStatus::from_signal(2).code(), 130);
    assert_eq!(ExitStatus::from_signal(9).code(), 137);
    assert_eq!(ExitStatus::from_signal(15).code(), 143);
}

#[test]
fn should_pass_an_external_status_through_unchanged_when_the_program_chose_it() {
    // Spec section 16.4: Ono must not reinterpret a program's own status, including values that
    // collide with Ono's own conventions.
    for code in [0, 1, 2, 42, 126, 127, 130, 255] {
        assert_eq!(ExitStatus::from_code(code).code(), code);
    }
}

#[test]
fn should_report_success_only_for_zero_when_asked() {
    assert!(ExitStatus::SUCCESS.is_success());
    for code in 1..=255u8 {
        assert!(!ExitStatus::from_code(code).is_success(), "status {code}");
    }
}
