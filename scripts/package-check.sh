#!/usr/bin/env bash
# The referee for packaging (ADR-0121, ADR-0122, ADR-0531): the packages `scripts/package.sh`
# wrote to dist/ are installed in fresh containers — `debian:bookworm` and `debian:trixie` for the
# .deb, `fedora:latest` for the .rpm — with networking disabled, and must work there as root and
# as an unprivileged user whose login shell is /usr/bin/ono; removing them must unregister the
# shell again and leave the user's own configuration alone.
#
# §48.3 asks for the oldest supported glibc/distribution baseline *as well as* one current
# representative. `debian:bookworm` is the baseline and it is not a choice of convenience: it is
# the base of the image `scripts/package.sh` compiles in, so its glibc 2.36 is the floor the
# binary actually has. `GLIBC_FLOOR` states it and the structural check holds the binary to it,
# because a floor nobody measured is a claim about an image tag.
#
# A package for a foreign architecture (one the host cannot execute) is verified structurally
# only: the declared architecture and the ELF machine of the packaged binary. Its runtime proof
# is the same script on a native runner, which is what .github/workflows/release.yml does.
#
# usage: scripts/package-check.sh [--target <triple>] [--keep-image]
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# The oldest supported baseline (§48.3), which is the base of the build image, and one current
# representative. Both pinned by digest (§44.1).
DEBIAN_BASELINE="${ONO_PACKAGE_CHECK_DEBIAN:-debian:bookworm@sha256:6ebd97fa83deb272194a2cf015b3d26a4d538e9ad3a7a79d544c8af5b0a01443}"
DEBIAN_CURRENT="${ONO_PACKAGE_CHECK_DEBIAN_CURRENT:-debian:trixie@sha256:f324c7ff54321e8d9c588493a20244965938ce0aa50bbd1022d38010e9ffc4b1}"
DEBIAN_IMAGES=("$DEBIAN_BASELINE" "$DEBIAN_CURRENT")
FEDORA_IMAGE="${ONO_PACKAGE_CHECK_FEDORA:-fedora:latest@sha256:43b29f65a41eb9c35e1cd5323e3bdf3b655c2357a9f4f1ff2f9c2798e5045d80}"

# The glibc the build image supplies, and therefore the oldest a package may require.
GLIBC_FLOOR="2.36"

# Where package validation records what it installed, by digest. Outside dist/, because dist/ is
# what two builds are compared byte for byte (ADR-0527) and a record of the check is not an
# artifact of the build.
TESTED_RECORD="target/package-check.sha256"

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
    --env "NATIVE=$native" --env "GLIBC_FLOOR=$GLIBC_FLOOR" \
    --env "FILE_VERSION=$file_version" --env "FILE_ARCH=$file_arch" \
    "$image" sh -c "$script"
}

# The version and architecture the *filename* claims, so §48.2's "package metadata version/
# architecture match artifact filename" is a comparison rather than a restatement of one source.
file_version="${deb#ono_}"; file_version="${file_version%%_*}"
file_arch="${deb##*_}"; file_arch="${file_arch%.deb}"

# --- .deb --------------------------------------------------------------------------------
#
# Bytes 18-19 of an ELF header are e_machine, little-endian: what a binary is for, checked
# without running it — the only check possible for a foreign architecture.
#
# Both Debian images: the oldest supported baseline and one current representative (§48.3).

deb_structure='
set -e
dpkg-deb --info "$PKG" | grep -q "^ Package: ono$"      || { echo "package name"; exit 1; }
dpkg-deb --info "$PKG" | grep -q "^ Version: $VERSION$"  || { echo "version"; exit 1; }
dpkg-deb --info "$PKG" | grep -q "^ Architecture: $DEB_ARCH$" || { echo "architecture"; exit 1; }
dpkg-deb --info "$PKG" | grep -q "^ Section: shells$"    || { echo "section"; exit 1; }
if [ "$NATIVE" = 1 ]; then
  dpkg-deb --info "$PKG" | grep "^ Depends:" | grep -q "libc6 (>= " || { echo "no computed libc6 dependency"; exit 1; }
fi
# §48.2 check: package metadata matches the artifact filename. A file named for one version
# holding another is how a release ships the wrong package under the right name.
dpkg-deb --info "$PKG" | grep -q "^ Version: $FILE_VERSION$" \
  || { echo "the filename says version $FILE_VERSION and the metadata does not"; exit 1; }
dpkg-deb --info "$PKG" | grep -q "^ Architecture: $FILE_ARCH$" \
  || { echo "the filename says architecture $FILE_ARCH and the metadata does not"; exit 1; }
dpkg-deb --fsys-tarfile "$PKG" | tar -xO ./usr/bin/ono > /tmp/ono
machine="$(od -An -tx1 -j18 -N2 /tmp/ono | tr -s " " | sed "s/^ //;s/ $//")"
[ "$machine" = "$ELF_MACHINE" ] || { echo "binary e_machine is [$machine], expected [$ELF_MACHINE]"; exit 1; }
# §48.2 check: no private build paths are embedded. The release compiles in a container at
# /project, so a /home/<somebody>/ inside the binary means it was built on a workstation and
# carries that workstation'"'"'s directory layout to every user who installs it.
if grep -aoE "/(home|Users)/[A-Za-z0-9._-]+/" /tmp/ono | sort -u | head -5 | grep .; then
  echo "the binary embeds a private build path"; exit 1
fi
# §48.3: the binary requires no glibc newer than the baseline supplies, which is what makes the
# baseline a compatibility proof rather than a distribution somebody happened to test on.
needed="$(grep -aoE "GLIBC_2\.[0-9]+" /tmp/ono | sort -uV | tail -1)"
[ -n "$needed" ] || { echo "the binary declares no glibc requirement at all"; exit 1; }
highest="$(printf "%s\nGLIBC_%s\n" "$needed" "$GLIBC_FLOOR" | sort -uV | tail -1)"
[ "$highest" = "GLIBC_$GLIBC_FLOOR" ] \
  || { echo "the binary needs $needed and the baseline supplies GLIBC_$GLIBC_FLOOR"; exit 1; }
echo "requires at most $needed"
'

deb_runtime='
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get install --yes --no-install-recommends "$PKG" >/tmp/apt.log 2>&1 || { cat /tmp/apt.log; exit 1; }
echo "--- installed"
# §48.2 check: expected path /usr/bin/ono exists
[ -x /usr/bin/ono ] || { echo "/usr/bin/ono is not there or not executable"; exit 1; }
# §48.2 check: file ownership and mode are correct
[ "$(stat -c "%U %G %a" /usr/bin/ono)" = "root root 755" ] \
  || { echo "/usr/bin/ono is $(stat -c "%U %G %a" /usr/bin/ono), expected root root 755"; exit 1; }
# §48.2 check: binary version equals release version
installed="$(ono --version | grep -oE "[0-9]+\.[0-9]+\.[0-9]+" | head -1)"
[ "$installed" = "$VERSION" ] \
  || { echo "the installed binary reports $installed and the release is $VERSION"; exit 1; }
ono --version
grep -qx /usr/bin/ono /etc/shells || { echo "/etc/shells lacks /usr/bin/ono after install"; exit 1; }
echo "--- as root"
count="$(ono -c "get process | count | to json")"
echo "get process | count | to json => $count"
echo "$count" | grep -Eq "[1-9]" || { echo "expected a process count"; exit 1; }
echo "--- as an unprivileged user whose login shell is ono"
# §48.2 check: login-shell smoke behaviour
useradd --create-home --shell /usr/bin/ono probe
[ "$(getent passwd probe | cut -d: -f7)" = /usr/bin/ono ]
su - probe -c "echo hi" | grep -qx hi || { echo "login shell -c failed"; exit 1; }
su - probe -c "get process | where pid == 1 | select name | to json" | grep -q "\"name\"" \
  || { echo "a pipeline from the login shell failed"; exit 1; }
# The user'"'"'s own configuration, written before the package is removed.
mkdir -p /home/probe/.config/ono
echo "# the operator wrote this" > /home/probe/.config/ono/config.ono
chown -R probe:probe /home/probe/.config
echo "--- remove"
apt-get remove --yes ono >/tmp/apt.log 2>&1 || { cat /tmp/apt.log; exit 1; }
! grep -q /usr/bin/ono /etc/shells || { echo "/etc/shells still lists /usr/bin/ono after removal"; exit 1; }
! [ -e /usr/bin/ono ] || { echo "/usr/bin/ono survived removal"; exit 1; }
# §48.2 check: uninstall leaves user configuration. Removing a shell must not remove what the
# person using it wrote.
[ -f /home/probe/.config/ono/config.ono ] \
  || { echo "removing the package deleted the user configuration"; exit 1; }
grep -q "the operator wrote this" /home/probe/.config/ono/config.ono \
  || { echo "removing the package rewrote the user configuration"; exit 1; }
echo "--- reinstall"
# §48.2 check: reinstall works
apt-get install --yes --no-install-recommends "$PKG" >/tmp/apt.log 2>&1 || { cat /tmp/apt.log; exit 1; }
apt-get install --yes --reinstall --no-install-recommends "$PKG" >/tmp/apt.log 2>&1 || { cat /tmp/apt.log; exit 1; }
[ "$(grep -cx /usr/bin/ono /etc/shells)" = 1 ] || { echo "reinstalling duplicated the /etc/shells entry"; exit 1; }
[ -f /home/probe/.config/ono/config.ono ] || { echo "reinstalling lost the user configuration"; exit 1; }
'

for image in "${DEBIAN_IMAGES[@]}"; do
  step "$deb in $image (structure)"
  if in_container "$image" "$deb_structure" "$deb"; then
    ok "declares Architecture: $deb_arch, packages an ELF for it, and needs no glibc past $GLIBC_FLOOR"
  else
    fail "$deb structure in $image"
  fi

  if [[ $native -eq 1 ]]; then
    step "$deb in $image (install, run, login shell, remove, reinstall)"
    if in_container "$image" "$deb_runtime" "$deb"; then
      ok "installs, runs as root and as a login shell, removes without touching user configuration, reinstalls"
    else
      fail "$deb install/run/remove in $image"
    fi
  fi
done

# --- .rpm --------------------------------------------------------------------------------
#
# Fedora's container image ships neither `su` nor `runuser`, and the check must not reach the
# network once it starts, so the image is prepared once with util-linux — that is the only
# addition, and the package is still installed into a system that has never seen it.

file_version="${rpm#ono-}"; file_version="${file_version%%-1.*}"
file_arch="${rpm##*-1.}"; file_arch="${file_arch%.rpm}"

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
# §48.2 check: package metadata matches the artifact filename
[ "$(rpm -qp --qf "%{VERSION}" "$PKG")" = "$FILE_VERSION" ] \
  || { echo "the filename says version $FILE_VERSION and the metadata does not"; exit 1; }
[ "$(rpm -qp --qf "%{ARCH}" "$PKG")" = "$FILE_ARCH" ] \
  || { echo "the filename says architecture $FILE_ARCH and the metadata does not"; exit 1; }
rpm -qpl "$PKG" | grep -qx /usr/bin/ono                    || { echo "file list"; exit 1; }
rpm -qp --scripts "$PKG" | grep -q /etc/shells             || { echo "scripts"; exit 1; }
if [ "$NATIVE" = 1 ]; then
  rpm -qp --requires "$PKG" | grep -q "^libc.so.6("         || { echo "no computed libc.so.6 requirement"; exit 1; }
fi
rpm2archive - < "$PKG" > /tmp/ono.tgz && tar -xzOf /tmp/ono.tgz ./usr/bin/ono > /tmp/ono
machine="$(od -An -tx1 -j18 -N2 /tmp/ono | tr -s " " | sed "s/^ //;s/ $//")"
[ "$machine" = "$ELF_MACHINE" ] || { echo "binary e_machine is [$machine], expected [$ELF_MACHINE]"; exit 1; }
# §48.2 check: no private build paths are embedded
if grep -aoE "/(home|Users)/[A-Za-z0-9._-]+/" /tmp/ono | sort -u | head -5 | grep .; then
  echo "the binary embeds a private build path"; exit 1
fi
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
# §48.2 check: expected path /usr/bin/ono exists, with the right ownership and mode
[ -x /usr/bin/ono ] || { echo "/usr/bin/ono is not there or not executable"; exit 1; }
[ "$(stat -c "%U %G %a" /usr/bin/ono)" = "root root 755" ] \
  || { echo "/usr/bin/ono is $(stat -c "%U %G %a" /usr/bin/ono), expected root root 755"; exit 1; }
# §48.2 check: binary version equals release version
installed="$(ono --version | grep -oE "[0-9]+\.[0-9]+\.[0-9]+" | head -1)"
[ "$installed" = "$VERSION" ] \
  || { echo "the installed binary reports $installed and the release is $VERSION"; exit 1; }
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
mkdir -p /home/probe/.config/ono
echo "# the operator wrote this" > /home/probe/.config/ono/config.ono
chown -R probe:probe /home/probe/.config
echo "--- reinstall keeps one entry"
# §48.2 check: reinstall works
dnf --disablerepo="*" --assumeyes reinstall "$PKG" >/tmp/dnf.log 2>&1 || { cat /tmp/dnf.log; exit 1; }
[ "$(grep -cx /usr/bin/ono /etc/shells)" = 1 ] || { echo "reinstalling duplicated the /etc/shells entry"; exit 1; }
echo "--- remove"
dnf --disablerepo="*" --assumeyes remove ono >/tmp/dnf.log 2>&1 || { cat /tmp/dnf.log; exit 1; }
! grep -q /usr/bin/ono /etc/shells || { echo "/etc/shells still lists /usr/bin/ono after removal"; exit 1; }
! [ -e /usr/bin/ono ] || { echo "/usr/bin/ono survived removal"; exit 1; }
# §48.2 check: uninstall leaves user configuration
[ -f /home/probe/.config/ono/config.ono ] \
  || { echo "removing the package deleted the user configuration"; exit 1; }
' "$rpm"; then
    ok "installs, runs as root and as a login shell, registers and unregisters /usr/bin/ono"
  else
    fail "$rpm install/run/remove"
  fi
fi

# --- what was tested, by digest ------------------------------------------------------------
#
# §48.2 check: the checksum manifest matches the file, and §48.4/§62.6: the artifact tested here
# is the artifact later uploaded. When a manifest already exists beside the packages the two are
# compared now; either way the digests of what was installed are recorded outside dist/, so the
# publishing step can prove it is promoting these bytes rather than a rebuild of them (ADR-0532).

step "recording what was validated"
mkdir -p "$(dirname "$TESTED_RECORD")"
( cd dist && sha256sum "$deb" "$rpm" ) > "$TESTED_RECORD"
cat "$TESTED_RECORD"
if [[ -f dist/SHA256SUMS ]]; then
  if ( cd dist && grep -F -f <(cut -d" " -f1 "$OLDPWD/$TESTED_RECORD") SHA256SUMS >/dev/null ) \
     && ( cd dist && sha256sum --check --strict --ignore-missing SHA256SUMS >/dev/null ); then
    ok "dist/SHA256SUMS records the digests of the packages that were just validated"
  else
    fail "dist/SHA256SUMS does not match the packages that were validated (spec §48.2)"
  fi
else
  ok "recorded in $TESTED_RECORD; the checksum manifest is written by the publishing step and \
compared against this record there (spec §48.4)"
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
