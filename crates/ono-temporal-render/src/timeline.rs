//! The timeline as text (spec v0.5 §11.5, §11.6, §11.7, §19.2, §19.3, §19.4, §24.4).
//!
//! Two renderings of one record. [`timeline`] is §11.5's default text: a row per event, a gap
//! wherever the window has one, and nothing else. [`timeline_view`] is §19.2's full-screen
//! layout: the place and the window across the top, the rows with a cursor, the evidence and
//! coverage behind them, and the keys of §19.3.
//!
//! The gap handling is the part that matters. §11.7: "A gap MUST not be hidden simply because
//! events exist on both sides." So gaps are merged into the row sequence by their own start
//! instant and never filtered by whether the interval looks busy.

use ono_value::{RecordValue, Value};

use crate::{
    Precision, RenderOptions, abbreviate_source, clock, field, fit, instant, items, pad,
    presentation_instant, record_instant, record_items, record_text, reference, refers_to, span,
    subject_width, text, value_text,
};

/// How wide the clock column is: `12:17:51.203`.
const CLOCK_WIDTH: usize = 12;

/// The kinds §19.4 never groups: a lifecycle change and an operator action are the events a
/// reader is looking for, and folding one into a count would hide it (§6.3, §18.4).
const GROUPABLE: [&str; 3] = ["object.observed", "object.changed", "provider.event"];

/// §11.5's default text rendering of an `ono.temporal-timeline/1`.
///
/// ```text
/// 12:17:51.203  config/nginx.conf       changed                    [ono]   @e17510000
/// 12:18:02.011  nginx.service           reload requested           [ono]   @e18020000
/// 12:20:00      ---- coverage gap: recorder offline 4m 12s ----
/// 12:24:12
/// ```
///
/// The width is honoured rather than assumed (v0.4 §39.3): the subject column narrows with the
/// terminal, and where a row cannot hold everything the source tag goes before the reference,
/// because §11.6 makes the reference a MUST and §11.5 makes the tag a SHOULD.
#[must_use]
pub fn timeline(view: &RecordValue, width: usize, options: &RenderOptions) -> Vec<String> {
    let width = width.max(20);
    let mut lines = Vec::new();
    for entry in entries(view, options) {
        lines.extend(entry.render(width, options, false));
    }
    if matches!(view.get("truncated"), Some(Value::Bool(true))) {
        // §19.4: a reader must be able to tell a quiet interval from a truncated one.
        lines.push(fit("  … the list was truncated by a limit", width));
    }
    lines
}

/// §19.2's full-screen timeline, and §19.3's keys under it.
///
/// The border style of the specification's sketch is illustrative and the information
/// architecture is normative, so the layout here is the house full-screen style — a header line,
/// the rows with a `>` cursor, the evidence and coverage line, the legend — carrying exactly the
/// components §19.2 names.
#[must_use]
pub fn timeline_view(view: &RecordValue, width: usize, options: &RenderOptions) -> Vec<String> {
    let width = width.max(20);
    let mut lines = vec![fit(&header(view, width, options), width), String::new()];
    for entry in entries(view, options) {
        lines.extend(entry.render(width, options, true));
    }
    if matches!(view.get("truncated"), Some(Value::Bool(true))) {
        lines.push(fit("  … the list was truncated by a limit", width));
    }
    lines.push(String::new());
    lines.push(fit(&evidence_line(view), width));
    lines.extend(legend(width));
    lines
}

/// `local/service/nginx      12:17 ------------------------- 12:25` (§19.2).
fn header(view: &RecordValue, width: usize, options: &RenderOptions) -> String {
    let place = record_text(view, "place_label")
        .or_else(|| record_text(view, "place"))
        .or_else(|| record_text(view, "scope"))
        .unwrap_or_default();
    let from = record_instant(view, "from").map(|at| clock(at, options, Precision::Minute));
    let until = record_instant(view, "until").map(|at| clock(at, options, Precision::Minute));
    let (Some(from), Some(until)) = (from, until) else {
        return format!(" {place}");
    };
    // §19.2 draws the window as a span between its ends. The rule is filler: it carries no
    // meaning colour or a glyph would have to carry instead (§45.2).
    let head = format!(" {place}   {from} ");
    let taken = head.chars().count() + until.chars().count() + 1;
    if width > taken + 3 {
        return format!("{head}{} {until}", "-".repeat(width - taken - 1));
    }
    format!("{head}- {until}")
}

/// `evidence: procfs, systemd     coverage: complete` (§19.2, §8.5).
fn evidence_line(view: &RecordValue) -> String {
    let coverage = view.get("coverage");
    let sources: Vec<String> = coverage
        .map(|coverage| items(coverage, "sources"))
        .unwrap_or_default()
        .iter()
        .filter_map(|source| match source {
            Value::String(source) => Some(abbreviate_source(source)),
            _ => None,
        })
        .collect();
    let headline = coverage
        .and_then(|coverage| text(coverage, "headline"))
        .unwrap_or_else(|| "unknown".to_owned());
    let gaps = record_items(view, "gaps").len();
    let mut line = format!(
        " evidence: {}   coverage: {headline}",
        if sources.is_empty() {
            "unknown".to_owned()
        } else {
            sources.join(", ")
        }
    );
    if gaps > 0 {
        // §11.7 again: the summary line never rounds a gap away either.
        line.push_str(&format!(
            "   {gaps} {}",
            if gaps == 1 { "gap" } else { "gaps" }
        ));
    }
    line
}

/// §19.3's keys, in the order the specification lists them.
///
/// The legend wraps rather than truncating, because a key the legend stopped naming is a key the
/// reader stops knowing about, and §19.3's list is normative.
fn legend(width: usize) -> Vec<String> {
    const KEYS: [&str; 10] = [
        "Enter inspect",
        "W why",
        "M map",
        "A at",
        "/ filter",
        "C correlations",
        "G gaps",
        "N now",
        "? help",
        "Esc exit",
    ];
    let mut lines = Vec::new();
    let mut line = String::new();
    for key in KEYS {
        let next = if line.is_empty() {
            format!(" {key}")
        } else {
            format!("{line}  {key}")
        };
        if next.chars().count() > width && !line.is_empty() {
            lines.push(line);
            line = format!(" {key}");
        } else {
            line = next;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.into_iter().map(|line| fit(&line, width)).collect()
}

/// One drawable thing in the window.
enum Entry<'a> {
    /// One event, on one row.
    Event(&'a Value),
    /// Several rows §19.4 folded into one, keeping their count and their span.
    Group(Group<'a>),
    /// A coverage gap inside the window (§11.7).
    Gap(&'a Value),
}

/// A density row: what it stands for, how many it hides and how long it took (§19.4).
///
/// `hidden` and the span are stated rather than counted from `members`, because §19.4's
/// cluster-level dimension lets a provider fold events the ledger never retained: the count the
/// producer gave is the truth, and the members are whatever §19.5 can still expand to.
struct Group<'a> {
    members: Vec<&'a Value>,
    hidden: usize,
    from: i128,
    until: i128,
}

impl<'a> Group<'a> {
    /// A run the renderer formed itself, where the producer stated no grouping.
    fn derived(members: Vec<&'a Value>) -> Self {
        let from = members
            .first()
            .and_then(|event| presentation_instant(event))
            .unwrap_or_default();
        let until = members
            .last()
            .and_then(|event| presentation_instant(event))
            .unwrap_or(from);
        Self {
            hidden: members.len().saturating_sub(1),
            members,
            from,
            until,
        }
    }
}

impl Entry<'_> {
    /// The lines this entry reads as. `cursor` draws §19.2's selection column.
    fn render(&self, width: usize, options: &RenderOptions, cursor: bool) -> Vec<String> {
        match self {
            Entry::Event(event) => vec![row(event, width, options, cursor, None)],
            Entry::Group(group) => group_rows(group, width, options, cursor),
            Entry::Gap(gap) => gap_rows(gap, width, options, cursor),
        }
    }

    /// When the entry starts, so gaps can be placed between the rows they fall between.
    fn at(&self) -> i128 {
        match self {
            Entry::Event(event) => presentation_instant(event).unwrap_or(i128::MIN),
            Entry::Group(group) => group.from,
            Entry::Gap(gap) => instant(gap, "from").unwrap_or(i128::MIN),
        }
    }
}

/// The events and the gaps of a window, in the order they are drawn.
///
/// Events keep the presentation order the query gave them (§26.3), which makes no ordering
/// claim; a gap is placed before the first event at or after its start, so the break stands
/// between the events on either side of it.
fn entries<'a>(view: &'a RecordValue, options: &RenderOptions) -> Vec<Entry<'a>> {
    let events = record_items(view, "events");
    let mut pending: Vec<&Value> = record_items(view, "gaps").iter().collect();
    pending.sort_by_key(|gap| instant(gap, "from").unwrap_or(i128::MIN));

    let rows = if options.group_repeats {
        // §19.4's grouping is the producer's judgement where it made one; the renderer's own
        // rule is the fallback for a record that carries none.
        stated_rows(view, events).unwrap_or_else(|| derived_rows(events))
    } else {
        events.iter().map(Entry::Event).collect()
    };

    let mut entries = Vec::with_capacity(rows.len() + pending.len());
    let mut next_gap = 0usize;
    for entry in rows {
        while let Some(gap) = pending.get(next_gap) {
            if instant(gap, "from").unwrap_or(i128::MAX) > entry.at() {
                break;
            }
            entries.push(Entry::Gap(gap));
            next_gap += 1;
        }
        entries.push(entry);
    }
    for gap in pending.into_iter().skip(next_gap) {
        entries.push(Entry::Gap(gap));
    }
    entries
}

/// The rows the producer's `groups` field states, or `None` where it states none.
///
/// A row naming no retained event is dropped rather than drawn empty; a row of one event that
/// hides nothing is an ordinary row.
fn stated_rows<'a>(view: &'a RecordValue, events: &'a [Value]) -> Option<Vec<Entry<'a>>> {
    let stated = match view.get("groups") {
        Some(Value::List(rows)) if !rows.is_empty() => rows,
        _ => return None,
    };
    let mut rows = Vec::with_capacity(stated.len());
    for group in stated.iter() {
        let members: Vec<&Value> = items(group, "members")
            .iter()
            .filter_map(|id| match id {
                Value::String(id) => events
                    .iter()
                    .find(|event| text(event, "event_id").is_some_and(|held| held == id.as_ref())),
                _ => None,
            })
            .collect();
        let Some(first) = members.first().copied() else {
            continue;
        };
        let hidden = usize::try_from(count(group, "hidden")).unwrap_or(0);
        if hidden == 0 && members.len() == 1 {
            rows.push(Entry::Event(first));
            continue;
        }
        let from = instant(group, "from")
            .or_else(|| presentation_instant(first))
            .unwrap_or_default();
        rows.push(Entry::Group(Group {
            hidden,
            from,
            until: instant(group, "until").unwrap_or(from),
            members,
        }));
    }
    (!rows.is_empty()).then_some(rows)
}

/// The rows this renderer forms for a record whose producer stated no grouping.
fn derived_rows(events: &[Value]) -> Vec<Entry<'_>> {
    let mut rows = Vec::new();
    let mut run: Vec<&Value> = Vec::new();
    for event in events {
        if groups_with(run.last().copied(), event) {
            run.push(event);
        } else {
            flush(&mut run, &mut rows);
            if groupable(event) {
                run.push(event);
            } else {
                rows.push(Entry::Event(event));
            }
        }
    }
    flush(&mut run, &mut rows);
    rows
}

/// A whole-number field of a sub-record, or zero where it carries none.
fn count(value: &Value, name: &str) -> i128 {
    match field(value, name) {
        Some(Value::Int(count)) => *count,
        _ => 0,
    }
}

/// Turns the run collected so far into an entry: a group where there is something to group, one
/// row where there is not.
fn flush<'a>(run: &mut Vec<&'a Value>, entries: &mut Vec<Entry<'a>>) {
    match run.len() {
        0 => {}
        1 => entries.push(Entry::Event(run[0])),
        _ => entries.push(Entry::Group(Group::derived(std::mem::take(run)))),
    }
    run.clear();
}

/// Whether §19.4 allows this kind to be folded into a group at all.
fn groupable(event: &Value) -> bool {
    text(event, "kind").is_some_and(|kind| GROUPABLE.contains(&kind.as_str()))
}

/// Whether `event` continues the run `previous` started: the same subject, the same kind and the
/// same fields (§19.4's "same object and same field in a short interval").
fn groups_with(previous: Option<&Value>, event: &Value) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    groupable(event)
        && groupable(previous)
        && text(previous, "kind") == text(event, "kind")
        && subject_label(previous) == subject_label(event)
        && changed_names(previous) == changed_names(event)
}

/// The names of the fields an event changed, which is what makes two rows the same row.
fn changed_names(event: &Value) -> Vec<String> {
    items(event, "changed_fields")
        .iter()
        .filter_map(|change| text(change, "field"))
        .collect()
}

/// A grouped row, and its members where the view expanded it (§19.4, §19.5).
fn group_rows(
    group: &Group<'_>,
    width: usize,
    options: &RenderOptions,
    cursor: bool,
) -> Vec<String> {
    let Some(first) = group.members.first() else {
        return Vec::new();
    };
    // §19.4: "Grouping MUST preserve hidden counts and time span." The count is the hidden rows
    // plus the one standing for them, and the span is the one the producer stated.
    let summary = format!(
        "x{} over {}",
        group.hidden.saturating_add(1),
        span(group.from, group.until)
    );
    let mut lines = vec![row(first, width, options, cursor, Some(&summary))];
    let expanded = text(first, "event_id")
        .is_some_and(|id| options.expanded.iter().any(|stored| refers_to(stored, &id)));
    if expanded {
        for member in &group.members {
            let line = row(member, width.saturating_sub(2), options, false, None);
            lines.push(fit(&format!("  {}", line.trim_start()), width));
        }
    }
    lines
}

/// §11.7's gap, in the two lines the specification writes it in.
///
/// ```text
/// 12:20:00        ---- coverage gap: recorder offline 4m12s ----
/// 12:24:12
/// ```
fn gap_rows(gap: &Value, width: usize, options: &RenderOptions, cursor: bool) -> Vec<String> {
    let lead = if cursor { " " } else { "" };
    let from = instant(gap, "from");
    let until = instant(gap, "until");
    let reason = text(gap, "detail").unwrap_or_else(|| {
        text(gap, "reason")
            .unwrap_or_else(|| "not recorded".to_owned())
            .replace('_', " ")
    });
    let length = match (from, until) {
        (Some(from), Some(until)) => format!(" {}", span(from, until)),
        _ => String::new(),
    };
    let opening = format!(
        "{lead}{}  ---- coverage gap: {reason}{length} ----",
        pad(
            &from.map_or_else(String::new, |at| clock(at, options, Precision::Second)),
            CLOCK_WIDTH
        )
    );
    let mut lines = vec![fit(&opening, width)];
    if let Some(until) = until {
        lines.push(fit(
            &format!("{lead}{}", clock(until, options, Precision::Second)),
            width,
        ));
    }
    lines
}

/// One event row (§11.5), with the uncertainty of §24.4 and the reference of §11.6.
fn row(
    event: &Value,
    width: usize,
    options: &RenderOptions,
    cursor: bool,
    summary: Option<&str>,
) -> String {
    let mut head = String::new();
    if cursor {
        let selected = text(event, "event_id").is_some_and(|id| {
            options
                .cursor
                .as_ref()
                .is_some_and(|stored| refers_to(stored, &id))
        });
        head.push(if selected { '>' } else { ' ' });
    }
    head.push_str(&pad(
        &presentation_instant(event)
            .map_or_else(String::new, |at| clock(at, options, Precision::Milli)),
        CLOCK_WIDTH,
    ));
    // §24.4: an instant that may be 40ms out means nothing without the clock it came from, so the
    // uncertainty and the clock domain are printed together (§25.5).
    if let Some(Value::Duration(uncertainty)) = field(event, "clock_uncertainty") {
        head.push_str(&format!(" +/- {uncertainty}"));
        if let Some(host) = text(event, "host") {
            head.push_str(&format!("  {host}"));
        }
    }

    let mut description = describe(event);
    if let Some(summary) = summary {
        description.push_str("  ");
        description.push_str(summary);
    }

    let tag = options
        .show_source_tags
        .then(|| source_tag(event).map(|tag| format!("[{tag}]")))
        .flatten();
    let reference = event_reference(event);
    compose(
        &head,
        &subject_label(event),
        &description,
        tag,
        reference,
        width,
    )
}

/// Lays a row out (v0.4 §39.3).
///
/// §11.6 makes the event reference a MUST and §11.5 makes the source tag a SHOULD, so a row that
/// cannot hold everything drops the tag first and shortens the description second; the reference
/// survives until the row itself has no room left at all.
fn compose(
    head: &str,
    subject: &str,
    description: &str,
    tag: Option<String>,
    reference: Option<String>,
    width: usize,
) -> String {
    let base = format!("{head}  {}", pad(subject, subject_width(width)));
    let bare = reference.map_or_else(String::new, |reference| format!("  {reference}"));
    let tagged = tag.map_or_else(|| bare.clone(), |tag| format!("  {tag}{bare}"));
    let body = if description.is_empty() {
        String::new()
    } else {
        format!(" {description}")
    };
    let taken = base.chars().count() + body.chars().count();

    for tail in [&tagged, &bare] {
        if taken + tail.chars().count() <= width {
            return fit(&format!("{base}{body}{tail}"), width);
        }
    }
    // Neither tail leaves room for the whole description, so the description is what gives way
    // and the reference stays (§11.6).
    let room = width.saturating_sub(base.chars().count() + bare.chars().count());
    if !description.is_empty() && room >= 2 {
        return fit(
            &format!("{base} {}{bare}", pad(description, room - 1)),
            width,
        );
    }
    fit(&format!("{base}{body}"), width)
}

/// What a person calls the subject of an event (§5.5).
///
/// An unresolved subject stays visibly unresolved: the source named something Ono could not
/// reconcile, and the row says so rather than dressing the text as an identity.
pub(crate) fn subject_label(event: &Value) -> String {
    let Some(subject) = field(event, "subject") else {
        return String::new();
    };
    subject_label_of(subject)
}

/// The same, for a subject sub-record read straight out of the record that carries it.
pub(crate) fn subject_label_of(subject: &Value) -> String {
    if let Some(label) = text(subject, "label") {
        if matches!(field(subject, "resolved"), Some(Value::Bool(false))) {
            return format!("{label} (unresolved)");
        }
        return label;
    }
    text(subject, "described")
        .map_or_else(String::new, |described| format!("{described} (unresolved)"))
}

/// What happened, in the words the event's own kind and payload give (§6.1, §6.2).
pub(crate) fn describe(event: &Value) -> String {
    let Some(kind) = text(event, "kind") else {
        return String::new();
    };
    match kind.as_str() {
        "object.observed" => "observed".to_owned(),
        "object.appeared" => "appeared".to_owned(),
        "object.disappeared" => "disappeared".to_owned(),
        "object.changed" => changed(event),
        "relation.added" => relation(event, "relation added"),
        "relation.removed" => relation(event, "relation removed"),
        "provider.event" => text(event, "subtype").unwrap_or_else(|| "provider event".to_owned()),
        other if other.starts_with("action.") => action(event, other),
        other => other.replace('.', " "),
    }
}

/// A change, with the field it touched (§6.2). A side with no evidence stays `unknown`.
fn changed(event: &Value) -> String {
    let changes = items(event, "changed_fields");
    match changes.split_first() {
        None => "changed".to_owned(),
        Some((only, [])) => {
            let field_name = text(only, "field").unwrap_or_default();
            let before = field(only, "before").map_or_else(|| "unknown".to_owned(), value_text);
            let after = field(only, "after").map_or_else(|| "unknown".to_owned(), value_text);
            let mut line = format!("changed  {field_name} {before} -> {after}");
            if let Some(certainty) = text(only, "certainty")
                && certainty != "observed"
            {
                line.push_str(&format!(" ({certainty})"));
            }
            line
        }
        Some((_, _)) => {
            let names: Vec<String> = changes
                .iter()
                .filter_map(|change| text(change, "field"))
                .collect();
            format!("changed  {} fields: {}", changes.len(), names.join(", "))
        }
    }
}

/// An action event, named by the operation the shell was asked for (§17.2).
fn action(event: &Value, kind: &str) -> String {
    let step = kind.strip_prefix("action.").unwrap_or(kind);
    match field(event, "payload").and_then(|payload| text(payload, "operation")) {
        Some(operation) => format!("{operation} {step}"),
        None => format!("action {step}"),
    }
}

/// A relation event, named by the edge that changed (§6.4).
fn relation(event: &Value, lead: &str) -> String {
    match field(event, "payload").and_then(|payload| text(payload, "relation")) {
        Some(relation) => format!("{lead}  {relation}"),
        None => lead.to_owned(),
    }
}

/// The reference §11.6 requires the row to carry.
///
/// A session that has already minted a short reference for this event puts it on the record, and
/// that is what the row prints: ADR-0660 makes a reference the shortest prefix of the digest that
/// names one event *inside a session*, so a renderer deriving its own would show the reader a
/// second spelling of one thing. Where no session reference travelled with the event, the row
/// derives a prefix of its own (ADR-0704), which resolves the same way.
fn event_reference(event: &Value) -> Option<String> {
    if let Some(given) = text(event, "reference") {
        return Some(if given.starts_with('@') {
            given
        } else {
            format!("@{given}")
        });
    }
    text(event, "event_id").map(|id| reference(&id))
}

/// The abbreviated source tag of §11.5, from the §7.1 evidence source class the event carries.
///
/// `ono.temporal-event/1` declares `source` as that class (ADR-0709), so the tag is an
/// abbreviation of a stated fact. Where an older record carries none, the provenance's provider
/// stands in, because that is the name `EventId::of` already digests as the event's source.
/// `provenance.source` is deliberately not consulted: it records what an observation was read
/// from — a procfs path, a D-Bus interface name — and abbreviating it prints `[Manager]` for a
/// systemd row.
fn source_tag(event: &Value) -> Option<String> {
    if let Some(source) = text(event, "source") {
        return Some(abbreviate_source(&source));
    }
    let provenance = field(event, "provenance")?;
    Some(abbreviate_source(&text(provenance, "provider")?))
}
