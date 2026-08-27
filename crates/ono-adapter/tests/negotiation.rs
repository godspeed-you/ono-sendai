//! Adapter negotiation (spec v0.3 §1.6, §1.14–§1.16, §1.24, §1.25, §1.46): given a resolved
//! executable, the user's argv and the output demand, the registry answers one explicit state,
//! deterministically, and the plan it returns is the exact argv the tool will be run with.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions the way a #[test] body does (AGENTS.md section 16)"
)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ono_adapter::{AdapterPack, Negotiation, OutputDemand, Registry, Version};
use ono_testkit::{Scratch, scratch};

const UTIL_LINUX: &str = include_str!("../../../docs/spec/adapters/first-party/util-linux.yaml");

/// A registry over the bundled packs whose version probe answers with `version` and counts.
fn registry(version: &'static str) -> (Registry, Arc<AtomicUsize>) {
    let probes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&probes);
    let registry = Registry::bundled(Box::new(move |_path, _argv| {
        counter.fetch_add(1, Ordering::SeqCst);
        (!version.is_empty()).then(|| format!("lsblk from util-linux {version}\n"))
    }));
    (registry, probes)
}

/// A fake executable: an empty file, so it has an identity (device, inode, mtime, size).
fn executable(dir: &Scratch, name: &str) -> PathBuf {
    dir.write(name, "");
    dir.path().join(name)
}

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

fn structured() -> OutputDemand {
    OutputDemand::Structured { schema: None }
}

#[test]
fn should_answer_structured_supported_with_the_exact_machine_argv() {
    let dir = scratch();
    let lsblk = executable(&dir, "lsblk");
    let (registry, _) = registry("2.41.3");
    let Negotiation::StructuredSupported { plan, .. } =
        registry.negotiate(&lsblk, &argv(&["lsblk"]), &structured())
    else {
        panic!("lsblk with a structured consumer is adapted (spec v0.3 §1.35)");
    };
    assert_eq!(plan.adapter().full_id(), "org.ono.compat.util-linux.lsblk");
    assert_eq!(
        plan.executable(),
        lsblk.as_path(),
        "the resolved binary is pinned (§1.22)"
    );
    assert_eq!(
        plan.argv(),
        argv(&[
            "lsblk",
            "--json",
            "--list",
            "--bytes",
            "--output",
            "NAME,PATH,TYPE,SIZE,FSTYPE,MOUNTPOINTS,MODEL,SERIAL,RO,RM,PKNAME,MAJ:MIN"
        ]),
        "spec v0.3 §1.47: generated argv is tested exactly"
    );
    assert_eq!(plan.env().get("LC_ALL").map(String::as_str), Some("C"));
    assert_eq!(plan.user_invocation(), argv(&["lsblk"]));
    assert_eq!(plan.version(), Some(&Version::parse("2.41.3").unwrap()));
}

#[test]
fn should_append_allowed_user_flags_and_positionals_to_the_plan() {
    let dir = scratch();
    let lsblk = executable(&dir, "lsblk");
    let (registry, _) = registry("2.41.3");
    let negotiated = registry.negotiate(&lsblk, &argv(&["lsblk", "-a", "/dev/sda"]), &structured());
    let plan = negotiated.plan().expect("adapted");
    assert_eq!(
        &plan.argv()[plan.argv().len() - 2..],
        argv(&["-a", "/dev/sda"]),
        "spec v0.3 §1.14: the user's selectors are preserved"
    );
}

#[test]
fn should_refuse_an_invocation_with_a_flag_the_contract_does_not_declare() {
    let dir = scratch();
    let lsblk = executable(&dir, "lsblk");
    let (registry, _) = registry("2.41.3");
    match registry.negotiate(&lsblk, &argv(&["lsblk", "-p"]), &structured()) {
        Negotiation::UnsupportedInvocation {
            adapter, reason, ..
        } => {
            assert_eq!(adapter, "org.ono.compat.util-linux.lsblk");
            assert!(
                reason.contains("-p"),
                "the offending flag is named, got {reason}"
            );
        }
        other => panic!("spec v0.3 §1.14: an unknown flag is never approximated, got {other:?}"),
    }
}

#[test]
fn should_accept_a_flag_that_takes_a_value_and_refuse_forbidden_positionals() {
    let dir = scratch();
    let lsns = executable(&dir, "lsns");
    let (registry, _) = registry("2.41.3");
    let plan = registry
        .negotiate(&lsns, &argv(&["lsns", "-t", "net"]), &structured())
        .plan()
        .cloned()
        .expect("`lsns -t net` is adapted");
    assert_eq!(&plan.argv()[plan.argv().len() - 2..], argv(&["-t", "net"]));
    assert!(matches!(
        registry.negotiate(&lsns, &argv(&["lsns", "4026531832"]), &structured()),
        Negotiation::UnsupportedInvocation { .. }
    ));
}

#[test]
fn should_prefer_raw_when_the_consumer_wants_bytes() {
    let dir = scratch();
    let lsblk = executable(&dir, "lsblk");
    let (registry, probes) = registry("2.41.3");
    match registry.negotiate(&lsblk, &argv(&["lsblk"]), &OutputDemand::RawBytes) {
        Negotiation::RawPreferred { reason } => {
            assert!(reason.contains("bytes"), "got {reason}");
        }
        other => panic!("spec v0.3 §1.4: bytes downstream keep the tool raw, got {other:?}"),
    }
    assert_eq!(
        probes.load(Ordering::SeqCst),
        0,
        "nothing is probed for a raw run"
    );
}

#[test]
fn should_not_apply_to_a_program_no_adapter_knows() {
    let dir = scratch();
    let grep = executable(&dir, "grep");
    let (registry, _) = registry("2.41.3");
    assert_eq!(
        registry.negotiate(&grep, &argv(&["grep", "x"]), &structured()),
        Negotiation::NotApplicable,
        "spec v0.3 §1.70: text tools stay raw"
    );
}

#[test]
fn should_refuse_an_executable_outside_the_supported_versions() {
    let dir = scratch();
    let lsblk = executable(&dir, "lsblk");
    let (registry, _) = registry("2.30");
    match registry.negotiate(&lsblk, &argv(&["lsblk"]), &structured()) {
        Negotiation::IncompatibleVersion {
            found, supported, ..
        } => {
            assert_eq!(found, Some(Version::parse("2.30").unwrap()));
            assert_eq!(supported, ">=2.37");
        }
        other => panic!("spec v0.3 §1.6: an untested version is refused, got {other:?}"),
    }
}

#[test]
fn should_refuse_when_the_version_cannot_be_detected() {
    let dir = scratch();
    let lsblk = executable(&dir, "lsblk");
    let (registry, _) = registry("");
    assert!(
        matches!(
            registry.negotiate(&lsblk, &argv(&["lsblk"]), &structured()),
            Negotiation::IncompatibleVersion { found: None, .. }
        ),
        "spec v0.3 §1.46: a failed probe refuses rather than assumes"
    );
}

#[test]
fn should_probe_an_executable_once_per_identity() {
    let dir = scratch();
    let lsblk = executable(&dir, "lsblk");
    let (registry, probes) = registry("2.41.3");
    for _ in 0..3 {
        let _ = registry.negotiate(&lsblk, &argv(&["lsblk"]), &structured());
    }
    assert_eq!(
        probes.load(Ordering::SeqCst),
        1,
        "spec v0.3 §1.46: probes are cached by executable identity"
    );
    // A rewritten binary is a different identity.
    std::fs::write(&lsblk, "changed").unwrap();
    let _ = registry.negotiate(&lsblk, &argv(&["lsblk"]), &structured());
    assert_eq!(probes.load(Ordering::SeqCst), 2);
}

#[test]
fn should_resolve_two_adapters_claiming_one_invocation_the_same_way_every_time() {
    let dir = scratch();
    let lsblk = executable(&dir, "lsblk");
    let rival = UTIL_LINUX
        .replacen("id: org.ono.compat.util-linux", "id: com.example.blocks", 1)
        .replacen("publisher: org.ono", "publisher: com.example", 1)
        .replacen("tier: first-party", "tier: community", 1);
    let rival = AdapterPack::parse(&rival).unwrap();

    let first = registry("2.41.3").0.with_pack(rival.clone());
    let reversed = Registry::new(
        vec![rival],
        Box::new(|_, _| Some("util-linux 2.41.3".into())),
    )
    .with_packs(ono_adapter::first_party().to_vec());

    for registry in [&first, &reversed] {
        match registry.negotiate(&lsblk, &argv(&["lsblk"]), &structured()) {
            Negotiation::StructuredSupported {
                plan,
                candidates,
                selection,
            } => {
                assert_eq!(
                    plan.adapter().full_id(),
                    "org.ono.compat.util-linux.lsblk",
                    "spec v0.3 §1.25: trust tier decides before lexical order, whatever the load order"
                );
                assert_eq!(
                    candidates,
                    vec![
                        "com.example.blocks.lsblk".to_owned(),
                        "org.ono.compat.util-linux.lsblk".to_owned()
                    ],
                    "explain can show every candidate"
                );
                assert!(
                    selection.contains("first-party"),
                    "the reason names the rule, got {selection}"
                );
            }
            other => panic!("got {other:?}"),
        }
    }
}

#[test]
fn should_report_a_conflict_when_one_adapter_is_installed_twice() {
    let dir = scratch();
    let lsblk = executable(&dir, "lsblk");
    // Adding a pack replaces an earlier one of the same id (ADR-0065), so the only way to hold
    // two copies is to construct the registry with both — which is what a duplicated install
    // directory amounts to.
    let twice = Registry::new(
        vec![
            AdapterPack::parse(UTIL_LINUX).unwrap(),
            AdapterPack::parse(UTIL_LINUX).unwrap(),
        ],
        Box::new(|_, _| Some("util-linux 2.41.3".into())),
    );
    match twice.negotiate(&lsblk, &argv(&["lsblk"]), &structured()) {
        Negotiation::Conflict { candidates } => {
            assert_eq!(candidates.len(), 2);
        }
        other => panic!("two copies of one id cannot be separated, got {other:?}"),
    }
}

#[test]
fn should_refuse_a_binary_that_is_not_the_one_the_contract_pins() {
    let dir = scratch();
    let shadow = executable(&dir, "lsblk");
    let pinned = UTIL_LINUX.replacen("      names: [lsblk]", "      names: [/usr/bin/lsblk]", 1);
    let registry = Registry::new(
        vec![AdapterPack::parse(&pinned).unwrap()],
        Box::new(|_, _| Some("util-linux 2.41.3".into())),
    );
    match registry.negotiate(&shadow, &argv(&["lsblk"]), &structured()) {
        Negotiation::ExecutableMismatch {
            expected, found, ..
        } => {
            assert_eq!(expected, "/usr/bin/lsblk");
            assert_eq!(found, shadow);
        }
        other => panic!("spec v0.3 §1.22: a shadowing binary is not adapted, got {other:?}"),
    }
}

#[test]
fn should_note_limits_when_the_map_leaves_a_schema_field_unreported() {
    let dir = scratch();
    let findmnt = executable(&dir, "findmnt");
    let (registry, _) = registry("2.41.3");
    match registry.negotiate(&findmnt, &argv(&["findmnt"]), &structured()) {
        Negotiation::StructuredSupportedWithLimits { limits, .. } => {
            assert!(
                limits.iter().any(|l| l.contains("device")),
                "spec v0.3 §1.6: limits are visible, got {limits:?}"
            );
        }
        other => panic!("findmnt cannot report `device`, so its support has limits, got {other:?}"),
    }
}

#[test]
fn should_describe_every_state_in_the_words_of_the_diagnostics_section() {
    let dir = scratch();
    let lsblk = executable(&dir, "lsblk");
    let (registry, _) = registry("2.41.3");
    let adapted = registry.negotiate(&lsblk, &argv(&["lsblk"]), &structured());
    assert!(
        adapted.describe(&structured()).starts_with("adapted"),
        "got {}",
        adapted.describe(&structured())
    );
    let raw = registry.negotiate(&lsblk, &argv(&["lsblk"]), &OutputDemand::RawBytes);
    assert_eq!(
        raw.describe(&OutputDemand::RawBytes),
        "raw (downstream bytes)"
    );
    let unsupported = registry.negotiate(&lsblk, &argv(&["lsblk", "-p"]), &structured());
    assert!(
        unsupported
            .describe(&structured())
            .starts_with("unsupported invocation"),
        "spec v0.3 §1.18: under a structured demand nothing downgrades silently, got {}",
        unsupported.describe(&structured())
    );
    assert!(
        unsupported
            .describe(&OutputDemand::Interactive)
            .starts_with("raw (unsupported invocation"),
        "at the terminal the adapter's fallback is raw, got {}",
        unsupported.describe(&OutputDemand::Interactive)
    );
}

#[test]
fn should_require_a_flag_when_the_contract_says_so_and_pin_the_family() {
    let dir = scratch();
    let ip = executable(&dir, "ip");
    let registry = Registry::bundled(Box::new(|_, _| Some("ip utility, iproute2-6.19.0".into())));
    let v6 = registry.negotiate(&ip, &argv(&["ip", "-6", "route"]), &structured());
    let plan = v6.plan().expect("`ip -6 route` is adapted");
    assert_eq!(
        plan.adapter().id(),
        "ip-route6",
        "the -6 form selects the IPv6 adapter"
    );
    assert_eq!(plan.argv(), argv(&["ip", "-j", "-6", "route", "show"]));
    let v4 = registry.negotiate(&ip, &argv(&["ip", "route"]), &structured());
    assert_eq!(
        v4.plan().expect("`ip route` is adapted").adapter().id(),
        "ip-route"
    );
    let words = registry.negotiate(&ip, &argv(&["ip", "a", "show", "dev", "lo"]), &structured());
    assert_eq!(
        words.plan().expect("`ip a show dev lo` is adapted").argv(),
        argv(&["ip", "-j", "address", "show", "show", "dev", "lo"]),
        "words after the alias pass through in order"
    );
}

#[test]
fn should_append_trailing_argv_after_the_users_own_words() {
    // find's action must come after the user's paths and tests (spec v0.3 §1.38).
    let dir = scratch();
    let find = executable(&dir, "find");
    let registry = Registry::bundled(Box::new(|_, _| Some("find (GNU findutils) 4.9.0".into())));
    let plan = registry
        .negotiate(
            &find,
            &argv(&["find", "/etc", "-type", "f", "-name", "*.conf"]),
            &structured(),
        )
        .plan()
        .cloned()
        .expect("`find /etc -type f -name *.conf` is adapted");
    assert_eq!(
        plan.argv(),
        argv(&[
            "find",
            "/etc",
            "-type",
            "f",
            "-name",
            "*.conf",
            "-printf",
            r"%y\t%s\t%m\t%u\t%g\t%T@\t%A@\t%i\t%D\t%p\0"
        ])
    );
    assert!(
        matches!(
            registry.negotiate(&find, &argv(&["find", "/etc", "-delete"]), &structured()),
            Negotiation::UnsupportedInvocation { .. }
        ),
        "an action is a different command"
    );
}

#[test]
fn should_decompose_combined_short_flags_where_the_contract_allows_it() {
    // Spec v0.3 §1.32's own spelling is `ss -tunap`.
    let dir = scratch();
    let ss = executable(&dir, "ss");
    let registry = Registry::bundled(Box::new(|_, _| Some("ss utility, iproute2-6.19.0".into())));
    let plan = registry
        .negotiate(&ss, &argv(&["ss", "-tunap"]), &structured())
        .plan()
        .cloned()
        .expect("`ss -tunap` is adapted");
    assert_eq!(
        plan.adapter().id(),
        "ss",
        "-t and -u together select the mixed adapter"
    );
    assert_eq!(
        plan.argv(),
        argv(&["ss", "-H", "-O", "-n", "-e", "-t", "-u", "-n", "-a", "-p"])
    );
    let tcp = registry.negotiate(&ss, &argv(&["ss", "-tlnp"]), &structured());
    assert_eq!(
        tcp.plan().expect("`ss -tlnp` is adapted").adapter().id(),
        "ss-tcp"
    );
    assert!(
        matches!(
            registry.negotiate(&ss, &argv(&["ss", "-txn"]), &structured()),
            Negotiation::UnsupportedInvocation { reason, .. } if reason.contains("-x")
        ),
        "an undeclared letter inside a combined flag is named"
    );
    assert!(
        matches!(
            Registry::bundled(Box::new(|_, _| Some("ss utility, iproute2-7.1.0".into())))
                .negotiate(&ss, &argv(&["ss", "-t"]), &structured()),
            Negotiation::IncompatibleVersion { .. }
        ),
        "spec v0.3 §1.9 tier C: a version outside the pinned range is refused"
    );
}

#[test]
fn should_hold_a_disabled_pack_back_from_structured_output() {
    // A pack whose process.exec grant was denied is registered disabled (spec v0.3 §1.22,
    // ADR-0065): its adapters answer `adapter.disabled` under a structured demand and let the
    // program run raw otherwise.
    let dir = scratch();
    let lsblk = executable(&dir, "lsblk");
    let pack = AdapterPack::parse(UTIL_LINUX).unwrap();
    let registry = Registry::new(
        Vec::new(),
        Box::new(|_, _| Some("util-linux 2.41.3".into())),
    )
    .with_disabled_pack(pack, "process.exec was not granted");
    let negotiation = registry.negotiate(&lsblk, &argv(&["lsblk"]), &structured());
    assert!(
        matches!(&negotiation, Negotiation::Disabled { adapter, reason }
        if adapter == "org.ono.compat.util-linux.lsblk" && reason.contains("process.exec")),
        "got {negotiation:?}"
    );
    let error = negotiation
        .refusal(&structured(), &lsblk, &argv(&["lsblk"]))
        .expect("a structured demand cannot be met");
    assert_eq!(error.code().name(), "adapter.disabled");
    assert!(
        negotiation.runs_raw(&OutputDemand::Interactive),
        "at the terminal the program is itself"
    );
    assert!(
        negotiation
            .describe(&OutputDemand::Interactive)
            .starts_with("raw (adapter disabled")
    );
}
