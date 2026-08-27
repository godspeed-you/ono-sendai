#!/usr/bin/env bash
# The referee for packaging (ADR-0121, ADR-0122): the packages `scripts/package.sh` wrote to
# dist/ are installed in fresh containers — `debian:bookworm` for the .deb, `fedora:latest` for
# the .rpm — with networking disabled, and must work there as root and as an unprivileged user
# whose login shell is /usr/bin/ono; removing them must unregister the shell again.
#
# A package for a foreign architecture (one the host cannot execute) is verified structurally
# only: the declared architecture and the ELF machine of the packaged binary. Its runtime proof
# is the same script on a native runner, which is what .github/workflows/release.yml does.
#
# usage: scripts/package-check.sh [--target <triple>] [--keep-image]
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

DEBIAN_IMAGE="${ONO_PACKAGE_CHECK_DEBIAN:-debian:bookworm}"
FEDORA_IMAGE="${ONO_PACKAGE_CHECK_FEDORA:-fedora:latest}"

host_triple="$(rustc -vV | sed -n 's/^host: //p')"
target="$host_triple"
keep_image=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) target="$2"; shift 2 ;;
    --target=*) target="${1#--target=}"; shift ;;
    --keep-image) keep_image=1; shift ;;
    *) echo "usage: scripts/package-check.sh [--target <triple>] [--keep-image]" >&2; exit 2 ;;
  esac
done

case "$target" in
  x86_64-unknown-linux-gnu)  deb_arch=amd64; rpm_arch=x86_64;  elf_machine="3e 00" ;;
  aarch64-unknown-linux-gnu) deb_arch=arm64; rpm_arch=aarch64; elf_machine="b7 00" ;;
  *) echo "package-check: no package layout for target $target" >&2; exit 2 ;;
esac

version="$(cargo pkgid --package ono-cli | sed 's/.*[#@]//')"
deb="ono_${version}_${deb_arch}.deb"
rpm="ono-${version}-1.${rpm_arch}.rpm"
for package in "$deb" "$rpm"; do
  if [[ ! -f "dist/$package" ]]; then
    echo "package-check: dist/$package is missing — run scripts/package.sh --target $target" >&2
    exit 1
  fi
done

runtime=""
for candidate in docker podman; do
  if command -v "$candidate" >/dev/null 2>&1; then runtime="$candidate"; break; fi
done
if [[ -z "$runtime" ]]; then
  echo "package-check: neither docker nor podman is available" >&2
  exit 127
fi

native=0
[[ "$target" == "$host_triple" ]] && native=1

failed=0
step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
ok()   { printf '  \033[32mok\033[0m   %s\n' "$*"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$*"; failed=1; }

# Runs a script as root in a fresh container with dist/ mounted read-only and no network. The
# image is pulled beforehand so the check itself never reaches out.
in_container() {
  local image="$1" script="$2"
  "$runtime" image inspect "$image" >/dev/null 2>&1 || "$runtime" pull --quiet "$image" >/dev/null
  "$runtime" run --rm --network none \
    --volume "$PWD/dist:/dist:ro" \
    --env "PKG=/dist/$3" --env "ELF_MACHINE=$elf_machine" \
    --env "DEB_ARCH=$deb_arch" --env "RPM_ARCH=$rpm_arch" --env "VERSION=$version" \
    --env "NATIVE=$native" \
    "$image" sh -c "$script"
}

# --- .deb --------------------------------------------------------------------------------
#
# Bytes 18-19 of an ELF header are e_machine, little-endian: what a binary is for, checked
# without running it — the only check possible for a foreign architecture.

step "$deb in $DEBIAN_IMAGE (structure)"
if in_container "$DEBIAN_IMAGE" '
set -e
dpkg-deb --info "$PKG" | grep -q "^ Package: ono$"      || { echo "package name"; exit 1; }
dpkg-deb --info "$PKG" | grep -q "^ Version: $VERSION$"  || { echo "version"; exit 1; }
dpkg-deb --info "$PKG" | grep -q "^ Architecture: $DEB_ARCH$" || { echo "architecture"; exit 1; }
dpkg-deb --info "$PKG" | grep -q "^ Section: shells$"    || { echo "section"; exit 1; }
if [ "$NATIVE" = 1 ]; then
  dpkg-deb --info "$PKG" | grep "^ Depends:" | grep -q "libc6 (>= " || { echo "no computed libc6 dependency"; exit 1; }
fi
dpkg-deb --fsys-tarfile "$PKG" | tar -xO ./usr/bin/ono > /tmp/ono
machine="$(od -An -tx1 -j18 -N2 /tmp/ono | tr -s " " | sed "s/^ //;s/ $//")"
[ "$machine" = "$ELF_MACHINE" ] || { echo "binary e_machine is [$machine], expected [$ELF_MACHINE]"; exit 1; }
' "$deb"; then
  ok "declares Architecture: $deb_arch and packages an ELF for it"
else
  fail "$deb structure"
fi

if [[ $native -eq 1 ]]; then
  step "$deb in $DEBIAN_IMAGE (install, run, login shell, remove)"
  if in_container "$DEBIAN_IMAGE" '
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get install --yes --no-install-recommends "$PKG" >/tmp/apt.log 2>&1 || { cat /tmp/apt.log; exit 1; }
echo "--- installed"
ono --version
grep -qx /usr/bin/ono /etc/shells || { echo "/etc/shells lacks /usr/bin/ono after install"; exit 1; }
echo "--- as root"
count="$(ono -c "get process | count | to json")"
echo "get process | count | to json => $count"
echo "$count" | grep -Eq "[1-9]" || { echo "expected a process count"; exit 1; }
echo "--- as an unprivileged user whose login shell is ono"
useradd --create-home --shell /usr/bin/ono probe
[ "$(getent passwd probe | cut -d: -f7)" = /usr/bin/ono ]
su - probe -c "echo hi" | grep -qx hi || { echo "login shell -c failed"; exit 1; }
su - probe -c "get process | where pid == 1 | select name | to json" | grep -q "\"name\"" \
  || { echo "a pipeline from the login shell failed"; exit 1; }
echo "--- remove"
apt-get remove --yes ono >/tmp/apt.log 2>&1 || { cat /tmp/apt.log; exit 1; }
! grep -q /usr/bin/ono /etc/shells || { echo "/etc/shells still lists /usr/bin/ono after removal"; exit 1; }
! [ -e /usr/bin/ono ] || { echo "/usr/bin/ono survived removal"; exit 1; }
apt-get install --yes --no-install-recommends "$PKG" >/tmp/apt.log 2>&1 || { cat /tmp/apt.log; exit 1; }
apt-get install --yes --reinstall --no-install-recommends "$PKG" >/tmp/apt.log 2>&1 || { cat /tmp/apt.log; exit 1; }
[ "$(grep -cx /usr/bin/ono /etc/shells)" = 1 ] || { echo "reinstalling duplicated the /etc/shells entry"; exit 1; }
' "$deb"; then
    ok "installs, runs as root and as a login shell, registers and unregisters /usr/bin/ono"
  else
    fail "$deb install/run/remove"
  fi
fi

# --- .rpm --------------------------------------------------------------------------------
#
# Fedora's container image ships neither `su` nor `runuser`, and the check must not reach the
# network once it starts, so the image is prepared once with util-linux — that is the only
# addition, and the package is still installed into a system that has never seen it.

fedora_check_image="ono-package-check:fedora"
step "preparing $fedora_check_image from $FEDORA_IMAGE"
printf 'FROM %s\nRUN dnf --assumeyes install util-linux && dnf clean all\n' "$FEDORA_IMAGE" \
  | "$runtime" build --quiet --tag "$fedora_check_image" - >/dev/null

step "$rpm in $FEDORA_IMAGE (structure)"
if in_container "$fedora_check_image" '
set -e
[ "$(rpm -qp --qf "%{NAME}" "$PKG")" = ono ]              || { echo "package name"; exit 1; }
[ "$(rpm -qp --qf "%{VERSION}-%{RELEASE}" "$PKG")" = "$VERSION-1" ] || { echo "version"; exit 1; }
[ "$(rpm -qp --qf "%{ARCH}" "$PKG")" = "$RPM_ARCH" ]      || { echo "architecture"; exit 1; }
rpm -qpl "$PKG" | grep -qx /usr/bin/ono                    || { echo "file list"; exit 1; }
rpm -qp --scripts "$PKG" | grep -q /etc/shells             || { echo "scripts"; exit 1; }
if [ "$NATIVE" = 1 ]; then
  rpm -qp --requires "$PKG" | grep -q "^libc.so.6("         || { echo "no computed libc.so.6 requirement"; exit 1; }
fi
rpm2archive - < "$PKG" > /tmp/ono.tgz && tar -xzOf /tmp/ono.tgz ./usr/bin/ono > /tmp/ono
machine="$(od -An -tx1 -j18 -N2 /tmp/ono | tr -s " " | sed "s/^ //;s/ $//")"
[ "$machine" = "$ELF_MACHINE" ] || { echo "binary e_machine is [$machine], expected [$ELF_MACHINE]"; exit 1; }
' "$rpm"; then
  ok "declares arch $rpm_arch and packages an ELF for it"
else
  fail "$rpm structure"
fi

if [[ $native -eq 1 ]]; then
  step "$rpm in $FEDORA_IMAGE (install, run, login shell, remove)"
  if in_container "$fedora_check_image" '
set -e
dnf --disablerepo="*" --assumeyes install "$PKG" >/tmp/dnf.log 2>&1 || { cat /tmp/dnf.log; exit 1; }
echo "--- installed"
ono --version
grep -qx /usr/bin/ono /etc/shells || { echo "/etc/shells lacks /usr/bin/ono after install"; exit 1; }
echo "--- as root"
count="$(ono -c "get process | count | to json")"
echo "get process | count | to json => $count"
echo "$count" | grep -Eq "[1-9]" || { echo "expected a process count"; exit 1; }
echo "--- as an unprivileged user whose login shell is ono"
useradd --create-home --shell /usr/bin/ono probe
[ "$(getent passwd probe | cut -d: -f7)" = /usr/bin/ono ]
su - probe -c "echo hi" | grep -qx hi || { echo "login shell -c failed"; exit 1; }
su - probe -c "get process | where pid == 1 | select name | to json" | grep -q "\"name\"" \
  || { echo "a pipeline from the login shell failed"; exit 1; }
echo "--- reinstall keeps one entry"
dnf --disablerepo="*" --assumeyes reinstall "$PKG" >/tmp/dnf.log 2>&1 || { cat /tmp/dnf.log; exit 1; }
[ "$(grep -cx /usr/bin/ono /etc/shells)" = 1 ] || { echo "reinstalling duplicated the /etc/shells entry"; exit 1; }
echo "--- remove"
dnf --disablerepo="*" --assumeyes remove ono >/tmp/dnf.log 2>&1 || { cat /tmp/dnf.log; exit 1; }
! grep -q /usr/bin/ono /etc/shells || { echo "/etc/shells still lists /usr/bin/ono after removal"; exit 1; }
! [ -e /usr/bin/ono ] || { echo "/usr/bin/ono survived removal"; exit 1; }
' "$rpm"; then
    ok "installs, runs as root and as a login shell, registers and unregisters /usr/bin/ono"
  else
    fail "$rpm install/run/remove"
  fi
fi

# --- verdict -----------------------------------------------------------------------------

if [[ $keep_image -eq 0 ]]; then
  "$runtime" image rm --force "$fedora_check_image" >/dev/null 2>&1 || true
fi

echo
if [[ $failed -ne 0 ]]; then
  printf '\033[31mpackage-check: red\033[0m\n'
  exit 1
fi
if [[ $native -eq 0 ]]; then
  printf 'package-check: %s packages verified structurally only — this %s host cannot execute\n' \
    "$rpm_arch" "${host_triple%%-*}"
  printf 'them, and their library dependencies were not computed (ADR-0123). The install, run and\n'
  printf 'remove proof for %s is this script on a native runner (.github/workflows/release.yml).\n' \
    "$rpm_arch"
  printf '\033[1;32mpackage-check: green (structural, %s)\033[0m\n' "$rpm_arch"
else
  printf '\033[1;32mpackage-check: green\033[0m\n'
fi
