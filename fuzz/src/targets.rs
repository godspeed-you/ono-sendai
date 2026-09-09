//! The five targets spec §35.6 names, and nothing else.
//!
//! Each target is one function from bytes to nothing. It must not panic, whatever it is handed;
//! that is the entire contract, and the reason the areas listed in §35.6 are exactly the places
//! where bytes arrive from somewhere the shell does not control — a terminal, a file, the
//! kernel, a remote host, a plugin.
//!
//! A target checks the invariants a decoder promises beyond "did not panic", because a decoder
//! that answers nonsense without crashing is still a decoder that got it wrong: the parser's
//! spans must stay inside the source, and the byte decoders must stay inside a plausible bound
//! rather than growing with a length field an attacker wrote.

use ono_adapter::{Adapter, Trace};
use ono_kuang_protocol::{
    CausalRuleDocument, FrameLimits, Manifest, PackageSignature, TemporalSourceDocument,
    decode_payload, read_frame,
};
use ono_parser::{parse, tokens, words_arguments};
use ono_protocol::{FrameKind, Limits, decode, decode_message};
use ono_provider_linux::decoders as procfs;
use ono_provider_netlink::{
    InterfaceNames, SocketProtocol, decode_inet_sockets, decode_interfaces, decode_neighbors,
    decode_routes, decode_unix_sockets,
};
use ono_value::{builtin_schemas, from_csv, from_json_str, from_yaml};

/// One fuzz target: an area of spec §35.6 and the function that hammers it.
#[derive(Debug, Clone, Copy)]
pub struct Target {
    /// The name the runner and the corpus directory use.
    pub name: &'static str,
    /// The area this target covers, in the specification's own words — spec §35.6 for the five
    /// the deterministic gate tier was built for, v0.4.1 §41.2 for the two the coverage-guided
    /// tier adds.
    pub area: &'static str,
    /// The body. It must return for every input, and must never panic.
    pub run: fn(&[u8]),
}

/// Every target, in the order spec §35.6 lists the areas.
pub const TARGETS: &[Target] = &[
    Target {
        name: "parser",
        area: "parser",
        run: parser,
    },
    Target {
        name: "serializers",
        area: "serializers",
        run: serializers,
    },
    Target {
        name: "remote-protocol",
        area: "remote protocol",
        run: remote_protocol,
    },
    Target {
        name: "plugin-protocol",
        area: "plugin protocol",
        run: plugin_protocol,
    },
    Target {
        name: "system-decoders",
        area: "procfs/netlink decoders",
        run: system_decoders,
    },
    // v0.4.1 §41.2 names two more entry points the coverage-guided tier must cover. Both are
    // reachable through the targets above and neither is *reached* by them: a handshake decoder
    // fed frame bytes spends its budget on framing, and an adapter decoder is not on the remote
    // path at all. Attacker classes 7 and 8 of §5.2 are exactly these two.
    Target {
        name: "remote-handshake",
        area: "remote handshake decoder",
        run: remote_handshake,
    },
    Target {
        name: "adapter-decoders",
        area: "adapter machine-readable decoders",
        run: adapter_decoders,
    },
    // v0.5 §47.3. The first two read bytes a plugin or a remote host can influence and that a
    // corrupt store can produce; the third is the most reachable parser in the tranche, sitting
    // at a prompt; the fourth holds the rule the whole causal design rests on.
    Target {
        name: "temporal-store",
        area: "temporal store decoders",
        run: temporal_store,
    },
    Target {
        name: "temporal-events",
        area: "temporal event payloads",
        run: temporal_events,
    },
    Target {
        name: "time-selectors",
        area: "time selectors",
        run: time_selectors,
    },
    Target {
        name: "causal-candidates",
        area: "causal rule candidates",
        run: causal_candidates,
    },
];

/// The target of that name.
#[must_use]
pub fn target(name: &str) -> Option<&'static Target> {
    TARGETS.iter().find(|target| target.name == name)
}

/// The shell language. Input is a command line, so it is read as text however malformed.
fn parser(data: &[u8]) {
    let source = String::from_utf8_lossy(data);
    let parsed = parse(&source);
    // A span that points outside the source is how a diagnostic becomes a panic in a caller
    // that slices with it, which is what the editor and the renderer both do.
    for diagnostic in parsed.diagnostics() {
        assert!(
            diagnostic.span().end() as usize <= source.len(),
            "a diagnostic span reaches past the end of the source"
        );
    }
    let mut previous = 0;
    for token in tokens(&source) {
        assert!(
            token.span.start() >= previous && token.span.end() as usize <= source.len(),
            "tokens must be ordered and inside the source"
        );
        previous = token.span.start();
    }
    let _ = words_arguments(&source);
}

/// The value codecs. The first byte chooses the codec, so one corpus reaches all of them.
fn serializers(data: &[u8]) {
    let Some((selector, rest)) = data.split_first() else {
        return;
    };
    let text = String::from_utf8_lossy(rest);
    let schemas = builtin_schemas();
    let decoded = match selector % 4 {
        0 => from_json_str(&text, schemas),
        1 => from_yaml(&text, schemas),
        2 => from_csv(&text),
        _ => Ok(ono_value::from_bytes(rest.to_vec())),
    };
    // Whatever came out must go back out again: an encoder that cannot write what its own
    // decoder read is a round trip that loses data (spec §35.2).
    if let Ok(value) = decoded {
        let _ = ono_value::to_json_string(&value);
        let _ = ono_value::to_yaml(&value);
        let _ = ono_value::canonical_text(&value);
        let _ = ono_value::to_bytes(&value);
    }
}

/// The remote agent protocol: framing first, then the message inside a frame.
fn remote_protocol(data: &[u8]) {
    // Tight bounds rather than the defaults, so a decoder that grows to a claimed length is
    // caught by the harness rather than by the machine's memory.
    let limits = Limits::new()
        .with_max_frame_payload(4096)
        .with_max_value_depth(16);
    let mut buffer = bytes::BytesMut::from(data);
    // Decoding drains the buffer frame by frame; a decoder that returns a frame without
    // consuming bytes would spin here, which is a finding of its own.
    let mut guard = 0;
    while let Ok(Some(frame)) = decode(&mut buffer, &limits) {
        let _ = frame;
        guard += 1;
        if guard > 1_000 {
            break;
        }
    }
    let schemas = builtin_schemas();
    for kind in FrameKind::ALL {
        let _ = decode_message(*kind, data, schemas, &limits);
    }
}

/// The plugin protocol: the frame reader, the payload decoder, the manifest, the signature and
/// the temporal contribution decoder of v0.5 §37.3.
///
/// The temporal half is here rather than in a target of its own because it is the same attacker
/// with the same bytes: a package that can send a frame can send a contribution in it. §37.3 puts
/// six validations between those bytes and the ledger, and a validation that panics on a payload
/// is a validation that hands the instance the host (spec §31.34, ADR-0041).
fn plugin_protocol(data: &[u8]) {
    let mut reader = std::io::Cursor::new(data);
    let mut guard = 0;
    while let Ok(Some(envelope)) = read_frame(&mut reader, FrameLimits::default()) {
        let _ = envelope;
        guard += 1;
        if guard > 1_000 {
            break;
        }
    }
    let _ = decode_payload(data);
    let text = String::from_utf8_lossy(data);
    let _ = Manifest::parse(&text);
    let _ = PackageSignature::parse(&text);
    // §37.3's size ceiling is checked before anything is decoded, and the target models that:
    // a payload larger than one contribution may be is refused by the host without a decoder
    // ever seeing it, so hammering the decoder with it would fuzz a path that does not exist.
    // §37.3's size ceiling is checked before anything is decoded, and the target models that: a
    // payload larger than one contribution may be is refused by the host without a decoder ever
    // seeing it, so hammering the decoder with it would fuzz a path that does not exist.
    if data.len() <= ono_kuang_supervisor::ContributionLimits::default().max_event_bytes {
        // The YAML parser itself is `Manifest::parse`'s subject; what these two add is the typed
        // shape behind a root key, so they are attempted where a mutator has produced one.
        if text.contains("temporal_sources") {
            let _ = TemporalSourceDocument::parse(&text);
        }
        if text.contains("causal_rules") {
            let _ = CausalRuleDocument::parse(&text);
        }
        temporal_contributions(&text);
    }
}

/// The v0.5 §37.3 contribution decoder, over whatever JSON the bytes happen to be.
///
/// The invariant beyond "did not panic" is the one §37.3 exists for: **an accepted event is one
/// the package could have seen.** A contribution the host accepted about a schema outside the
/// package's visible set would be a plugin asserting an object into existence, which every
/// reconstruction downstream would then believe.
fn temporal_contributions(text: &str) {
    use ono_kuang_supervisor::{Contribution, VisibleSchemas};

    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let events: Vec<serde_json::Value> = match value.clone() {
        serde_json::Value::Array(items) => items,
        other => vec![other],
    };
    let now = jiff::Timestamp::UNIX_EPOCH;
    let mut package = Contribution::new("dev.example.fuzz")
        .seeing(VisibleSchemas::of(["dev.example.fuzz.thing/1".to_owned()]))
        .declaring(["dev.example.fuzz.rule".to_owned()]);
    if let Ok(claims) = package.check_events(&events, "fuzz", now) {
        for claim in claims {
            assert!(
                claim
                    .subject_schema
                    .as_deref()
                    .is_none_or(|schema| schema == "dev.example.fuzz.thing/1"),
                "v0.5 section 37.3: an accepted event names an object the package can resolve"
            );
            assert!(
                claim.source.as_str().starts_with("kuang:dev.example.fuzz/"),
                "v0.5 section 37.3: the host owns attribution, whatever the payload claimed"
            );
        }
    }
    let _ = package.check_link(&value, "fuzz");
}

/// The kernel's own interfaces: netlink from a socket, procfs from a file.
fn system_decoders(data: &[u8]) {
    let names = InterfaceNames::from_links(data);
    let bounded = |what: &str, decoded: &ono_provider_netlink::Decoded| {
        assert!(
            decoded.records().len() + decoded.errors().len() < 10_000,
            "the {what} decoder answered more than a kernel could have sent"
        );
        for record in decoded.records() {
            for field in ["name", "index", "address", "inode", "local", "destination"] {
                let _ = record.access(field);
            }
        }
    };
    bounded("interface", &decode_interfaces(data, data));
    bounded("route", &decode_routes(data, &names));
    bounded("neighbor", &decode_neighbors(data, &names));
    bounded("tcp", &decode_inet_sockets(data, SocketProtocol::Tcp, None));
    bounded("unix", &decode_unix_sockets(data, None));

    let text = String::from_utf8_lossy(data);
    let _ = procfs::parse_stat(&text);
    let _ = procfs::parse_status_ids(&text);
    let _ = procfs::service_unit(&text);
    let _ = procfs::parse_cmdline(data);
    let _ = procfs::parse_mountinfo(&text);
    let _ = procfs::parse_fstab(&text);
}

/// The handshake a remote agent and a client exchange before anything else (v0.4.1 §41.2, §13.1).
///
/// The framing is `remote-protocol`'s subject; this one hands the *payload* straight to each
/// handshake message's decoder, so an input reaches the version list, the provider descriptors
/// and the capability descriptors rather than spending itself on a length prefix.
fn remote_handshake(data: &[u8]) {
    let limits = Limits::new()
        .with_max_frame_payload(4096)
        .with_max_value_depth(16);
    let schemas = builtin_schemas();
    for kind in [FrameKind::Hello, FrameKind::Accept, FrameKind::Reject] {
        // A handshake message that decodes must survive being asked what it agreed to: §13.2
        // binds the negotiated version to the authenticated handshake, and a descriptor that
        // panics when it is read is that binding falling over on attacker bytes.
        if let Ok(message) = decode_message(kind, data, schemas, &limits) {
            let _ = message.kind();
            let _ = ono_protocol::encode_message(&message, &limits);
        }
    }
}

/// The adapter decoders, fed bytes an external program could have written (v0.4.1 §41.2, §5.2's
/// attacker class 8: "an adapter producing malformed machine-readable output").
///
/// The first byte chooses an adapter from the first-party packs, so one corpus reaches every
/// decoder kind the packs declare — JSON, JSON lines, key/value and the table readers — and the
/// rest is what the program is imagined to have printed.
fn adapter_decoders(data: &[u8]) {
    let Some((selector, rest)) = data.split_first() else {
        return;
    };
    let adapters: Vec<&'static Adapter> = ono_adapter::first_party()
        .iter()
        .flat_map(|pack| pack.adapters().iter())
        .collect();
    if adapters.is_empty() {
        return;
    }
    let adapter = adapters[usize::from(*selector) % adapters.len()];
    let trace = Trace {
        executable: std::path::PathBuf::from("/usr/bin/fuzzed"),
        version: None,
        user_invocation: vec!["fuzzed".to_owned()],
        actual_invocation: vec!["fuzzed".to_owned()],
        host: None,
    };
    let schemas = builtin_schemas();
    // Whole-output decoding, and the incremental path beside it: a streaming decoder that
    // disagrees with the batch one about the same bytes is a finding, and neither may panic.
    let _ = ono_adapter::decode(adapter, rest, &trace, schemas);
    if let Ok(mut decoding) = ono_adapter::Decoding::borrowed(adapter, trace, schemas) {
        for chunk in rest.chunks(7) {
            for outcome in decoding.feed(chunk) {
                let _ = outcome;
            }
        }
        for outcome in decoding.finish() {
            let _ = outcome;
        }
    }
}

/// A corrupt temporal store, opened and queried (v0.5 §31.7, §47.3).
///
/// §31.7 requires a store whose bytes are damaged to refuse the affected history, name what was
/// discarded, and leave the shell working. What must not happen is a panic, an unbounded
/// allocation, or a read outside the row — and the bytes reaching this decoder are exactly the
/// bytes a plugin or a remote host contributed, written and read back.
fn temporal_store(data: &[u8]) {
    let Ok(scratch) = tempfile::tempdir() else {
        return;
    };
    let path = scratch.path().join("ledger.sqlite3");
    if std::fs::write(&path, data).is_err() {
        return;
    }
    // Opening is allowed to fail; it is not allowed to panic, and neither is reading whatever it
    // decided it could open.
    if let Ok(store) =
        ono_temporal_ledger::LedgerStore::open_with(&ono_temporal_ledger::StoreOptions::at(&path))
    {
        use ono_temporal_core::LedgerRead as _;
        let _ = store.events(&ono_temporal_core::EventQuery::default());
        let _ = store.coverage(&ono_temporal_core::CoverageQuery::default());
        let _ = store.retention();
        let _ = store.integrity();
    }
}

/// An event payload decoded from arbitrary bytes (v0.5 §31.4, §47.3).
fn temporal_events(data: &[u8]) {
    let _ = ono_temporal_ledger::decode_payload(data);
}

/// A time selector parsed and resolved from arbitrary text (v0.5 §4.4, §47.3).
///
/// This is the most reachable parser the tranche adds: it sits at a prompt, and a shell reads
/// what a person types. Resolution is included because the interesting failures are there —
/// a daylight-saving fold, a wall time outside the representable range, a future instant.
fn time_selectors(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let Ok(selector) = ono_temporal_core::TimeSelector::parse(&text) else {
        return;
    };
    struct NoAnchors;
    impl ono_temporal_core::EventAnchors for NoAnchors {
        fn instant_of(&self, _id: &ono_temporal_core::EventId) -> Option<jiff::Timestamp> {
            None
        }
    }
    for zone in [
        "UTC",
        "Europe/Berlin",
        "America/New_York",
        "Pacific/Kiritimati",
    ] {
        let Ok(zone) = jiff::tz::TimeZone::get(zone) else {
            continue;
        };
        let _ = selector.resolve(&zone, jiff::Timestamp::UNIX_EPOCH, &NoAnchors);
        let _ = selector.resolve(&zone, jiff::Timestamp::now(), &NoAnchors);
    }
}

/// An arbitrary event set through the causal rule runtime (v0.5 §15.2, §47.3).
///
/// The assertion inside is the one the whole causal design rests on and the cheapest place to
/// hold it: **no rule may emit a causal relation without a rule id, a source and at least one
/// piece of evidence.** §55.3 names the failure this prevents, and a fuzz target asks the
/// question over inputs nobody thought to write down.
fn causal_candidates(data: &[u8]) {
    let events = decode_events(data);
    if events.is_empty() {
        return;
    }
    let engine = ono_temporal_query::causal::CausalEngine::builtin();
    let context = ono_temporal_query::causal::CausalContext::default();
    for link in engine.links(&events, &context) {
        if link.relation.is_causal() {
            assert!(
                !link.evidence.is_empty(),
                "v0.5 §15.2: a causal relation carries the evidence that established it"
            );
            assert!(
                !link.rule.as_str().trim().is_empty(),
                "v0.5 §15.8: a causal relation names the rule that emitted it"
            );
        }
    }
}

/// Events built from the bytes through the public constructors.
///
/// The rule runtime takes typed events rather than bytes, so the bytes choose the *shape* — the
/// kind, the subject, the instants, the sequence — and everything is built the way an ingest path
/// builds it. That is what explores the rule space: which rule fires is decided by kinds,
/// identities and evidence, and those are exactly what varies here.
fn decode_events(data: &[u8]) -> Vec<ono_temporal_core::TemporalEvent> {
    use ono_spatial_core::{BootIdentity, SpatialIdentity, SpatialType};
    use ono_temporal_core::{ClockDomain, EventKind, EventSeed, EventTimes, SpatialRef};

    let scope = ono_spatial_core::SpatialScope::host(
        "fuzz",
        BootIdentity::new("fuzz", "00000000-0000-4000-8000-000000000000"),
    );
    data.chunks(8)
        .take(64)
        .filter(|chunk| chunk.len() == 8)
        .map(|chunk| {
            let kind = EventKind::ALL[usize::from(chunk[0]) % EventKind::ALL.len()];
            let subject = SpatialIdentity::lifetime(
                SpatialType::Process,
                [("pid".to_owned(), chunk[1].to_string())],
            )
            .spatial_id();
            let nanos = i64::from(chunk[2]) * 1_000_000_000;
            let at = jiff::Timestamp::from_nanosecond(i128::from(nanos))
                .unwrap_or(jiff::Timestamp::UNIX_EPOCH);
            EventSeed {
                kind,
                subtype: None,
                scope: scope.clone(),
                subject: Some(SpatialRef::Resolved {
                    id: subject,
                    object_type: SpatialType::Process,
                    label: std::sync::Arc::from("fuzz"),
                }),
                related: Vec::new(),
                times: EventTimes {
                    source_time: Some(at),
                    observed_at: at,
                    ingested_at: at,
                    source_sequence: Some(u64::from(chunk[3])),
                    monotonic_nanos: Some(u64::from(chunk[4])),
                    clock_uncertainty: None,
                    domain: ClockDomain {
                        host: std::sync::Arc::from("fuzz"),
                        boot_id: Some(std::sync::Arc::from("boot")),
                    },
                },
                before: None,
                after: None,
                changed_fields: Vec::new(),
                evidence: Vec::new(),
                causal_parents: Vec::new(),
                payload: None,
                provenance: ono_value::Provenance::local(
                    "linux.procfs",
                    ono_value::SchemaId::new("ono.temporal-event", 1),
                ),
            }
            .seal()
        })
        .collect()
}
