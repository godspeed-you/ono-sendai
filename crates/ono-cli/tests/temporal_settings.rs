//! The twelve configuration settings of v0.5 §33, as the shell reports them.
//!
//! §33 asks for three things at once: the settings must be **typed**, they must be
//! **inspectable**, and they must **default as specified** — with
//! `temporal.recording.enabled = false` at the head of the list, because §10.2 and §2's
//! sixteenth invariant make opt-in recording a contract rather than a preference.
//!
//! Every figure this suite compares against comes out of `docs/contracts/temporal/temporal.yaml`,
//! never out of a literal typed here. §36.4 makes that registry the one home of the defaults, and
//! a test that repeated them would become a second home the moment the two disagreed. The
//! comparison itself is made by the shell — `where default_value == 24h` is the shell's own
//! equality over its own typed value — so a duration written `24h` in the registry and held as
//! nanoseconds in the catalogue are compared as the one figure they are.
//!
//! The proofs are outcomes at the contract boundary: what `get config` printed, which error a
//! wrongly typed assignment raised, what the setting held afterwards (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ono_testkit::{Scratch, scratch};
use serde_yaml_ng::Value as Yaml;

/// How many settings §33 defines. The list is closed: it is quoted in full in the specification,
/// so a thirteenth key is as much a drift as a missing one.
const DECLARED_SETTINGS: usize = 12;

/// A shell that reads and writes no configuration but the one inside `home`, and that is told
/// nothing about temporal recording.
///
/// [`support::isolated`] already draws the boundary every suite in this family draws — a
/// configuration home, a state home and an `ONO_CONFIG_DIR` inside a scratch directory — so this
/// suite draws no second one. What it adds is the one thing a suite about *defaults* needs and
/// the shared helper has no reason to care about: the environment spelling of
/// `temporal.recording.enabled` is removed, because an override in the developer's environment
/// would make §33's default a statement about that machine rather than about Ono.
fn configured_shell(home: &Scratch, script: &str) -> ono_testkit::Run {
    support::isolated(home)
        .env("NO_COLOR", "1")
        .env_remove("ONO_TEMPORAL_RECORDING_ENABLED")
        .args(["-c", script])
        .run()
}

/// The `temporal.*` block of `docs/contracts/temporal/temporal.yaml`: key, declared type, and the
/// default written the way a user would type it.
fn declared() -> Vec<(String, String, String)> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts/temporal/temporal.yaml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()));
    let registry: Yaml =
        serde_yaml_ng::from_str(&text).expect("the temporal registry is valid YAML");
    registry["settings"]
        .as_sequence()
        .expect("§36.1: the temporal registry declares its settings")
        .iter()
        .map(|setting| {
            let key = setting["key"]
                .as_str()
                .unwrap_or_else(|| panic!("every declared setting has a key, got {setting:?}"))
                .to_owned();
            let ty = setting["type"]
                .as_str()
                .unwrap_or_else(|| panic!("§33: `{key}` must declare a type"))
                .to_owned();
            let default = serde_yaml_ng::to_string(&setting["default"])
                .unwrap_or_else(|error| panic!("§33: `{key}` must declare a default: {error}"))
                .trim()
                .trim_matches('\'')
                .trim_matches('"')
                .to_owned();
            (key, ty, default)
        })
        .collect()
}

/// Every `temporal.*` row of `get config`, by key.
fn inspected(home: &Scratch) -> BTreeMap<String, Yaml> {
    let run = configured_shell(home, "get config | to json");
    run.assert_success();
    support::rows(&run)
        .into_iter()
        .filter(|row| {
            row["key"]
                .as_str()
                .is_some_and(|key| key.starts_with("temporal."))
        })
        .map(|row| (support::text(&row, "key"), row))
        .collect()
}

/// The `to json` documents a multi-statement script printed, in the order it printed them.
fn documents(run: &ono_testkit::Run) -> Vec<Yaml> {
    run.stdout()
        .lines()
        .filter(|line| line.trim_start().starts_with('['))
        .map(support::json)
        .collect()
}

#[test]
fn should_inspect_all_twelve_temporal_settings_when_the_configuration_is_listed() {
    // §33: "Settings MUST be typed, inspectable and included in machine-readable configuration
    // metadata." `get config | to json` is that metadata, so a setting the specification names
    // and the listing omits is not inspectable, and a thirteenth one nothing declares is surface
    // nobody documented.
    let home = scratch();
    let served: BTreeSet<String> = inspected(&home).keys().cloned().collect();
    let declared: BTreeSet<String> = declared().into_iter().map(|(key, _, _)| key).collect();

    assert_eq!(
        declared.len(),
        DECLARED_SETTINGS,
        "v0.5 §33 defines {DECLARED_SETTINGS} canonical settings, and \
         docs/contracts/temporal/temporal.yaml declares {}",
        declared.len()
    );
    assert_eq!(
        served, declared,
        "v0.5 §33: every canonical temporal setting is inspectable through `get config`, and \
         `get config` lists no temporal setting the registry does not declare"
    );
}

#[test]
fn should_type_every_temporal_setting_as_the_registry_declares_when_it_is_inspected() {
    // §33's "typed" is observable: the row carries the type, and a consumer that reads the
    // configuration metadata learns from it what a value for the key may be.
    let home = scratch();
    let served = inspected(&home);
    for (key, ty, _) in declared() {
        let row = served
            .get(&key)
            .unwrap_or_else(|| panic!("v0.5 §33: `get config` must report `{key}`"));
        assert_eq!(
            support::text(row, "type"),
            ty,
            "v0.5 §33: `{key}` is declared `{ty}` in docs/contracts/temporal/temporal.yaml, and \
             the shell must report the same type, got {row:?}"
        );
    }
}

#[test]
fn should_default_every_temporal_setting_to_the_declared_figure_when_nothing_is_configured() {
    // §33 fixes each default, and `docs/contracts/temporal/temporal.yaml` is where they live
    // (§36.1). The comparison is made by the shell itself — `where default_value == 512MiB`
    // survives only when the shell's own equality says the two are one figure — so this holds
    // whatever internal representation the catalogue chose.
    let home = scratch();
    let settings = declared();
    let script = settings
        .iter()
        .map(|(key, _, default)| {
            format!("get config {key} | where default_value == {default} | select key | to json")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let run = configured_shell(&home, &script);
    run.assert_success();

    let answers = documents(&run);
    assert_eq!(
        answers.len(),
        settings.len(),
        "one answer per setting, got {:?}",
        run.output()
    );
    for ((key, _, default), answer) in settings.iter().zip(answers) {
        let matched = support::items(&answer);
        assert_eq!(
            matched.len(),
            1,
            "v0.5 §33: `{key}` defaults to `{default}`, and the shell's own equality rejected \
             that figure — the setting it serves is {:?}",
            configured_shell(&home, &format!("get config {key} | to json")).stdout()
        );
        assert_eq!(
            support::text(&matched[0], "key"),
            *key,
            "the answer belongs to the setting that was asked about"
        );
    }
}

#[test]
fn should_serve_the_declared_default_as_the_value_when_no_layer_configures_it() {
    // A default is only a default while nothing overrides it, and `layer` is where the shell says
    // so. On an installation with no configuration file every temporal setting must be answered
    // from the built-in layer, with the value the default states — which is also what makes the
    // previous test's comparison a statement about this shell and not only about its registry.
    let home = scratch();
    for (key, row) in inspected(&home) {
        assert_eq!(
            support::text(&row, "layer"),
            "default",
            "v0.5 §33: nothing configures `{key}` on a fresh installation, so it is served from \
             the default layer, got {row:?}"
        );
        assert_eq!(
            row["value"], row["default_value"],
            "v0.5 §33: an unconfigured `{key}` holds its own default, got {row:?}"
        );
    }
}

#[test]
fn should_keep_recording_disabled_when_a_shell_starts_with_no_configuration() {
    // §33: "temporal.recording.enabled = false". §10.2: "Persistent recording MUST be disabled by
    // default." §2's sixteenth invariant says the same thing a third time, which is how much this
    // one figure matters — every other temporal default describes a recorder the user asked for.
    let home = scratch();
    let run = configured_shell(&home, "get config temporal.recording.enabled | to json");
    run.assert_success();
    let row = support::single_result(&run);

    assert_eq!(
        row["value"].as_bool(),
        Some(false),
        "v0.5 §10.2, §33, §2.16: recording is off until a user switches it on, got {row:?}"
    );
    assert_eq!(
        row["default_value"].as_bool(),
        Some(false),
        "v0.5 §33: the declared default is `false`, whatever a session was told, got {row:?}"
    );
    assert_eq!(
        support::text(&row, "layer"),
        "default",
        "v0.5 §10.2: a fresh installation configures nothing, so the answer comes from the \
         built-in default, got {row:?}"
    );
}

#[test]
fn should_refuse_a_wrongly_typed_assignment_and_keep_the_setting_when_one_is_made() {
    // §33's "typed" is a rule about writes as well as reads: a setting that accepted a string
    // where its type says int would be a name with a value, not a typed setting. The refusal is
    // the structured `type.mismatch` of spec §43, and the observable half that matters is what
    // the setting holds afterwards — a refused write leaves the previous value in place.
    let home = scratch();
    let run = configured_shell(
        &home,
        "set config temporal.session.max_events \"not-a-number\"\n\
         get config temporal.session.max_events | to json",
    );

    assert!(
        run.output().contains("type.mismatch"),
        "v0.5 §33: `temporal.session.max_events` is an int, and assigning a string to it is \
         refused with the structured `type.mismatch` of spec §43, got {:?}",
        run.output()
    );
    let row = support::single_result(&run);
    assert_eq!(
        row["value"], row["default_value"],
        "v0.5 §33: the refused assignment changed nothing, so the setting still holds its \
         declared default, got {row:?}"
    );
}
