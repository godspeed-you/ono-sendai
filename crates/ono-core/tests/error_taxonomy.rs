//! The error taxonomy of spec section 43 is a public contract: codes are stable, unique, and
//! each has exactly one kind (ADR-0006). These tests assert the contract, not the enum.

use ono_core::{ErrorCode, ErrorKind};

#[test]
fn should_render_every_code_in_the_specified_form_when_displayed() {
    for code in ErrorCode::ALL {
        let rendered = code.code();
        assert!(
            rendered.starts_with("Ono-Sendai-E"),
            "spec section 43 writes codes as `Ono-Sendai-ENNNN`, got {rendered}"
        );
        let digits = &rendered["Ono-Sendai-E".len()..];
        assert_eq!(
            digits.len(),
            4,
            "code numbers are four digits, got {rendered}"
        );
        assert!(
            digits.chars().all(|c| c.is_ascii_digit()),
            "code numbers are numeric, got {rendered}"
        );
    }
}

#[test]
fn should_give_every_code_a_unique_number_and_name_when_enumerated() {
    let mut numbers: Vec<&str> = ErrorCode::ALL.iter().map(|c| c.code()).collect();
    let count = numbers.len();
    numbers.sort_unstable();
    numbers.dedup();
    assert_eq!(numbers.len(), count, "error code numbers must be unique");

    let mut names: Vec<&str> = ErrorCode::ALL.iter().map(|c| c.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), count, "error code names must be unique");
}

#[test]
fn should_name_every_code_as_a_dotted_family_selector_when_asked() {
    for code in ErrorCode::ALL {
        let name = code.name();
        let (family, rest) = name
            .split_once('.')
            .unwrap_or_else(|| panic!("`{name}` is not a `family.detail` selector"));
        assert!(
            !family.is_empty() && !rest.is_empty(),
            "empty part in {name}"
        );
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
            "selectors are lowercase snake/dotted, got {name}"
        );
    }
}

#[test]
fn should_carry_the_codes_specified_in_section_43_when_enumerated() {
    // Every code the specification lists, verbatim. A code missing here means the taxonomy
    // drifted from spec section 43.
    let specified = [
        ("Ono-Sendai-E0001", "parse.syntax"),
        ("Ono-Sendai-E0002", "parse.incomplete"),
        ("Ono-Sendai-E0101", "resolve.command_not_found"),
        ("Ono-Sendai-E0102", "resolve.target_not_found"),
        ("Ono-Sendai-E0103", "resolve.ambiguous"),
        ("Ono-Sendai-E0201", "type.mismatch"),
        ("Ono-Sendai-E0202", "type.unknown_field"),
        ("Ono-Sendai-E0203", "type.invalid_unit"),
        ("Ono-Sendai-E0301", "io.not_found"),
        ("Ono-Sendai-E0302", "io.permission_denied"),
        ("Ono-Sendai-E0303", "io.already_exists"),
        ("Ono-Sendai-E0304", "io.not_directory"),
        ("Ono-Sendai-E0401", "provider.unavailable"),
        ("Ono-Sendai-E0402", "provider.unsupported"),
        ("Ono-Sendai-E0403", "provider.schema_violation"),
        ("Ono-Sendai-E0501", "external.exit_nonzero"),
        ("Ono-Sendai-E0502", "external.signal"),
        ("Ono-Sendai-E0601", "remote.unreachable"),
        ("Ono-Sendai-E0602", "remote.protocol_mismatch"),
        ("Ono-Sendai-E0603", "remote.host_key_changed"),
        ("Ono-Sendai-E0701", "safety.confirmation_required"),
        ("Ono-Sendai-E0702", "safety.policy_denied"),
        ("Ono-Sendai-E0801", "stream.unbounded_operation"),
        ("Ono-Sendai-E0802", "stream.cancelled"),
        ("Ono-Sendai-E0803", "stream.backpressure_timeout"),
    ];

    for (number, name) in specified {
        let found = ErrorCode::ALL
            .iter()
            .find(|c| c.code() == number)
            .unwrap_or_else(|| panic!("spec section 43 defines {number} {name}, which is missing"));
        assert_eq!(found.name(), name, "wrong name for {number}");
    }
}

#[test]
fn should_resolve_a_code_from_its_selector_when_parsed() {
    for code in ErrorCode::ALL {
        assert_eq!(
            ErrorCode::from_name(code.name()),
            Some(*code),
            "`{}` must round-trip through its selector",
            code.name()
        );
    }
    assert_eq!(ErrorCode::from_name("nonsense.code"), None);
}

#[test]
fn should_classify_permission_denial_as_a_permission_error_when_asked() {
    // ADR-0006: io.permission_denied carries the `permission` kind, not `io`, because a script
    // branching on kind needs to tell a missing file from a forbidden one.
    assert_eq!(ErrorCode::IoPermissionDenied.kind(), ErrorKind::Permission);
    assert_eq!(ErrorCode::IoNotFound.kind(), ErrorKind::Io);
}

#[test]
fn should_classify_a_changed_host_key_as_a_safety_error_when_asked() {
    // ADR-0006: a changed host key is a trust decision, not a transport failure.
    assert_eq!(ErrorCode::RemoteHostKeyChanged.kind(), ErrorKind::Safety);
    assert_eq!(ErrorCode::RemoteUnreachable.kind(), ErrorKind::Provider);
}

#[test]
fn should_name_every_kind_as_the_specification_spells_it_when_rendered() {
    let expected = [
        (ErrorKind::Resolution, "resolution"),
        (ErrorKind::Permission, "permission"),
        (ErrorKind::Io, "io"),
        (ErrorKind::Parse, "parse"),
        (ErrorKind::Type, "type"),
        (ErrorKind::Provider, "provider"),
        (ErrorKind::External, "external"),
        (ErrorKind::Conflict, "conflict"),
        (ErrorKind::Timeout, "timeout"),
        (ErrorKind::Cancelled, "cancelled"),
        (ErrorKind::Safety, "safety"),
        (ErrorKind::Stream, "stream"),
    ];
    for (kind, name) in expected {
        assert_eq!(kind.as_str(), name);
        assert_eq!(ErrorKind::from_name(name), Some(kind));
    }
}
