//! One test per schema-evolution rule of spec §10.4.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use ono_value::{
    Compatibility, FieldDef, FieldType, Schema, SchemaChangeKind, SchemaId, Unit, classify_change,
};

fn base() -> Schema {
    Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).required())
        .field(FieldDef::new("size", FieldType::Int).nullable())
        .identity(["id"])
        .default_view(["id"])
        .build()
        .unwrap()
}

#[test]
fn should_classify_adding_an_optional_field_as_compatible() {
    let next = Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).required())
        .field(FieldDef::new("size", FieldType::Int).nullable())
        .field(FieldDef::new("note", FieldType::String).nullable())
        .identity(["id"])
        .default_view(["id"])
        .build()
        .unwrap();

    let diff = classify_change(&base(), &next);

    assert_eq!(diff.compatibility(), Compatibility::Compatible);
    assert!(
        diff.changes()
            .iter()
            .any(|change| change.kind() == SchemaChangeKind::FieldAdded),
        "the added field must appear in the diff"
    );
}

#[test]
fn should_classify_adding_a_required_field_as_breaking() {
    let next = Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).required())
        .field(FieldDef::new("size", FieldType::Int).nullable())
        .field(FieldDef::new("note", FieldType::String).required())
        .identity(["id"])
        .default_view(["id"])
        .build()
        .unwrap();

    assert_eq!(
        classify_change(&base(), &next).compatibility(),
        Compatibility::Breaking
    );
}

#[test]
fn should_classify_removing_a_field_as_breaking() {
    let next = Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).required())
        .identity(["id"])
        .default_view(["id"])
        .build()
        .unwrap();

    let diff = classify_change(&base(), &next);

    assert_eq!(diff.compatibility(), Compatibility::Breaking);
    assert!(
        diff.changes()
            .iter()
            .any(|change| change.kind() == SchemaChangeKind::FieldRemoved)
    );
}

#[test]
fn should_classify_renaming_a_field_as_breaking() {
    let next = Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).required())
        .field(FieldDef::new("magnitude", FieldType::Int).nullable())
        .identity(["id"])
        .default_view(["id"])
        .build()
        .unwrap();

    let diff = classify_change(&base(), &next);

    assert_eq!(
        diff.compatibility(),
        Compatibility::Breaking,
        "a rename is indistinguishable from a removal plus an addition, and both must be reported"
    );
    assert!(
        diff.changes()
            .iter()
            .any(|change| change.kind() == SchemaChangeKind::FieldRemoved)
    );
    assert!(
        diff.changes()
            .iter()
            .any(|change| change.kind() == SchemaChangeKind::FieldAdded)
    );
}

#[test]
fn should_classify_widening_a_numeric_field_losslessly_as_compatible() {
    let next = Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).required())
        .field(FieldDef::new("size", FieldType::Decimal).nullable())
        .identity(["id"])
        .default_view(["id"])
        .build()
        .unwrap();

    let diff = classify_change(&base(), &next);

    assert_eq!(diff.compatibility(), Compatibility::Compatible);
    assert!(
        diff.changes()
            .iter()
            .any(|change| change.kind() == SchemaChangeKind::FieldTypeWidened)
    );
}

#[test]
fn should_classify_narrowing_a_numeric_field_as_breaking() {
    let wide = Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).required())
        .field(FieldDef::new("size", FieldType::Float).nullable())
        .identity(["id"])
        .default_view(["id"])
        .build()
        .unwrap();

    let diff = classify_change(&wide, &base());

    assert_eq!(diff.compatibility(), Compatibility::Breaking);
    assert!(
        diff.changes()
            .iter()
            .any(|change| change.kind() == SchemaChangeKind::FieldTypeChanged)
    );
}

#[test]
fn should_classify_changing_the_unit_of_a_field_as_breaking() {
    let with_unit = Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).required())
        .field(
            FieldDef::new("size", FieldType::Int)
                .nullable()
                .with_unit(Unit::Bytes),
        )
        .identity(["id"])
        .default_view(["id"])
        .build()
        .unwrap();
    let with_other_unit = Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).required())
        .field(
            FieldDef::new("size", FieldType::Int)
                .nullable()
                .with_unit(Unit::Seconds),
        )
        .identity(["id"])
        .default_view(["id"])
        .build()
        .unwrap();

    let diff = classify_change(&with_unit, &with_other_unit);

    assert_eq!(diff.compatibility(), Compatibility::Breaking);
    assert!(
        diff.changes()
            .iter()
            .any(|change| change.kind() == SchemaChangeKind::FieldUnitChanged)
    );
}

#[test]
fn should_classify_making_a_field_nullable_as_breaking() {
    let next = Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).nullable())
        .field(FieldDef::new("size", FieldType::Int).nullable())
        .identity(["id"])
        .default_view(["id"])
        .build()
        .unwrap();

    let diff = classify_change(&base(), &next);

    assert_eq!(
        diff.compatibility(),
        Compatibility::Breaking,
        "a consumer that never had to handle null now does"
    );
}

#[test]
fn should_classify_changing_the_identity_as_breaking() {
    let next = Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).required())
        .field(FieldDef::new("size", FieldType::Int).nullable())
        .identity(["id", "size"])
        .default_view(["id"])
        .build()
        .unwrap();

    let diff = classify_change(&base(), &next);

    assert_eq!(diff.compatibility(), Compatibility::Breaking);
    assert!(
        diff.changes()
            .iter()
            .any(|change| change.kind() == SchemaChangeKind::IdentityChanged)
    );
}

#[test]
fn should_classify_changing_only_the_default_view_as_compatible() {
    let next = Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
        .field(FieldDef::new("id", FieldType::Int).required())
        .field(FieldDef::new("size", FieldType::Int).nullable())
        .identity(["id"])
        .default_view(["id", "size"])
        .build()
        .unwrap();

    let diff = classify_change(&base(), &next);

    assert_eq!(
        diff.compatibility(),
        Compatibility::Compatible,
        "a table is a view, not a value (spec §10.7)"
    );
    assert!(
        diff.changes()
            .iter()
            .any(|change| change.kind() == SchemaChangeKind::DefaultViewChanged)
    );
}

#[test]
fn should_report_no_change_between_a_schema_and_itself() {
    let diff = classify_change(&base(), &base());

    assert!(diff.changes().is_empty(), "an identical schema has no diff");
    assert_eq!(diff.compatibility(), Compatibility::Compatible);
}
