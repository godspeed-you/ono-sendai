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

# --- Recorded after everything above, so none of the files above changes shape. ---
# A clone of a snapshot newer than the recovery point: `zfs rollback -R` would destroy it, and a
# clone of the recovery point itself is untouched (§13.6, Appendix D.5).
zfs snapshot tank/home@point
zfs snapshot tank/home@newer
zfs clone tank/home@newer tank/home-clone
emit list-clones-of-newer zfs list -H -p -t filesystem -o name,origin
emit get-clones-of-newer zfs get -H -p -o name,property,value clones tank/home@point tank/home@newer
emit destroy-newer-clone-held zfs destroy tank/home@newer
# Bookmarks whose source snapshots are gone, one older and one newer than the recovery point:
# only the newer one is in a rollback's way, and createtxg is what says which is which.
zfs snapshot tank/data/customer@gone-old
zfs bookmark tank/data/customer@gone-old tank/data/customer#orphan-old
zfs destroy tank/data/customer@gone-old
zfs snapshot tank/data/customer@point
zfs snapshot tank/data/customer@gone-new
zfs bookmark tank/data/customer@gone-new tank/data/customer#orphan-new
zfs destroy tank/data/customer@gone-new
emit list-bookmarks-orphaned zfs list -H -p -t bookmark -o name,creation,guid
emit get-createtxg-orphaned zfs get -H -p -o name,property,value createtxg tank/data/customer@point tank/data/customer#orphan-old tank/data/customer#orphan-new
emit rollback-refused-orphan zfs rollback tank/data/customer@point
# `written` for a dataset whose newest snapshot is the recovery point: nothing written since, then
# something (ADR-0829 decision 1). And `written@<snapshot>`, which is what a rollback past newer
# snapshots discards — plain `written` counts only since the newest one.
emit get-written-data-none zfs get -H -p -o name,property,value written tank/data
dd if=/dev/urandom of=/tank/data/after.bin bs=1M count=1 2>/dev/null
emit get-written-data zfs get -H -p -o name,property,value written tank/data
emit get-written-since zfs get -H -p -o name,property,value written@ono-a82f-20260909T194500Z rpool/ROOT/debian
# `zfs allow`, with a delegation and without one (§43.4).
zfs allow -u nobody destroy,mount,rollback,snapshot rpool/ROOT/debian
emit allow-delegated zfs allow rpool/ROOT/debian
emit allow-none zfs allow tank/home
# A degraded pool: a two-way mirror with one side offline (§13.1's pool health).
truncate -s 256M /pool/img3.zfs; truncate -s 256M /pool/img4.zfs
L3=$(losetup --find --show /pool/img3.zfs); L4=$(losetup --find --show /pool/img4.zfs)
zpool create -f degraded mirror "$L3" "$L4"
zpool offline degraded "$L4"
emit zpool-list-degraded zpool list -H -p -o name,size,alloc,free,capacity,fragmentation,health degraded
emit zpool-status-degraded zpool status degraded

zfs destroy -r tank/cloned 2>/dev/null || true
zfs destroy -r tank/home-clone 2>/dev/null || true
zpool destroy degraded; losetup -d "$L3"; losetup -d "$L4"; rm -f /pool/img3.zfs /pool/img4.zfs
zpool destroy rpool; zpool destroy tank
losetup -d "$L1"; losetup -d "$L2"; rm -f /pool/img1.zfs /pool/img2.zfs
echo "ZFS FIXTURES DONE"
