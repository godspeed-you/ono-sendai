//! Spans are byte offsets into a source line. Diagnostics of spec section 16.3 point at them,
//! so their arithmetic is a contract of its own.

use ono_core::Span;

#[test]
fn should_describe_a_half_open_byte_range_when_constructed() {
    let span = Span::new(3, 7);
    assert_eq!(span.start(), 3);
    assert_eq!(span.end(), 7);
    assert_eq!(span.len(), 4);
    assert!(!span.is_empty());
}

#[test]
fn should_be_empty_when_start_and_end_coincide() {
    let span = Span::new(5, 5);
    assert!(span.is_empty());
    assert_eq!(span.len(), 0);
}

#[test]
fn should_cover_both_operands_when_two_spans_are_joined() {
    let joined = Span::new(2, 5).join(Span::new(9, 11));
    assert_eq!(joined, Span::new(2, 11));
    assert_eq!(Span::new(9, 11).join(Span::new(2, 5)), Span::new(2, 11));
}

#[test]
fn should_slice_the_source_it_points_at_when_applied() {
    let source = "get process | where cpu > 20";
    assert_eq!(Span::new(0, 11).of(source), "get process");
    assert_eq!(Span::new(14, 19).of(source), "where");
}

#[test]
fn should_yield_nothing_when_it_points_outside_the_source() {
    // A diagnostic must never panic while rendering, however wrong its span is.
    assert_eq!(Span::new(0, 900).of("short"), "");
    assert_eq!(Span::new(900, 901).of("short"), "");
}

#[test]
fn should_report_a_one_based_line_and_column_when_located_in_a_source() {
    let source = "get process\nwhere cpu > 20";
    let (line, column) = Span::new(0, 3).line_column(source);
    assert_eq!((line, column), (1, 1));
    let (line, column) = Span::new(12, 17).line_column(source);
    assert_eq!((line, column), (2, 1));
    let (line, column) = Span::new(18, 21).line_column(source);
    assert_eq!((line, column), (2, 7));
}
