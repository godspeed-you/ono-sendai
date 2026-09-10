#!/usr/bin/env bash
# Builds a real BTRFS filesystem from nothing and records what the real tools print about it.
# Runs inside the disposable privileged container `scripts/fs-fixtures.sh` starts; it is not meant
# to be run on a host. Every `emit` writes one file: the command on the first line, the combined
# output, then `[exit <status>]`.
set -e
apt-get update -qq >/dev/null 2>&1
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq kmod btrfs-progs >/dev/null 2>&1
modprobe btrfs
OUT=/out
emit() { name=$1; shift; set +e; { echo "\$ $*"; "$@" 2>&1; echo "[exit $?]"; } > "$OUT/$name.txt"; set -e; }

truncate -s 2G /pool/img.btrfs
LOOP=$(losetup --find --show /pool/img.btrfs)
mkfs.btrfs -q -L onotest "$LOOP"
mkdir -p /mnt/top && mount "$LOOP" /mnt/top
btrfs subvolume create /mnt/top/@ >/dev/null
btrfs subvolume create /mnt/top/@home >/dev/null
btrfs subvolume create /mnt/top/@var >/dev/null
emit subvolume-create btrfs subvolume create /mnt/top/@snapshots
mkdir -p /mnt/top/@/etc/nginx /mnt/top/@/var
echo "worker_processes 4;" > /mnt/top/@/etc/nginx/nginx.conf
# a plain directory named like a subvolume — Appendix B.9's trap
mkdir -p /mnt/top/@/looks-like-a-subvol
# mount the subvolumes the way a distribution would
mkdir -p /mnt/root
mount -o subvol=/@ "$LOOP" /mnt/root
mkdir -p /mnt/root/home /mnt/root/var
mount -o subvol=/@home "$LOOP" /mnt/root/home
mount -o subvol=/@var "$LOOP" /mnt/root/var
# a NESTED subvolume inside @var — the §14.3 boundary
btrfs subvolume create /mnt/root/var/lib-app >/dev/null
mkdir -p /mnt/root/var/lib-app/state
dd if=/dev/urandom of=/mnt/root/var/lib-app/state/state.db bs=1M count=4 2>/dev/null
mkdir -p /mnt/root/var/plain-dir && echo x > /mnt/root/var/plain-dir/f

emit version btrfs --version
emit fs-show btrfs filesystem show
emit fs-show-mount btrfs filesystem show /mnt/root
emit fs-usage btrfs filesystem usage /mnt/root
emit fs-df btrfs filesystem df /mnt/root
emit subvol-list btrfs subvolume list -a -p -u -q -R /mnt/top
emit subvol-list-root btrfs subvolume list -a -p -u -q -R /mnt/root
emit subvol-show-root btrfs subvolume show /mnt/root
emit subvol-show-var btrfs subvolume show /mnt/root/var
emit subvol-show-nested btrfs subvolume show /mnt/root/var/lib-app
emit subvol-show-plaindir btrfs subvolume show /mnt/root/var/plain-dir
emit subvol-show-lookslike btrfs subvolume show /mnt/root/looks-like-a-subvol
emit get-default btrfs subvolume get-default /mnt/top
emit mountinfo grep btrfs /proc/self/mountinfo

# read-only snapshots, as the provider would take them
btrfs subvolume snapshot -r /mnt/root /mnt/top/@snapshots/ono-a82f-root >/dev/null
btrfs subvolume snapshot -r /mnt/root/var /mnt/top/@snapshots/ono-a82f-var >/dev/null
emit snapshot-create btrfs subvolume snapshot -r /mnt/root/home /mnt/top/@snapshots/ono-a82f-home
emit subvol-list-after btrfs subvolume list -a -p -u -q -R -s /mnt/top
emit subvol-show-snapshot btrfs subvolume show /mnt/top/@snapshots/ono-a82f-root
emit snapshot-ro-flag btrfs property get /mnt/top/@snapshots/ono-a82f-root ro

# what a snapshot of the parent does NOT contain: the nested subvolume is an empty directory
emit nested-in-snapshot ls -la /mnt/top/@snapshots/ono-a82f-var/lib-app
emit nested-live ls -la /mnt/root/var/lib-app
emit plaindir-in-snapshot ls -la /mnt/top/@snapshots/ono-a82f-var/plain-dir

echo "worker_processes 8;" > /mnt/root/etc/nginx/nginx.conf
emit live-file cat /mnt/root/etc/nginx/nginx.conf
emit snapshot-file cat /mnt/top/@snapshots/ono-a82f-root/etc/nginx/nginx.conf

# failure shapes
emit snapshot-exists btrfs subvolume snapshot -r /mnt/root /mnt/top/@snapshots/ono-a82f-root
emit snapshot-missing-source btrfs subvolume snapshot -r /mnt/root/nope /mnt/top/@snapshots/x
emit delete-readonly btrfs subvolume delete /mnt/top/@snapshots/ono-a82f-root
emit subvol-show-missing btrfs subvolume show /mnt/root/nothing-here
emit unprivileged-list su -s /bin/sh -c "btrfs subvolume list /mnt/root" nobody
emit unprivileged-snapshot su -s /bin/sh -c "btrfs subvolume snapshot -r /mnt/root /mnt/top/@snapshots/nope" nobody
emit set-default-refused btrfs subvolume set-default 999999 /mnt/top
emit quota-disabled btrfs qgroup show /mnt/top

umount /mnt/root/var /mnt/root/home /mnt/root /mnt/top || true
losetup -d "$LOOP"; rm -f /pool/img.btrfs
echo "BTRFS FIXTURES DONE"
