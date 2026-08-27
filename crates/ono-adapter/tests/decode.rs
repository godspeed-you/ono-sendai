//! Decoding (spec v0.3 §1.8, §1.10, §1.11, §1.20, ADR-0057): tool output becomes canonical
//! records with adapter provenance; anything else becomes a structured error, never a panic.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions the way a #[test] body does (AGENTS.md section 16)"
)]

use std::path::{Path, PathBuf};

use ono_adapter::{Adapter, Trace, Version};
use ono_core::ErrorCode;
use ono_value::{Value, builtin_schemas};

fn fixture(adapter: &str, name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/spec/adapters/fixtures/util-linux")
            .join(adapter)
            .join(format!("{name}.out")),
    )
    .expect("the fixture exists")
}

fn adapter(id: &str) -> &'static Adapter {
    ono_adapter::first_party()
        .iter()
        .find(|pack| pack.id() == "org.ono.compat.util-linux")
        .unwrap()
        .adapters()
        .iter()
        .find(|adapter| adapter.id() == id)
        .unwrap()
}

fn trace(program: &str) -> Trace {
    Trace {
        executable: Path::new("/usr/bin").join(program),
        version: Version::parse("2.41.3"),
        user_invocation: vec![program.to_owned()],
        actual_invocation: vec![program.to_owned(), "--json".to_owned()],
        host: None,
    }
}

fn records(values: Vec<Value>) -> Vec<std::sync::Arc<ono_value::RecordValue>> {
    values
        .into_iter()
        .map(|value| match value {
            Value::Record(record) => record,
            other => panic!("every decoded value is a record, got {other:?}"),
        })
        .collect()
}

#[test]
fn should_decode_lsblk_into_typed_block_device_records() {
    let values = ono_adapter::decode(
        adapter("lsblk"),
        &fixture("lsblk", "disk-with-partitions"),
        &trace("lsblk"),
        builtin_schemas(),
    )
    .expect("the fixture decodes");
    let rows = records(values);
    assert_eq!(rows.len(), 3);
    let sda1 = &rows[1];
    assert_eq!(sda1.schema_id().to_string(), "ono.block-device/1");
    assert_eq!(sda1.get("name"), Some(&Value::string("sda1")));
    assert!(
        matches!(sda1.get("size"), Some(Value::ByteSize(size)) if size.bytes() == 1_127_219_200),
        "a bare number in bytes becomes a byte size (schema-driven coercion), got {:?}",
        sda1.get("size")
    );
    assert!(
        matches!(sda1.get("mountpoints"), Some(Value::List(items)) if matches!(items.first(), Some(Value::Path(_)))),
        "list<path> is coerced element-wise, got {:?}",
        sda1.get("mountpoints")
    );
    assert_eq!(sda1.get("read_only"), Some(&Value::Bool(false)));
    assert_eq!(sda1.get("parent"), Some(&Value::string("sda")));
    assert_eq!(
        rows[0].get("filesystem"),
        Some(&Value::Null),
        "null stays null (spec §10.5)"
    );
}

#[test]
fn should_attach_adapter_provenance_to_every_record() {
    let values = ono_adapter::decode(
        adapter("lsblk"),
        &fixture("lsblk", "disk-with-partitions"),
        &trace("lsblk"),
        builtin_schemas(),
    )
    .unwrap();
    let provenance = records(values)[0].provenance().clone();
    assert_eq!(
        provenance.provider(),
        "adapter:org.ono.compat.util-linux.lsblk"
    );
    let adapter = provenance
        .adapter()
        .expect("spec v0.3 §1.8: every adapted value exposes adapter provenance");
    assert_eq!(adapter.adapter_version(), "0.1.0");
    assert_eq!(adapter.executable(), Path::new("/usr/bin/lsblk"));
    assert_eq!(adapter.executable_version(), Some("2.41.3"));
    assert_eq!(adapter.user_invocation(), "lsblk");
    assert_eq!(adapter.actual_invocation(), "lsblk --json");
    assert_eq!(adapter.decoder(), "json");
    assert_eq!(adapter.stability(), "stable");
    assert_eq!(
        adapter.exactness().get("size"),
        Some(&"normalized".to_owned()),
        "a unit conversion is normalized, everything else exact"
    );
    let rendered = provenance.render();
    assert!(
        rendered.contains("actual_invocation") && rendered.contains("lsblk --json"),
        "{rendered}"
    );
}

#[test]
fn should_keep_fields_the_map_does_not_name_as_extensions() {
    let values = ono_adapter::decode(
        adapter("lsblk"),
        &fixture("lsblk", "newer-fields"),
        &trace("lsblk"),
        builtin_schemas(),
    )
    .unwrap();
    let row = &records(values)[0];
    let extensions = row.extra();
    let Some(Value::Map(tool)) = extensions.get("org.ono.compat.util-linux.lsblk") else {
        panic!(
            "spec v0.3 §1.11: tool-specific fields live under the adapter's namespace, got {extensions:?}"
        );
    };
    assert_eq!(tool.get("zoned"), Some(&Value::string("none")));
}

#[test]
fn should_split_options_and_derive_read_only_for_findmnt() {
    let values = ono_adapter::decode(
        adapter("findmnt"),
        &fixture("findmnt", "root-and-run"),
        &trace("findmnt"),
        builtin_schemas(),
    )
    .unwrap();
    let rows = records(values);
    assert!(
        matches!(rows[1].get("options"), Some(Value::List(items)) if items.len() == 2),
        "`rw,relatime` splits into two options (spec §23.5), got {:?}",
        rows[1].get("options")
    );
    assert_eq!(rows[2].get("read_only"), Some(&Value::Bool(true)));
    assert_eq!(rows[0].get("read_only"), Some(&Value::Bool(false)));
}

#[test]
fn should_fail_structurally_on_truncated_output_and_keep_the_bytes() {
    let error = ono_adapter::decode(
        adapter("lsblk"),
        &fixture("lsblk", "truncated"),
        &trace("lsblk"),
        builtin_schemas(),
    )
    .expect_err("truncated JSON cannot become records");
    assert_eq!(error.code(), ErrorCode::AdapterDecodeFailed);
    let metadata = error.metadata();
    assert_eq!(
        metadata.get("adapter"),
        Some(&Value::string("org.ono.compat.util-linux.lsblk")),
        "spec v0.3 §1.65: the error carries the adapter, got {metadata:?}"
    );
    assert_eq!(
        metadata.get("executable"),
        Some(&Value::string("/usr/bin/lsblk"))
    );
    assert_eq!(metadata.get("invocation"), Some(&Value::string("lsblk")));
    assert_eq!(metadata.get("raw_fallback_safe"), Some(&Value::Bool(true)));
    assert!(
        matches!(metadata.get("raw"), Some(Value::Bytes(bytes)) if !bytes.is_empty()),
        "the raw bytes are retained, got {metadata:?}"
    );
}

#[test]
fn should_report_a_schema_violation_when_a_field_has_the_wrong_type() {
    let error = ono_adapter::decode(
        adapter("lsns"),
        &fixture("lsns", "wrong-type"),
        &trace("lsns"),
        builtin_schemas(),
    )
    .expect_err("spec v0.3 §1.10: no silent field shifting");
    assert_eq!(error.code(), ErrorCode::AdapterSchemaViolation);
    assert!(
        error.message().contains("`id`"),
        "the field is named, got {}",
        error.message()
    );
}

#[test]
fn should_never_panic_on_hostile_bytes() {
    // A seeded walk over garbage, nesting bombs and non-UTF-8: every outcome is a value or a
    // structured error.
    let mut rng = ono_testkit::Rng::seeded(0x5eed);
    let deep = format!(
        "{{\"blockdevices\": {}1{}}}",
        "[".repeat(5000),
        "]".repeat(5000)
    );
    let mut inputs: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"\xff\xfe\x00garbage".to_vec(),
        b"{\"blockdevices\": \"not a list\"}".to_vec(),
        b"{\"blockdevices\": [42, null, \"x\"]}".to_vec(),
        b"[]".to_vec(),
        deep.into_bytes(),
    ];
    for _ in 0..200 {
        let length = rng.below(64);
        inputs.push((0..length).map(|_| rng.below(256) as u8).collect());
    }
    for input in inputs {
        let _ = ono_adapter::decode(adapter("lsblk"), &input, &trace("lsblk"), builtin_schemas());
    }
}

fn ip_adapter(id: &str) -> &'static Adapter {
    ono_adapter::first_party()
        .iter()
        .find(|pack| pack.id() == "org.ono.compat.iproute2")
        .unwrap()
        .adapters()
        .iter()
        .find(|adapter| adapter.id() == id)
        .unwrap()
}

#[test]
fn should_derive_records_from_children_with_templates_literals_and_inference() {
    let bytes = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/spec/adapters/fixtures/iproute2/ip-address/two-interfaces.out"),
    )
    .unwrap();
    let addresses = records(
        ono_adapter::decode(
            ip_adapter("ip-address"),
            &bytes,
            &trace("ip"),
            builtin_schemas(),
        )
        .unwrap(),
    );
    assert_eq!(
        addresses.len(),
        3,
        "children are the records, an interface without addresses adds none"
    );
    assert!(
        matches!(addresses[1].get("address"), Some(Value::IpNetwork(network)) if network.to_string() == "192.168.0.167/24"),
        "a template over two decoded fields becomes one typed ipnetwork, got {:?}",
        addresses[1].get("address")
    );
    assert_eq!(addresses[1].get("interface"), Some(&Value::string("eth0")));
    assert_eq!(
        addresses[0]
            .provenance()
            .adapter()
            .unwrap()
            .exactness()
            .get("address"),
        Some(&"normalized".to_owned()),
        "a templated field is normalized"
    );

    let neigh = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/spec/adapters/fixtures/iproute2/ip-neigh/mixed.out"),
    )
    .unwrap();
    let neighbours = records(
        ono_adapter::decode(
            ip_adapter("ip-neigh"),
            &neigh,
            &trace("ip"),
            builtin_schemas(),
        )
        .unwrap(),
    );
    assert_eq!(neighbours[2].get("family"), Some(&Value::string("inet6")));
    assert_eq!(
        neighbours[2]
            .provenance()
            .adapter()
            .unwrap()
            .exactness()
            .get("family"),
        Some(&"inferred".to_owned()),
        "spec v0.3 §1.8: an inferred field says so"
    );
    assert_eq!(
        neighbours[0].get("state"),
        Some(&Value::string("reachable")),
        "`first` then `map`"
    );

    let route =
        br#"[{"dst":"default","gateway":"10.0.0.1","dev":"eth0","protocol":"static","flags":[]}]"#;
    let trace6 = Trace {
        user_invocation: vec!["ip".into(), "-6".into(), "route".into()],
        actual_invocation: vec![
            "ip".into(),
            "-j".into(),
            "-6".into(),
            "route".into(),
            "show".into(),
        ],
        ..trace("ip")
    };
    let routes = records(
        ono_adapter::decode(ip_adapter("ip-route6"), route, &trace6, builtin_schemas()).unwrap(),
    );
    assert_eq!(
        routes[0].get("family"),
        Some(&Value::string("inet6")),
        "a literal from the invocation"
    );
    assert!(
        matches!(routes[0].get("destination"), Some(Value::IpNetwork(network)) if network.to_string() == "::/0"),
        "`default` maps to the family's whole space, got {:?}",
        routes[0].get("destination")
    );
}

fn ps_adapter() -> &'static Adapter {
    ono_adapter::first_party()
        .iter()
        .find(|pack| pack.id() == "org.ono.compat.procps")
        .expect("spec v0.3 §1.69 step 6: ps is bundled")
        .adapters()
        .iter()
        .find(|adapter| adapter.id() == "ps")
        .unwrap()
}

#[test]
fn should_split_whitespace_columns_with_a_greedy_last_field_and_derive_process_fields() {
    let bytes = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/spec/adapters/fixtures/procps/ps/three-processes.out"),
    )
    .unwrap();
    let rows = records(
        ono_adapter::decode(ps_adapter(), &bytes, &trace("ps"), builtin_schemas()).unwrap(),
    );
    assert_eq!(rows.len(), 3);
    let systemd = &rows[0];
    assert_eq!(
        systemd.get("name"),
        Some(&Value::string("systemd")),
        "program name from args"
    );
    assert!(
        matches!(systemd.get("command"), Some(Value::List(words)) if words.len() == 5),
        "args keep their spaces through the greedy last column, got {:?}",
        systemd.get("command")
    );
    assert_eq!(
        systemd.get("state"),
        Some(&Value::string("sleeping")),
        "first letter of `Ss`"
    );
    assert!(
        matches!(systemd.get("memory"), Some(Value::ByteSize(size)) if size.bytes() == 17_264 * 1024),
        "rss is KiB, got {:?}",
        systemd.get("memory")
    );
    let started = systemd.get("started").cloned().unwrap_or(Value::Null);
    assert!(
        matches!(started, Value::Timestamp(_)),
        "started is inferred from elapsed seconds, got {started:?}"
    );
    let exactness = systemd.provenance().adapter().unwrap().exactness().clone();
    assert_eq!(
        exactness.get("started").map(String::as_str),
        Some("inferred")
    );
    assert_eq!(exactness.get("name").map(String::as_str), Some("inferred"));
    assert_eq!(
        rows[1].get("name"),
        Some(&Value::string("kthreadd")),
        "brackets stripped for a kernel thread"
    );
}

#[test]
fn should_stream_a_lines_decoder_record_by_record() {
    let bytes = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/spec/adapters/fixtures/procps/ps/three-processes.out"),
    )
    .unwrap();
    let mut decoding =
        ono_adapter::Decoding::borrowed(ps_adapter(), trace("ps"), builtin_schemas()).unwrap();
    assert!(
        decoding.streams(),
        "a newline-separated protocol streams (ADR-0060)"
    );
    let first_line = bytes.iter().position(|b| *b == b'\n').unwrap() + 1;
    let early = decoding.feed(&bytes[..first_line]);
    assert_eq!(
        early.len(),
        1,
        "one complete line is one record before the rest arrives"
    );
    let rest = decoding.feed(&bytes[first_line..]);
    assert_eq!(rest.len(), 2);
    assert!(decoding.finish().is_empty());
}

#[test]
fn should_decode_ss_endpoints_into_nested_endpoint_records() {
    let adapter = ono_adapter::first_party()
        .iter()
        .find(|pack| pack.id() == "org.ono.compat.iproute2")
        .unwrap()
        .adapters()
        .iter()
        .find(|adapter| adapter.id() == "ss-tcp")
        .unwrap();
    let bytes = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/spec/adapters/fixtures/iproute2/ss-tcp/listening.out"),
    )
    .unwrap();
    let rows =
        records(ono_adapter::decode(adapter, &bytes, &trace("ss"), builtin_schemas()).unwrap());
    let Some(Value::Record(local)) = rows[0].get("local") else {
        panic!(
            "`local` is an ono.endpoint/1 record, got {:?}",
            rows[0].get("local")
        );
    };
    assert_eq!(local.schema_id().to_string(), "ono.endpoint/1");
    assert!(matches!(local.get("address"), Some(Value::Ip(ip)) if ip.to_string() == "127.0.0.1"));
    assert_eq!(local.get("port"), Some(&Value::Port(631)));
    let Some(Value::Record(peer)) = rows[0].get("remote") else {
        panic!("a wildcard peer is still an endpoint record");
    };
    assert_eq!(
        peer.get("port"),
        Some(&Value::Null),
        "`*` is an unknown port, not zero"
    );
    assert_eq!(
        rows[0].provenance().adapter().unwrap().stability(),
        "version-constrained",
        "spec v0.3 §1.8: a human-output parser says so"
    );
}
