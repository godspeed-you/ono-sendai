# ADR-0115: The package provider speaks dpkg's and apt's machine formats

- Status: accepted
- Date: 2026-08-27
- Spec refs: §9.1 (Package), §11.5, §16.5, §17.1, §17.2, §31.58, §35.3, §43, §50; AGENTS.md §6;
  ADR-0006, ADR-0068, ADR-0112
- Decided by: agent (autonomous)

## Context

Spec §9.1 promises `get`, `find`, `add`, `remove` and `set package`, and §37 schedules no
provider for them, so `package.yaml` carried every command as `phase: planned`. The RED suite
fixes what a package provider is: managers discovered on `PATH`, a listing read from
`dpkg-query -W -f` in a tab-separated `Package Version Status` format, a search through
`apt-cache search`, mutations through `apt-get`, `E0401` naming what was looked for when no
manager is there, `E0403` with nothing fabricated when the listing is not in the declared
format, and `E0302` as a `failed` row when an unprivileged user asks for an install. Spec
§31.58 and AGENTS.md §6 fix the method — an explicit machine-readable mode, never a human
listing — but not which managers, which formats, or how elevation is handled.

## Decision

### 1. dpkg and apt, through the modes they document for machines

`crates/ono-provider-linux/src/packages.rs` is the `linux.packages` provider. It finds
`dpkg-query`, `apt-cache`, `apt-get` and `apt-mark` on `PATH` — never at an absolute path —
and reads exactly two answers:

- `dpkg-query -W -f '${Package}\t${Version}\t${Status}\n' [name…]` for the installed set:
  one package per line, three tab-separated fields, `install ok installed` meaning installed and
  every other status word (`config-files`, `not-installed`, `half-installed`, …) meaning not.
  dpkg-query's exit status 1 — "a name you gave is unknown" — is an ordinary answer.
- `apt-cache search <term>`: `name - description` per line, as apt-cache(8) documents; what
  dpkg has of the hits decides `installed` and `version`.

Every line is checked against the declared shape and the package-name grammar of
deb-control(5). A listing that fails — not UTF-8, a line without three fields, a name that is
not a name — is `provider.schema_violation` (E0403) for the whole answer, and no record is
made from it. Unknown is null, never fabricated (spec §35.3).

The `provider` field of a record is the manager that answered (`dpkg`), and the identity is
`provider + name`, as `ono.service/1` is: one machine can carry more than one package database.

### 2. rpm is named in the refusal and not served

Where no `dpkg-query` is on `PATH` the provider is `Unavailable` with the reason "looked for
`dpkg-query` (dpkg/apt); rpm-based systems are not served by this build". Naming rpm as
*looked for* would be a lie — nothing looks for it — and not naming it would leave a Fedora
user guessing why. `docs/STATE.md` → *Next up* carries the rpm/dnf provider; the listing side
is one query format (`rpm -qa --queryformat`) away.

### 3. A name resolves to a package whether or not it is installed

`add package foo` must act on a package dpkg does not list. `resolve` therefore answers a
well-formed name dpkg has no entry for with the package identity `(dpkg, foo)` — a record whose
`installed` is `false`, which is what dpkg's silence means — so the generic mutation can carry
it to `act`. Whether the repositories carry the name is apt-get's to say when asked. A name
that is not a package name resolves to nothing and is E0301 from the generic mutation.

### 4. `get package <name>` pushes the name down and still filters

dpkg-query is asked for the one name, and the `name` selector is also applied to what comes
back. A manager that answers a narrowed request with its whole listing must not turn the
selector into a no-op; correctness never depends on whether the push-down was honoured
(`ono_provider_api::Query`).

### 5. Elevation is refused before the manager is run

`package.yaml` declares `add`, `remove` and `set` as `privilege: elevated`, and
`capabilities.yaml` gives `package.manage` elevation `required`. An unprivileged user's
request is answered as one `failed` row carrying `io.permission_denied` (E0302) naming the
uid, **before** `apt-get` is started: the outcome is known, spec §17.2 wants elevation
visible rather than discovered by a lock file's error, and a manager run for nothing may still
touch the lock. As root, `apt-get install -y`, `apt-get remove -y` / `purge -y`, `apt-get
install -y name=version` and `apt-mark hold|unhold` carry the action; `changed` is decided by
comparing dpkg's version of the package before and after, not by reading apt's prose, and a
non-zero exit is a `failed` row with the manager's stderr as the message (`io.not_found` when
apt says it cannot locate the package, `external.exit_nonzero` otherwise).

## Consequences

- `get package | select name version | to json` against the fake managers is the two packages
  dpkg listed; `get package curl` is the one; `find package curl` is the hits by name; an empty
  `PATH` is E0401 naming dpkg and rpm; a garbage listing is E0403 with nothing on stdout.
  Tests: `crates/ono-cli/tests/containers_packages_missing.rs::should_report_provider_unavailable_when_no_package_manager_is_on_the_path`,
  `::should_list_installed_packages_when_dpkg_query_answers_in_its_machine_format`,
  `::should_resolve_one_package_when_getting_it_by_name`,
  `::should_search_the_repositories_when_finding_a_package`,
  `::should_report_a_schema_violation_when_the_manager_prints_garbage`; the parsers are
  unit-tested in the crate.
- `add|remove|set package` as an unprivileged user are one `failed` row with E0302 and exit
  status 1; `explain add package foo` names the provider and `elevated`. Tests:
  `::should_fail_with_permission_denied_when_{adding,removing}_a_package_unprivileged`,
  `::should_fail_with_permission_denied_when_setting_a_package_version_unprivileged`,
  `::should_name_the_provider_and_the_privilege_when_explaining_a_package_install`.
- `docs/spec/providers/linux-packages.yaml` declares the provider; `package.v1.yaml` is the
  record; `package.yaml` and `targets.yaml` move to phase C.
- The root path of the mutations is exercised only by hand until the container suite gains a
  root case; the unprivileged refusal is what every acceptance run proves.

## Alternatives considered

- **Running `apt list --installed` or `dpkg -l`.** Rejected: both are human listings whose
  columns change with the terminal width and the locale; `dpkg-query -f` is the format dpkg
  offers programs.
- **Running `apt-get` unprivileged and classifying its lock error.** Rejected: that is
  parsing human prose to learn a fact the uid already states, and the run may touch the lock.
- **An `ono.package/1` identity of `name` alone.** Rejected: a name is not unique across
  package databases, and the service schema already set the pattern.
