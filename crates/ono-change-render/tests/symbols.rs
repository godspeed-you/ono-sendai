//! §20.3's fixed visual language, and Appendix E.7's recovery marks.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_core::{EffectKind, ProtectionLevel};
use ono_change_render::{Charset, Symbol, legend};

#[test]
fn should_offer_both_a_unicode_and_an_ascii_form_for_every_symbol() {
    for symbol in Symbol::ALL {
        assert!(
            !symbol.ascii().is_empty(),
            "§20.3: ASCII fallback MUST exist, and {symbol:?} has none"
        );
        assert!(
            !symbol.unicode().is_empty(),
            "§20.3: a symbol without a glyph cannot carry a meaning ({symbol:?})"
        );
    }
}

#[test]
fn should_spell_the_ascii_symbols_exactly_as_the_specification_writes_them() {
    let expected = [
        (Symbol::Addition, "+"),
        (Symbol::Removal, "-"),
        (Symbol::Modification, "~"),
        (Symbol::Unknown, "?"),
        (Symbol::Risk, "!"),
        (Symbol::RecoveryAvailable, "<->"),
        (Symbol::PartialCoverage, "1/2"),
        (Symbol::CompensationOnly, "~>"),
    ];
    for (symbol, glyph) in expected {
        assert_eq!(
            symbol.ascii(),
            glyph,
            "§20.3 and Appendix E.7 fix the compact language; {symbol:?} may not drift"
        );
    }
}

#[test]
fn should_keep_every_symbol_distinct_in_the_ascii_alphabet() {
    let mut marks: Vec<&str> = Symbol::ALL.iter().map(|symbol| symbol.ascii()).collect();
    marks.sort_unstable();
    let before = marks.len();
    marks.dedup();
    assert_eq!(
        marks.len(),
        before,
        "§20.3 fixes one mark per meaning; two meanings sharing a mark is an unreadable view"
    );
}

#[test]
fn should_keep_every_symbol_distinct_in_the_unicode_alphabet() {
    let mut marks: Vec<&str> = Symbol::ALL.iter().map(|symbol| symbol.unicode()).collect();
    marks.sort_unstable();
    let before = marks.len();
    marks.dedup();
    assert_eq!(
        marks.len(),
        before,
        "§20.3 permits better glyphs and never permits two meanings to collide"
    );
}

#[test]
fn should_draw_the_ascii_form_when_the_session_chose_ascii() {
    assert_eq!(
        Symbol::RecoveryAvailable.glyph(Charset::Ascii),
        "<->",
        "§20.3: an ASCII session sees the ASCII alphabet and loses no meaning"
    );
    assert_ne!(
        Symbol::RecoveryAvailable.glyph(Charset::Unicode),
        Symbol::RecoveryAvailable.glyph(Charset::Ascii),
        "§20.3 permits a better glyph where terminal support is known"
    );
}

#[test]
fn should_give_every_symbol_a_meaning_a_reader_can_look_up() {
    for symbol in Symbol::ALL {
        assert!(
            !symbol.meaning().is_empty(),
            "§20.3 fixes a language, and a language needs a glossary ({symbol:?})"
        );
    }
}

#[test]
fn should_agree_with_the_protection_level_symbol_the_core_declares() {
    for level in ProtectionLevel::ALL {
        assert_eq!(
            Symbol::for_protection(*level).ascii(),
            level.symbol(),
            "§50.1: which mark a level carries is a fact of the level, not of the renderer"
        );
    }
}

#[test]
fn should_mark_an_emitted_effect_as_risk_rather_than_as_an_addition() {
    assert_eq!(
        Symbol::for_effect(EffectKind::Emit),
        Symbol::Risk,
        "§35.1: an outward call has already left, which is the boundary `!` marks"
    );
    assert_eq!(Symbol::for_effect(EffectKind::Create), Symbol::Addition);
    assert_eq!(Symbol::for_effect(EffectKind::Remove), Symbol::Removal);
    assert_eq!(Symbol::for_effect(EffectKind::Modify), Symbol::Modification);
    assert_eq!(Symbol::for_effect(EffectKind::Unknown), Symbol::Unknown);
}

#[test]
fn should_print_a_legend_holding_every_symbol_of_the_language() {
    let lines = legend(80, Charset::Ascii);
    assert_eq!(
        lines.len(),
        Symbol::ALL.len(),
        "§20.3: a fixed language is only usable if all of it can be looked up"
    );
    for symbol in Symbol::ALL {
        assert!(
            lines.iter().any(|line| line.contains(symbol.meaning())),
            "the legend omits {symbol:?}, which §20.3 fixes"
        );
    }
}

#[test]
fn should_default_to_the_ascii_alphabet_every_terminal_can_show() {
    assert_eq!(
        Charset::default(),
        Charset::Ascii,
        "§20.3 makes ASCII the guaranteed alphabet, so it is the one a caller falls back to"
    );
}
