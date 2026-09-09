#!/usr/bin/env bash
# Builds a real ZFS filesystem from nothing and records what the real tools print about it.
# Runs inside the disposable privileged container `scripts/fs-fixtures.sh` starts; it is not meant
# to be run on a host. Every `emit` writes one file: the command on the first line, the combined
# output, then `[exit <status>]`.
set -e
apt-get update -qq >/dev/null 2>&1
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq kmod zfsutils-linux >/dev/null 2>&1
modprobe zfs
OUT=/out
truncate -s 1G /pool/img1.zfs; truncate -s 1G /pool/img2.zfs
L1=$(losetup --find --show /pool/img1.zfs); L2=$(losetup --find --show /pool/img2.zfs)
zpool create -f rpool "$L1"
zpool create -f tank "$L2"
zfs create -o mountpoint=/altroot rpool/ROOT
zfs create rpool/ROOT/debian
zfs create tank/data
zfs create tank/data/customer
zfs create tank/home
dd if=/dev/urandom of=/tank/data/customer/db.bin bs=1M count=8 2>/dev/null
mkdir -p /altroot/etc/nginx && echo "worker_processes 4;" > /altroot/etc/nginx/nginx.conf

emit() { name=$1; shift; set +e; { echo "\$ $*"; "$@" 2>&1; echo "[exit $?]"; } > "$OUT/$name.txt"; set -e; }

emit version zfs version
emit zpool-version zpool version
emit list-filesystems zfs list -H -p -t filesystem -o name,mountpoint,mounted,type,used,available,referenced,origin,canmount
emit list-all zfs list -H -p -t all -o name,type,creation,used,referenced,mountpoint
emit get-mounted zfs get -H -p -o name,property,value mounted,mountpoint,canmount,readonly,origin
emit zpool-list zpool list -H -p -o name,size,alloc,free,capacity,fragmentation,health
emit zpool-status zpool status
emit zpool-get zpool get -H -p -o name,property,value capacity,free,size,health,fragmentation

zfs snapshot rpool/ROOT/debian@ono-a82f-20260909T194500Z
zfs snapshot -r tank/data@ono-b91c-20260909T194501Z
echo "worker_processes 8;" > /altroot/etc/nginx/nginx.conf
zfs snapshot rpool/ROOT/debian@later-1
emit list-snapshots zfs list -H -p -t snapshot -o name,creation,used,referenced,guid,defer_destroy
emit list-snapshots-sorted zfs list -H -p -t snapshot -o name,creation -s creation
zfs bookmark rpool/ROOT/debian@later-1 rpool/ROOT/debian#book-1
emit list-bookmarks zfs list -H -p -t bookmark -o name,creation,guid
zfs clone tank/data@ono-b91c-20260909T194501Z tank/cloned
emit list-clones zfs list -H -p -t filesystem -o name,origin
emit get-clones zfs get -H -p -o name,property,value clones rpool/ROOT/debian@ono-a82f-20260909T194500Z tank/data@ono-b91c-20260909T194501Z
emit mountinfo grep -E ' (zfs|ext4) ' /proc/self/mountinfo

# rollback that must be refused because a newer snapshot exists
emit rollback-refused zfs rollback rpool/ROOT/debian@ono-a82f-20260909T194500Z
# snapshot name that already exists
emit snapshot-exists zfs snapshot rpool/ROOT/debian@later-1
# a dataset that does not exist
emit list-missing zfs list -H -p -t filesystem rpool/nope
emit snapshot-missing zfs snapshot rpool/nope@x
# destroy with clone present
emit destroy-clone-held zfs destroy tank/data@ono-b91c-20260909T194501Z
# the .zfs snapshot directory view used for selective restore
zfs set snapdir=visible rpool/ROOT/debian
emit snapdir ls -la /altroot/.zfs/snapshot/
emit snapshot-file cat /altroot/.zfs/snapshot/ono-a82f-20260909T194500Z/etc/nginx/nginx.conf
emit live-file cat /altroot/etc/nginx/nginx.conf
emit get-written zfs get -H -p -o name,property,value written rpool/ROOT/debian
emit get-used-by-snapshots zfs get -H -p -o name,property,value usedbysnapshots,usedbydataset rpool/ROOT/debian
# permission failure shape, as an unprivileged user
emit unprivileged-snapshot su -s /bin/sh -c "zfs snapshot rpool/ROOT/debian@nope" nobody
emit unprivileged-list su -s /bin/sh -c "zfs list -H -o name" nobody

zfs destroy -r tank/cloned 2>/dev/null || true
zpool destroy rpool; zpool destroy tank
losetup -d "$L1"; losetup -d "$L2"; rm -f /pool/img1.zfs /pool/img2.zfs
echo "ZFS FIXTURES DONE"
