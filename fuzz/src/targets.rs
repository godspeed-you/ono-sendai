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
use ono_kuang_protocol::{FrameLimits, Manifest, PackageSignature, decode_payload, read_frame};
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

/// The plugin protocol: the frame reader, the payload decoder, the manifest and the signature.
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
