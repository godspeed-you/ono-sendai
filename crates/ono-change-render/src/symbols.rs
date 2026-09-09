//! The fixed visual language of §20.3 and Appendix E.7.
//!
//! §20.3 calls the compact language *fixed*: eight marks, each with one meaning, so an operator
//! who learns `!` in a plan reads the same `!` in a map, a coverage matrix and a recovery view.
//! The list here is that list, plus Appendix E.7's `~>` for compensation, which §27.4 keeps
//! separate from restore because compensating is not putting the prior bytes back.
//!
//! §20.3 permits better glyphs "when terminal support is known" and requires the ASCII fallback
//! to exist, which is the same contract [`ono_spatial_render::Charset`] carries for the map. A
//! [`Charset`] therefore chooses between two complete alphabets rather than between decorated and
//! undecorated output: nothing is dropped in ASCII, and no meaning lives in a glyph one of the
//! two alphabets lacks.
//!
//! Appendix E.8 is the rule that keeps the alphabet honest: **there is no symbol that means
//! "safe"**. `<->` says a recovery path exists, and it is rendered only by
//! [`crate::protection::protection_block`], which cannot emit it without the exclusions beside
//! it. Nothing here maps a protection level to a badge on its own.

use ono_change_core::{EffectKind, ProtectionLevel};

/// Which characters the terminal can be promised (§20.3: "ASCII fallback MUST exist").
///
/// The two variants mirror `ono_spatial_render::Charset`, because a session that chose ASCII for
/// the map has chosen it for the plan as well, and a reader who sees `-->` in one view should not
/// meet `──▸` in the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Charset {
    /// Arrows and mathematical marks, where the terminal and the locale support them.
    Unicode,
    /// Plain ASCII, which every terminal can show. The fallback §20.3 requires.
    #[default]
    Ascii,
}

/// One mark of §20.3's and Appendix E.7's fixed language.
///
/// The enumeration is closed for the same reason §10.2's protection levels are: a view that can
/// invent a ninth mark can invent a meaning, and the operator has no way to look it up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Symbol {
    /// §20.3: an object the plan proposes to bring into being.
    Addition,
    /// §20.3: an object the plan proposes to take away.
    Removal,
    /// §20.3: an object that exists and whose state the plan proposes to change.
    Modification,
    /// §20.3 and §8.1: an effect Ono has no justified model for. §2.4 forbids promoting it.
    Unknown,
    /// §20.3 and Appendix E.7: risk, or the boundary past which nothing can be undone (§2.13).
    Risk,
    /// §20.3 and Appendix E.7: a validated restore asset covers this.
    RecoveryAvailable,
    /// §20.3 and Appendix E.7: some of what this covers is covered, and some is not (§10.2).
    PartialCoverage,
    /// Appendix E.7 and §27.4: an inverse action exists, and no prior state image does.
    CompensationOnly,
}

impl Symbol {
    /// Every mark, in the order §20.3 and Appendix E.7 list them.
    pub const ALL: &'static [Symbol] = &[
        Symbol::Addition,
        Symbol::Removal,
        Symbol::Modification,
        Symbol::Unknown,
        Symbol::Risk,
        Symbol::RecoveryAvailable,
        Symbol::PartialCoverage,
        Symbol::CompensationOnly,
    ];

    /// The ASCII form, which is the one §20.3 writes down and therefore the normative one.
    #[must_use]
    pub const fn ascii(self) -> &'static str {
        match self {
            Symbol::Addition => "+",
            Symbol::Removal => "-",
            Symbol::Modification => "~",
            Symbol::Unknown => "?",
            Symbol::Risk => "!",
            Symbol::RecoveryAvailable => "<->",
            Symbol::PartialCoverage => "1/2",
            Symbol::CompensationOnly => "~>",
        }
    }

    /// The form for a terminal known to render arrows and mathematical marks (§20.3).
    ///
    /// Each one is a single cell wide, so a column of marks stays a column whichever alphabet the
    /// session chose, and no meaning is carried by a glyph the ASCII alphabet has no answer for.
    #[must_use]
    pub const fn unicode(self) -> &'static str {
        match self {
            Symbol::Addition => "+",
            Symbol::Removal => "\u{2212}",
            Symbol::Modification => "~",
            Symbol::Unknown => "?",
            Symbol::Risk => "!",
            Symbol::RecoveryAvailable => "\u{2194}",
            Symbol::PartialCoverage => "\u{00bd}",
            Symbol::CompensationOnly => "\u{21dd}",
        }
    }

    /// The mark as `charset` draws it.
    #[must_use]
    pub const fn glyph(self, charset: Charset) -> &'static str {
        match charset {
            Charset::Unicode => self.unicode(),
            Charset::Ascii => self.ascii(),
        }
    }

    /// The sentence `help` prints beside the mark, so the language can be looked up (§20.3).
    #[must_use]
    pub const fn meaning(self) -> &'static str {
        match self {
            Symbol::Addition => "proposed addition",
            Symbol::Removal => "proposed removal",
            Symbol::Modification => "proposed modification",
            Symbol::Unknown => "unknown effect",
            Symbol::Risk => "risk / irreversible boundary",
            Symbol::RecoveryAvailable => "recovery available",
            Symbol::PartialCoverage => "partial coverage",
            Symbol::CompensationOnly => "compensation only",
        }
    }

    /// The mark §20.3 gives an effect of this kind.
    ///
    /// `Emit` is a risk mark rather than an addition: §35.1's outward call has already happened by
    /// the time anything could reconsider it, which is what
    /// [`EffectKind::is_inherently_irreversible`] says and what `!` means.
    #[must_use]
    pub const fn for_effect(kind: EffectKind) -> Self {
        match kind {
            EffectKind::Create => Symbol::Addition,
            EffectKind::Remove => Symbol::Removal,
            EffectKind::Modify | EffectKind::Replace => Symbol::Modification,
            EffectKind::Interrupt | EffectKind::Emit => Symbol::Risk,
            EffectKind::Unknown => Symbol::Unknown,
        }
    }

    /// The mark §20.3 gives a protection level.
    ///
    /// The mapping is [`ProtectionLevel::symbol`]'s, read rather than restated: §50.1 forbids a
    /// renderer from computing a fact, and which mark a level carries is a fact of the level. The
    /// crate's tests hold the two spellings together so a change in core cannot drift past here.
    #[must_use]
    pub const fn for_protection(level: ProtectionLevel) -> Self {
        match level {
            ProtectionLevel::Protected | ProtectionLevel::Transactional => {
                Symbol::RecoveryAvailable
            }
            ProtectionLevel::PartiallyProtected => Symbol::PartialCoverage,
            ProtectionLevel::Compensatable => Symbol::CompensationOnly,
            ProtectionLevel::Unprotected => Symbol::Risk,
            ProtectionLevel::Unknown => Symbol::Unknown,
        }
    }
}

/// The lines `help symbols` prints: every mark of §20.3 with what it means.
///
/// The legend exists because §20.3 fixes a language rather than decorating output. A reader who
/// meets `1/2` in a coverage matrix has to be able to find out that it is not a fraction.
#[must_use]
pub fn legend(width: usize, charset: Charset) -> Vec<String> {
    let marks: Vec<&'static str> = Symbol::ALL
        .iter()
        .map(|symbol| symbol.glyph(charset))
        .collect();
    let column = marks
        .iter()
        .map(|mark| crate::display_width(mark))
        .max()
        .unwrap_or(1);
    Symbol::ALL
        .iter()
        .zip(marks)
        .map(|(symbol, mark)| {
            let padding = " ".repeat(column.saturating_sub(crate::display_width(mark)));
            crate::fit(&format!("  {mark}{padding}  {}", symbol.meaning()), width)
        })
        .collect()
}
