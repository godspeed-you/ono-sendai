# ADR-0122: What installing and removing the package does to `/etc/shells` and to users

- Status: accepted
- Date: 2026-08-27
- Spec refs: §37 (a shell usable as a login shell), §4; docs/ACCEPTANCE.md §4.5
- Decided by: agent (autonomous)

## Context

`chsh -s /usr/bin/ono` is refused unless `/usr/bin/ono` is listed in `/etc/shells`, and
`login`, `sshd` and PAM consult the same file. A package that installs a shell but leaves the
registration to the user has not installed a shell. The other half is just as important: a
package that de-registers the shell at the wrong moment — on an upgrade, say — locks every user
whose shell is `ono` out of their own account. The spec says nothing about installation; the
acceptance container registers the shell by hand (`useradd --shell`). This ADR states what the
packages do and, as importantly, what they refuse to do.

## Decision

1. **Install registers.** After the files are unpacked, `/usr/bin/ono` is added to `/etc/shells`
   exactly once:
   - deb `postinst configure`: `add-shell /usr/bin/ono` (debianutils; the package depends on it).
     `add-shell` is idempotent, and `configure` runs on first install, on upgrade and on
     `dpkg-reconfigure` alike — all three leave one entry.
   - rpm `%post`: append the line unless `grep -qx` already finds it. `%post` runs with `$1 = 1`
     on install and `$1 = 2` on upgrade; the guard makes both harmless.

2. **Removal unregisters — only real removal.**
   - deb `postrm`: `remove-shell /usr/bin/ono` on `remove` and `purge`, nothing on `upgrade`,
     `failed-upgrade`, `abort-*`. dpkg calls the *old* package's postrm during an upgrade, which
     is exactly the moment the entry must survive.
   - rpm `%postun`: delete the line only when `$1 = 0` (the last instance is gone); `$1 = 1` is
     an upgrade and leaves the file alone.

3. **Users are never touched.** Neither install nor removal rewrites `/etc/passwd` or the shell
   of any account. A user makes `ono` their login shell with `chsh -s /usr/bin/ono`, and a user
   who removes the package while it is still their shell is in the same position as with any
   other removed shell on Debian or Fedora: `chsh` back first, or the administrator does. The
   README says so next to the install instructions. Silently resetting people's shells to
   `/bin/sh` is not this package's business — it would be a policy the user never agreed to,
   and it would also be wrong on systems where accounts come from LDAP or systemd-homed.

4. **The registration is not a conffile.** `/etc/shells` belongs to the base system and is
   edited through the mechanisms above; the package neither ships nor owns it.

## Consequences

- `xtask/tests/packaging.rs` requires the `add-shell`/`remove-shell` lines in the deb scripts and
  `/etc/shells` plus `/usr/bin/ono` in both rpm scripts.
- `scripts/package-check.sh` proves the behaviour in fresh containers: after install the entry
  is present and an unprivileged user created with `--shell /usr/bin/ono` can log in
  (`su - user`) and run a pipeline; after `apt-get remove` / `dnf remove` the entry is gone.
- `remove-shell` is only called when it exists, so a `postrm` on a system that lost debianutils
  mid-removal still exits 0 and does not leave the package half-removed.

## Alternatives considered

- **Leave `/etc/shells` to the user** — a documented manual step is the status quo the packages
  exist to replace. Rejected.
- **Reset affected users' shells on removal** — protective in the common case, destructive in
  every uncommon one (remote accounts, a deliberate reinstall, a shell path that is also a
  symlink target). Rejected; documented instead.
- **Ship `/etc/shells` as a conffile** — would make the package own a system file it shares
  with every other shell. Rejected.
