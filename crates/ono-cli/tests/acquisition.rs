//! How a package reaches the host, and what that does not decide (K11A; ADR-0606, ADR-0607).
//!
//! Three sources — a catalog artifact over the network, a local directory, a payload an
//! operating-system package placed under a system source root — and one rule: acquisition may
//! be delegated, trust, permission and activation may not (K11A §0.2). Every test here drives
//! the real `ono` binary with a scratch root as its whole world, a scratch system source root
//! under it, and — where the network is the source — a local HTTP server on a loopback port
//! serving a `.kuang` archive `kuang-sign pack` would write.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md §16)"
)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ono_kuang_protocol::content_digest;
use serde_yaml_ng::Value;

mod support;
use support::{
    catalog, catalog_with_digests, declared_manifest, items, key, kuang_shell as ono,
    kubeconfig_of, last_json_document, lay_out, sign,
};

const ECHO: &str = "dev.example.echo";

// --- fixtures ------------------------------------------------------------------------------------

/// A scratch world with nothing installed, no catalog and no system payload.
fn world() -> ono_testkit::Scratch {
    use std::os::unix::fs::PermissionsExt as _;
    let scratch = ono_testkit::scratch();
    for directory in ["plugins", "home", "sources", "system-sources", "cache"] {
        std::fs::create_dir_all(scratch.path().join(directory)).expect("the scratch layout");
    }
    // A system root is read-only to everyone but its owner, which is what a package manager
    // leaves; a developer machine's umask may say otherwise, so the fixture says it itself.
    std::fs::set_permissions(
        scratch.path().join("system-sources"),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("the root's mode");
    scratch
}

/// The manifest of the reference package at `version`, reading the scratch kubeconfig.
fn manifest(home: &ono_testkit::Scratch, version: &str) -> String {
    declared_manifest(ECHO, "echo", version, &kubeconfig_of(home))
}

/// Places a payload the way a `.deb` or `.rpm` wrapper does: `<root>/<id>/<version>/` plus the
/// sidecar naming the outer package (K11A §8.2, §10.1). Returns the payload directory.
fn system_payload(home: &ono_testkit::Scratch, id: &str, manifest_text: &str) -> PathBuf {
    let version = manifest_text
        .lines()
        .find_map(|line| line.trim().strip_prefix("version: "))
        .expect("the manifest states a version")
        .trim()
        .to_owned();
    let versions = home.path().join("system-sources").join(id);
    std::fs::create_dir_all(&versions).expect("the id directory");
    // Laid out by id first, so the contributions carry the package's own command ids, then
    // placed under the version the layout names.
    let built = lay_out(
        &home.path().join(format!("build-{version}")),
        id,
        manifest_text,
    );
    let payload = versions.join(&version);
    std::fs::rename(&built, &payload).expect("the payload moves under its version");
    std::fs::write(
        versions.join(format!("{version}.origin.yaml")),
        "format: kuang-system-origin/1\npackage: ono-plugin-echo\nmanager: apt/dpkg\n",
    )
    .expect("the sidecar");
    payload
}

/// A `.kuang` archive of `directory`: the artifact files as a plain tar (K11A §7).
fn pack(directory: &Path) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    builder.mode(tar::HeaderMode::Deterministic);
    let mut names: Vec<String> = ono_kuang_protocol::artifact_files(directory)
        .into_iter()
        .map(|file| file.path)
        .collect();
    if directory.join(ono_kuang_protocol::SIGNATURE_FILE).is_file() {
        names.push(ono_kuang_protocol::SIGNATURE_FILE.to_owned());
    }
    for name in names {
        let mut source = std::fs::File::open(directory.join(&name)).expect("the file");
        builder.append_file(&name, &mut source).expect("the entry");
    }
    builder.into_inner().expect("the archive")
}

/// A loopback HTTP server answering `GET /<name>` with the bytes registered for it, counting
/// every request it serves.
struct Server {
    url: String,
    hits: Arc<AtomicUsize>,
}

fn serve(files: Vec<(String, Vec<u8>)>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let port = listener.local_addr().expect("the address").port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream);
            let mut request = String::new();
            if reader.read_line(&mut request).unwrap_or(0) == 0 {
                continue;
            }
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
                    break;
                }
            }
            counter.fetch_add(1, Ordering::SeqCst);
            let path = request.split_whitespace().nth(1).unwrap_or("/");
            let mut stream = reader.into_inner();
            match files.iter().find(|(name, _)| format!("/{name}") == path) {
                Some((_, body)) => {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(body);
                }
                None => {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
            }
            let _ = stream.flush();
        }
    });
    Server {
        url: format!("http://127.0.0.1:{port}"),
        hits,
    }
}

/// A catalog naming the reference package's archive at the server, with the digest of the
/// directory the archive was packed from.
fn network_catalog(home: &ono_testkit::Scratch, server: &Server, version: &str, digest: &str) {
    catalog_with_digests(
        home,
        "net",
        "net",
        true,
        &[(
            ECHO,
            "echo",
            version,
            &format!("{}/echo-{version}.kuang", server.url),
            digest,
        )],
    );
}

/// A package directory outside every source, to pack from.
fn staged(home: &ono_testkit::Scratch, version: &str) -> PathBuf {
    lay_out(&home.path().join("build"), ECHO, &manifest(home, version))
}

fn text_of<'a>(record: &'a Value, name: &str) -> &'a str {
    record
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("`{name}` is a string in {record:?}"))
}

fn acquisition(home: &ono_testkit::Scratch) -> Value {
    let run = ono(home, "inspect plugin echo | select acquisition | to json");
    run.assert_success();
    let value = last_json_document(&run);
    let rows = items(&value);
    assert_eq!(rows.len(), 1, "{:?}", run.output());
    rows[0]
        .get("acquisition")
        .cloned()
        .expect("the inspection carries an acquisition record")
}

fn count(home: &ono_testkit::Scratch, script: &str) -> String {
    let run = ono(home, &format!("{script} | count | to json"));
    run.stdout().trim().to_owned()
}

fn assert_refused_with(run: &ono_testkit::Run, code: &str, why: &str) {
    assert!(
        !run.status().is_success() && run.stderr().contains(code),
        "{why}: expected {code}, got {:?}",
        run.output()
    );
}

// --- §25.3: the system package source --------------------------------------------------------

#[test]
fn should_find_a_system_provided_payload_without_installing_or_granting_anything() {
    // K11A §9, §12, §25.3 (11–13): a payload under the system root is a candidate, and only
    // that — not `INSTALLED`, not enabled, no permission decided, nothing granted.
    let home = world();
    system_payload(&home, ECHO, &manifest(&home, "0.1.0"));

    let found = ono(
        &home,
        "find plugin echo | select name version source_kind system_package installed | to json",
    );
    found.assert_success();
    let value = last_json_document(&found);
    let rows = items(&value);
    assert_eq!(rows.len(), 1, "one candidate: {:?}", found.output());
    assert_eq!(text_of(&rows[0], "source_kind"), "system-package");
    assert_eq!(text_of(&rows[0], "system_package"), "ono-plugin-echo");
    assert_eq!(rows[0].get("installed"), Some(&Value::Bool(false)));

    assert_eq!(
        count(&home, "get plugin"),
        "[0]",
        "nothing is installed by being there"
    );
    assert_eq!(
        count(&home, &format!("get capability --plugin {ECHO}")),
        "[0]",
        "and nothing is granted by being there"
    );
    let refused = ono(&home, "echo:emit --count 1");
    assert!(
        !refused.status().is_success(),
        "a payload nobody installed contributes no command: {:?}",
        refused.output()
    );
}

#[test]
fn should_install_from_the_system_source_with_no_catalog_and_record_the_lineage() {
    // K11A §8.3, §18, §20, §25.3 (14–16), §25.7 (35): the short name resolves to the system
    // payload with no catalog at all, the payload is copied — never run in place — verified,
    // and the installed copy remembers where it came from and what it is pinned to.
    let home = world();
    let payload = system_payload(&home, ECHO, &manifest(&home, "0.1.0"));

    let installed = ono(
        &home,
        "install plugin echo --confirm | select status | to json; get plugin | select id version readiness | to json",
    );
    installed.assert_success();
    assert!(
        installed.stdout().contains("\"status\":\"success\"")
            && installed.stdout().contains(
                "{\"id\":\"dev.example.echo\",\"version\":\"0.1.0\",\"readiness\":\"ready\"}"
            ),
        "{:?}",
        installed.output()
    );
    let origin = acquisition(&home);
    assert_eq!(text_of(&origin, "source_kind"), "system-package");
    assert_eq!(text_of(&origin, "system_package"), "ono-plugin-echo");
    assert_eq!(text_of(&origin, "package_manager"), "apt/dpkg");
    assert_eq!(origin.get("origin_available"), Some(&Value::Bool(true)));
    assert_eq!(
        text_of(&origin, "installed_digest"),
        content_digest(&payload),
        "the installed copy is pinned to the payload's content digest"
    );
    let plugin_home = home.path().join("plugins").join(ECHO);
    assert!(
        plugin_home.join("manifest.yaml").is_file(),
        "Ono's own copy is what runs, not the package manager's files (K11A §8.3)"
    );
    let ran = ono(&home, "echo:emit --count 2 | to json");
    ran.assert_success();
    assert!(ran.stdout().contains("[1,2]"), "{:?}", ran.output());
}

#[test]
fn should_set_aside_a_system_root_ordinary_users_can_write_to() {
    // K11A §16.1, §25.3 (18): a root writable by everyone is not a system source, whatever its
    // path says; it is set aside with a warning, and nothing under it is offered.
    use std::os::unix::fs::PermissionsExt as _;
    let home = world();
    system_payload(&home, ECHO, &manifest(&home, "0.1.0"));
    std::fs::set_permissions(
        home.path().join("system-sources"),
        std::fs::Permissions::from_mode(0o777),
    )
    .expect("the mode");

    let found = ono(&home, "find plugin echo | to json");
    assert!(
        !found.stdout().contains("system-package"),
        "nothing under the root is offered: {:?}",
        found.output()
    );
    assert!(
        found.stderr().contains("Ono-Sendai-E1607") && found.stderr().contains("writable"),
        "the root is named as set aside: {:?}",
        found.stderr()
    );
    let refused = ono(&home, "install plugin echo --confirm");
    assert_refused_with(
        &refused,
        "Ono-Sendai-E1601",
        "a spoofed root offers nothing",
    );
    assert!(refused.stderr().contains("Ono-Sendai-E1607"));
}

// --- §25.4: source selection -----------------------------------------------------------------

#[test]
fn should_prefer_the_system_payload_over_a_network_fetch_for_one_release() {
    // K11A §10.3, §19.1, §25.4 (19–20): the same release from the system root and from the
    // catalog is one release with two locations; the system copy is chosen and the network is
    // never touched.
    let home = world();
    let payload = system_payload(&home, ECHO, &manifest(&home, "0.1.0"));
    let digest = content_digest(&payload);
    let server = serve(vec![("echo-0.1.0.kuang".to_owned(), pack(&payload))]);
    network_catalog(&home, &server, "0.1.0", &digest);

    let found = ono(&home, "find plugin echo | select source_kind | to json");
    found.assert_success();
    let value = last_json_document(&found);
    assert_eq!(
        items(&value).len(),
        1,
        "one release, not two: {:?}",
        found.output()
    );
    assert_eq!(text_of(&items(&value)[0], "source_kind"), "system-package");

    ono(&home, "install plugin echo --confirm | count").assert_success();
    assert_eq!(
        text_of(&acquisition(&home), "source_kind"),
        "system-package"
    );
    assert_eq!(server.hits.load(Ordering::SeqCst), 0, "nothing was fetched");
}

#[test]
fn should_refuse_one_version_offered_with_two_different_contents() {
    // K11A §19.2, §25.4 (21), invariant 8: the system payload and the catalog's digest disagree
    // about 0.1.0. Nothing resolves that by order; the refusal lists every voice.
    let home = world();
    system_payload(&home, ECHO, &manifest(&home, "0.1.0"));
    let other = staged(&home, "0.1.0");
    std::fs::write(other.join("contributions/commands.yaml"), "commands: []\n")
        .expect("a different content");
    let server = serve(vec![("echo-0.1.0.kuang".to_owned(), pack(&other))]);
    network_catalog(&home, &server, "0.1.0", &content_digest(&other));

    let refused = ono(&home, "install plugin echo --confirm");
    assert_refused_with(
        &refused,
        "Ono-Sendai-E1605",
        "Gate: a supply-chain conflict fails closed",
    );
    assert!(
        refused.stderr().contains("system package") && refused.stderr().contains("catalog"),
        "both voices are named: {:?}",
        refused.stderr()
    );
    assert_eq!(count(&home, "get plugin"), "[0]");
    let sources = ono(
        &home,
        "try { install plugin echo --confirm } catch e { $e | to json }",
    );
    assert!(
        sources.stdout().matches("\"digest\"").count() >= 2,
        "the structured conflict carries every source with its digest: {:?}",
        sources.output()
    );
    // A person selects: the system copy, deliberately.
    let chosen = ono(
        &home,
        "install plugin echo --source system --confirm | count",
    );
    chosen.assert_success();
    assert_eq!(
        text_of(&acquisition(&home), "source_kind"),
        "system-package"
    );
    assert_eq!(server.hits.load(Ordering::SeqCst), 0);
}

#[test]
fn should_keep_two_ids_under_one_name_ambiguous_however_local_one_of_them_is() {
    // K11A §10.3, §19.3, §25.4 (22): a system payload named `echo` under another id does not
    // win over the catalog's `echo` by being on the disk.
    let home = world();
    lay_out(
        &home.path().join("sources"),
        ECHO,
        &manifest(&home, "0.1.0"),
    );
    catalog(
        &home,
        "test",
        "test",
        &[(
            ECHO,
            "echo",
            "0.1.0",
            "git:https://example.invalid/echo#v0.1.0",
        )],
    );
    let other = "dev.other.echo";
    system_payload(
        &home,
        other,
        &declared_manifest(other, "echo", "0.1.0", &kubeconfig_of(&home))
            .replace("publisher: dev.example", "publisher: dev.other"),
    );

    let refused = ono(&home, "install plugin echo --confirm");
    assert_refused_with(
        &refused,
        "Ono-Sendai-E1602",
        "two ids under one name stay ambiguous",
    );
    assert!(
        refused.stderr().contains("dev.other.echo")
            && refused.stderr().contains("dev.example.echo"),
        "{:?}",
        refused.stderr()
    );
    let named = ono(&home, "install plugin dev.other.echo --confirm | count");
    named.assert_success();
    assert_eq!(
        text_of(&acquisition(&home), "source_kind"),
        "system-package"
    );
}

#[test]
fn should_take_the_source_a_person_names_over_the_default_order() {
    // K11A §10.4, §25.4 (23): `--source catalog` fetches although a system copy exists, and the
    // install says so.
    let home = world();
    let payload = system_payload(&home, ECHO, &manifest(&home, "0.1.0"));
    let digest = content_digest(&payload);
    let server = serve(vec![("echo-0.1.0.kuang".to_owned(), pack(&payload))]);
    network_catalog(&home, &server, "0.1.0", &digest);

    let run = ono(
        &home,
        "install plugin echo --source catalog --confirm | count",
    );
    run.assert_success();
    assert_eq!(server.hits.load(Ordering::SeqCst), 1, "fetched once");
    let origin = acquisition(&home);
    assert_eq!(text_of(&origin, "source_kind"), "catalog-network");
    assert!(text_of(&origin, "source_identity").starts_with(&server.url));
    assert_eq!(text_of(&origin, "catalog"), "net");
}

// --- §25.1: catalog and network --------------------------------------------------------------

#[test]
fn should_fetch_a_catalog_artifact_into_staging_verify_it_and_install() {
    // K11A §6.2, §6.4, §16.3, §25.1 (1–2): the archive lands in the cache's staging area, is
    // unpacked, hashed against the catalog's digest, and only then becomes a local copy the
    // transaction installs from. Staging is empty afterwards.
    let home = world();
    let built = staged(&home, "0.1.0");
    let server = serve(vec![("echo-0.1.0.kuang".to_owned(), pack(&built))]);
    network_catalog(&home, &server, "0.1.0", &content_digest(&built));

    let run = ono(
        &home,
        "install plugin echo --confirm | select status | to json; echo:emit --count 3 | to json",
    );
    run.assert_success();
    assert!(run.stdout().contains("[1,2,3]"), "{:?}", run.output());
    let cache = home.path().join("cache/ono/kuang/packages");
    assert!(
        cache
            .join(ECHO)
            .join("0.1.0")
            .join("manifest.yaml")
            .is_file(),
        "the verified payload is cached by id and version"
    );
    let staging = cache.join(".staging");
    assert!(
        !staging.exists() || std::fs::read_dir(&staging).map(|d| d.count()).unwrap_or(0) == 0,
        "nothing is left in staging"
    );
    let origin = acquisition(&home);
    assert_eq!(text_of(&origin, "source_kind"), "catalog-network");
    assert_eq!(text_of(&origin, "installed_digest"), content_digest(&built));
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);
}

#[test]
fn should_refuse_a_fetched_artifact_whose_digest_is_not_the_catalogs() {
    // K11A §6.2, §25.1 (3, 5): a digest mismatch fails before anything is installed, and it
    // leaves no package, no permission and no cached copy behind.
    let home = world();
    let built = staged(&home, "0.1.0");
    let server = serve(vec![("echo-0.1.0.kuang".to_owned(), pack(&built))]);
    network_catalog(
        &home,
        &server,
        "0.1.0",
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
    );

    let refused = ono(&home, "install plugin echo --confirm");
    assert_refused_with(
        &refused,
        "Ono-Sendai-K11003",
        "the artifact is not the one vouched for",
    );
    assert_eq!(count(&home, "get plugin"), "[0]");
    assert_eq!(
        count(&home, &format!("get capability --plugin {ECHO}")),
        "[0]"
    );
    assert!(
        !home
            .path()
            .join("cache/ono/kuang/packages")
            .join(ECHO)
            .exists(),
        "a mismatching payload is not cached"
    );
}

#[test]
fn should_refuse_a_fetched_artifact_whose_signature_does_not_verify() {
    // K11A §6.2 step 5, §25.1 (4): the digest matches the catalog — the catalog is wrong or
    // complicit — and the package's own signature does not cover the bytes. Refused, with the
    // signature's code, before any install.
    let home = world();
    let built = staged(&home, "0.1.0");
    sign(&built, &key(1));
    std::fs::write(built.join("contributions/commands.yaml"), "commands: []\n")
        .expect("tampered after signing");
    let server = serve(vec![("echo-0.1.0.kuang".to_owned(), pack(&built))]);
    network_catalog(&home, &server, "0.1.0", &content_digest(&built));

    let refused = ono(&home, "install plugin echo --confirm");
    assert_refused_with(
        &refused,
        "Ono-Sendai-K11004",
        "a signature that does not verify blocks",
    );
    assert_eq!(count(&home, "get plugin"), "[0]");
}

#[test]
fn should_leave_nothing_behind_when_the_fetch_fails() {
    // K11A §25.1 (5): the server has no such file. The refusal names the artifact; no state.
    let home = world();
    let built = staged(&home, "0.1.0");
    let server = serve(Vec::new());
    network_catalog(&home, &server, "0.1.0", &content_digest(&built));

    let refused = ono(&home, "install plugin echo --confirm");
    assert_refused_with(
        &refused,
        "Ono-Sendai-E1603",
        "a failed fetch is unavailable, not installed",
    );
    assert!(
        refused.stderr().contains("echo-0.1.0.kuang"),
        "{:?}",
        refused.stderr()
    );
    assert_eq!(count(&home, "get plugin"), "[0]");
    assert!(
        !home
            .path()
            .join("cache/ono/kuang/packages")
            .join(ECHO)
            .exists()
    );
}

#[test]
fn should_verify_a_cached_copy_again_rather_than_trust_the_cache() {
    // K11A §6.4, §25.1 (6): a cache hit does not bypass verification. The cached payload is
    // altered between two installs, and the second sees a copy that is not what the catalog
    // vouches for.
    let home = world();
    let built = staged(&home, "0.1.0");
    let server = serve(vec![("echo-0.1.0.kuang".to_owned(), pack(&built))]);
    network_catalog(&home, &server, "0.1.0", &content_digest(&built));
    ono(&home, "install plugin echo --confirm | count").assert_success();
    ono(&home, "remove plugin echo | count").assert_success();
    let cached = home
        .path()
        .join("cache/ono/kuang/packages")
        .join(ECHO)
        .join("0.1.0");
    std::fs::write(cached.join("contributions/commands.yaml"), "commands: []\n")
        .expect("the cache is altered");

    let refused = ono(&home, "install plugin echo --confirm");
    assert_refused_with(
        &refused,
        "Ono-Sendai-E1605",
        "the altered cache disagrees with the catalog and is not silently used",
    );
    assert_eq!(count(&home, "get plugin"), "[0]");
}

#[test]
fn should_refuse_a_plain_http_artifact_the_catalog_did_not_allow() {
    // K11A §6.3: plain `http://` needs an operator catalog that says so; without it the
    // catalog document is refused as a whole and the name resolves to nothing.
    let home = world();
    let built = staged(&home, "0.1.0");
    let server = serve(vec![("echo-0.1.0.kuang".to_owned(), pack(&built))]);
    catalog_with_digests(
        &home,
        "net",
        "net",
        false,
        &[(
            ECHO,
            "echo",
            "0.1.0",
            &format!("{}/echo-0.1.0.kuang", server.url),
            &content_digest(&built),
        )],
    );

    let refused = ono(&home, "install plugin echo --confirm");
    assert_refused_with(&refused, "Ono-Sendai-E1601", "the catalog was not read");
    let found = ono(&home, "find plugin echo | count");
    assert!(
        found.stderr().contains("insecure_http"),
        "the reason is the transport rule: {:?}",
        found.stderr()
    );
    assert_eq!(server.hits.load(Ordering::SeqCst), 0);
}

// --- §25.5: upgrades ---------------------------------------------------------------------------

#[test]
fn should_keep_the_installed_copy_when_the_package_manager_replaces_the_source() {
    // K11A §14.1, §14.4, §25.5 (24), §25.6 (32): `apt upgrade` swaps 0.1.0 for 0.1.1 under the
    // system root. The installed copy is still 0.1.0, still runs, and says its origin is gone.
    let home = world();
    system_payload(&home, ECHO, &manifest(&home, "0.1.0"));
    ono(&home, "install plugin echo --confirm | count").assert_success();
    std::fs::remove_dir_all(home.path().join("system-sources").join(ECHO)).expect("apt removed it");
    system_payload(&home, ECHO, &manifest(&home, "0.1.1"));

    let still = ono(
        &home,
        "get plugin echo | select version | to json; echo:emit --count 1 | to json",
    );
    still.assert_success();
    assert!(
        still.stdout().contains("\"version\":\"0.1.0\"") && still.stdout().contains("[1]"),
        "{:?}",
        still.output()
    );
    let origin = acquisition(&home);
    assert_eq!(origin.get("origin_available"), Some(&Value::Bool(false)));
    let found = ono(
        &home,
        "find plugin echo | select version source_kind installed | to json",
    );
    found.assert_success();
    assert!(
        found.stdout().contains("\"version\":\"0.1.1\"")
            && found.stdout().contains("\"installed\":false"),
        "the new payload is a candidate, not an upgrade that happened: {:?}",
        found.output()
    );
}

#[test]
fn should_upgrade_from_the_system_lineage_only_when_asked_and_only_with_consent() {
    // K11A §14.2, §14.3, §25.5 (25–27): `install plugin echo` takes the newer system release;
    // a release that widens a scope is new consent, and mutation is still not granted.
    let home = world();
    system_payload(&home, ECHO, &manifest(&home, "0.1.0"));
    ono(&home, "install plugin echo --confirm | count").assert_success();
    system_payload(&home, ECHO, &manifest(&home, "0.1.1"));

    let upgraded = ono(
        &home,
        "install plugin echo --confirm | select status | to json; get plugin echo | select version readiness | to json",
    );
    upgraded.assert_success();
    assert!(
        upgraded
            .stdout()
            .contains("{\"version\":\"0.1.1\",\"readiness\":\"ready\"}"),
        "{:?}",
        upgraded.output()
    );
    assert_eq!(
        text_of(&acquisition(&home), "source_kind"),
        "system-package"
    );
    let mutation = ono(
        &home,
        "get permission echo | where id == \"cluster-mutation\" | select state | to json",
    );
    assert!(
        mutation.stdout().contains("\"denied\""),
        "{:?}",
        mutation.output()
    );

    // 0.1.2 reads every file under the home: a widened scope no `apt upgrade` consents to.
    let wide = format!("{}/**", home.path().join("home").display());
    system_payload(
        &home,
        ECHO,
        &declared_manifest(ECHO, "echo", "0.1.2", &wide),
    );
    let refused = ono(&home, "install plugin echo");
    assert_refused_with(
        &refused,
        "Ono-Sendai-E0701",
        "a widened scope is new consent",
    );
    assert!(
        refused.stderr().contains("requests additional access"),
        "{:?}",
        refused.stderr()
    );
    let still = ono(&home, "get plugin echo | select version | to json");
    assert!(still.stdout().contains("0.1.1"));
}

#[test]
fn should_keep_the_lineage_it_was_installed_through_until_a_person_changes_it() {
    // K11A §18, §27, §25.5 (28): installed from a local source, a system payload that appears
    // later does not become the upgrade path by appearing. `--source system` is the deliberate
    // change, and the origin says so afterwards.
    let home = world();
    lay_out(
        &home.path().join("sources"),
        ECHO,
        &manifest(&home, "0.1.0"),
    );
    catalog(
        &home,
        "test",
        "test",
        &[(
            ECHO,
            "echo",
            "0.1.0",
            "git:https://example.invalid/echo#v0.1.0",
        )],
    );
    ono(&home, "install plugin echo --confirm | count").assert_success();
    assert_eq!(text_of(&acquisition(&home), "source_kind"), "local-path");
    system_payload(&home, ECHO, &manifest(&home, "0.1.1"));

    let unchanged = ono(&home, "install plugin echo --confirm");
    assert!(
        !unchanged.status().is_success() && unchanged.stderr().contains("already installed"),
        "the local lineage offers nothing newer, and the system payload is not taken silently: {:?}",
        unchanged.output()
    );
    let still = ono(&home, "get plugin echo | select version | to json");
    assert!(still.stdout().contains("0.1.0"));

    let switched = ono(
        &home,
        "install plugin echo --source system --confirm | count",
    );
    switched.assert_success();
    let origin = acquisition(&home);
    assert_eq!(text_of(&origin, "source_kind"), "system-package");
    let version = ono(&home, "get plugin echo | select version | to json");
    assert!(version.stdout().contains("0.1.1"));
}

// --- §25.6: removal ----------------------------------------------------------------------------

#[test]
fn should_remove_onos_copy_and_leave_the_system_source_where_the_package_manager_put_it() {
    // K11A §15.1, §25.6 (30): `remove plugin` removes Ono's state and says the system source
    // remains; it never runs the package manager, so the payload is still there.
    let home = world();
    let payload = system_payload(&home, ECHO, &manifest(&home, "0.1.0"));
    ono(&home, "install plugin echo --confirm | count").assert_success();

    let removed = ono(&home, "remove plugin echo | select status | to json");
    removed.assert_success();
    assert!(
        removed
            .stderr()
            .contains("system-provided source remains available")
            && removed.stderr().contains("ono-plugin-echo"),
        "{:?}",
        removed.output()
    );
    assert!(
        payload.join("manifest.yaml").is_file(),
        "the payload is the package manager's"
    );
    assert_eq!(count(&home, "get plugin"), "[0]");
    assert_eq!(
        count(&home, &format!("get capability --plugin {ECHO}")),
        "[0]"
    );
    let again = ono(
        &home,
        "find plugin echo | select source_kind installed | to json",
    );
    assert!(
        again
            .stdout()
            .contains("\"source_kind\":\"system-package\",\"installed\":false"),
        "and it is a candidate again: {:?}",
        again.output()
    );
}

#[test]
fn should_install_a_signed_system_payload_unattended_and_state_its_trust_as_unknown() {
    // K11A §2.5, §11.1, §13: the operator's package manager is the provenance a fleet relies on,
    // so a signed payload from a system package installs unattended even when no trust store
    // enrols its key — and the trust facts stay what they are: signature valid, publisher
    // unknown, never "trusted because root installed it".
    let home = world();
    let built = staged(&home, "0.1.0");
    sign(&built, &key(7));
    let versions = home.path().join("system-sources").join(ECHO);
    std::fs::create_dir_all(&versions).expect("the id directory");
    std::fs::rename(&built, versions.join("0.1.0")).expect("the payload moves under its version");
    std::fs::write(
        versions.join("0.1.0.origin.yaml"),
        "format: kuang-system-origin/1\npackage: ono-plugin-echo\nmanager: apt/dpkg\n",
    )
    .expect("the sidecar");
    // The bootstrap-style situation: a catalog names the package too, with a local artifact.
    catalog(
        &home,
        "official",
        "official",
        &[(
            ECHO,
            "echo",
            "0.1.0",
            "git:https://example.invalid/echo#v0.1.0",
        )],
    );

    let installed = ono(
        &home,
        "install plugin echo --confirm | select status | to json; verify plugin echo | select signature trust | to json",
    );
    installed.assert_success();
    assert!(
        installed.stdout().contains("{\"status\":\"success\"}")
            && installed
                .stdout()
                .contains("{\"signature\":\"valid\",\"trust\":\"unknown\"}"),
        "{:?}",
        installed.output()
    );
    assert!(
        installed.stderr().contains("no trust store enrols")
            && installed.stderr().contains("system package"),
        "the unknown trust is said, and root is not trust: {:?}",
        installed.stderr()
    );
    assert_eq!(
        text_of(&acquisition(&home), "source_kind"),
        "system-package"
    );
}
