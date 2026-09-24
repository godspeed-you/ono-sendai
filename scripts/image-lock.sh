# shellcheck shell=bash
# Sourced by scripts/acceptance.sh and scripts/package-check.sh: the lock that keeps a run from
# removing an image another run of the same checkout is still using (ADR-0901, ADR-0919).
#
# Image tags belong to the container daemon, which every user of the machine shares; a `sudo`
# run and a user run of one checkout build the same tag. So the lock lives where every run
# against this checkout agrees: in the repository's common git directory when the checkout is the
# top of a repository — shared by its worktrees, and by root and the owner alike — and otherwise
# in the checkout itself. Never in a per-user runtime directory, and never in a shared /tmp.
#
# The file is created once, exclusively (noclobber: never through a symlink someone planted), and
# every run opens it read-only: flock needs no write access, and a file another user created is
# still one this user can lock.
#
# image_lock_open <tool>   opens the lock and sets `image_lock_fd` and `image_lock_file`
image_lock_open() {
  local checkout top common
  checkout="$(pwd -P)"
  top="$(git rev-parse --show-toplevel 2>/dev/null || true)"
  if [[ -n "$top" && "$(cd "$top" && pwd -P)" == "$checkout" ]] \
      && common="$(git rev-parse --git-common-dir 2>/dev/null)" && [[ -d "$common" ]]; then
    image_lock_file="$(cd "$common" && pwd -P)/ono-$1.lock"
  else
    image_lock_file="$checkout/.ono-$1.lock"
  fi
  if [[ ! -e "$image_lock_file" ]]; then
    ( set -o noclobber; : > "$image_lock_file" ) 2>/dev/null || true
  fi
  if [[ ! -f "$image_lock_file" || -L "$image_lock_file" ]]; then
    echo "$1: cannot use $image_lock_file as the image lock (not a regular file)" >&2
    exit 1
  fi
  # shellcheck disable=SC2034  # read by the sourcing script
  exec {image_lock_fd}<"$image_lock_file"
}
