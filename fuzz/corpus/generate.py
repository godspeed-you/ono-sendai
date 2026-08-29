#!/usr/bin/env python3
"""Builds the seed corpus of the §35.6 fuzz targets.

The seeds are the shapes each decoder actually meets: the netlink messages the
kernel sends, the frames the remote agent sends, the envelopes a plugin sends,
the documents the codecs read, the procfs lines the kernel writes, and the
command lines a user types. They are the same shapes the existing property and
robustness suites build in Rust; here they are written to files so a mutator has
something to start from.
"""
import os, struct, sys, pathlib

ROOT = pathlib.Path(sys.argv[1])

def put(target, name, data):
    d = ROOT / "corpus" / target
    d.mkdir(parents=True, exist_ok=True)
    if isinstance(data, str):
        data = data.encode()
    (d / name).write_bytes(data)

# ---------------------------------------------------------------- parser
parser_seeds = {
    "pipeline": "get process | where cpu > 20 and memory >= 512MiB | sort cpu desc | take 10\n",
    "redirects": "ls -la > out.txt 2> err.txt; cat < in.txt | head -3 2>&1\n",
    "blocks": "fn f(a, b) { if a > b { return a } else { return b } }\n",
    "loops": "for x in [1, 2, 3] { while true { break } }\n",
    "literals": 'let v = {a: 20, b: 512MiB, c: 7d, d: 95%, e: 0x1f, f: 1.5, g: "a\\q", h: \'raw\', i: /re/i}\n',
    "try-catch": "try { unmount filesystem / } catch e { $e | to json }\n",
    "selectors": "get process 1234 | select name.first ?. pid | where @-1 => $x -> $x\n",
    "unicode": "echo \U0001F600 é \U0001D11E \\\n",
    "unclosed": "(" * 512,
    "nested-blocks": "if true " + "{ if true " * 64 + "}" * 65 + "\n",
    "empty": "",
    "keywords": "let fn if else for while match try catch return break continue use\n",
    "spatial": "look; near socket; enter 1; follow parent; find place sleep | count\n",
    "plugin-call": "load plugin dev.example.echo --grant clock.read; echo:clock | to json\n",
}
for name, text in parser_seeds.items():
    put("parser", name + ".ono", text)

# ------------------------------------------------------------ serializers
# The first byte selects the codec: 0 json, 1 yaml, 2 csv, 3 raw bytes.
def codec(selector, text):
    return bytes([selector]) + text.encode()

put("serializers", "json-record.bin", codec(0, '{"$record": {"schema": "ono.process/1", "fields": {"pid": {"$int": 1}, "name": "init"}}}'))
put("serializers", "json-scalars.bin", codec(0, '[null, true, -1, 1.5, "\\u0000", 1e999999, 99999999999999999999999999]'))
put("serializers", "json-tags.bin", codec(0, '{"$bytesize": "512MiB", "$duration": "7d", "$error": {"code": "Ono-Sendai-E0101"}}'))
put("serializers", "json-nested.bin", codec(0, "[" * 32 + "]" * 32))
put("serializers", "yaml-document.bin", codec(1, "---\n- &a one\n- *a\n- !!str 2\n- |\n  block\n- >\n  folded\n"))
put("serializers", "yaml-map.bin", codec(1, "a: 1\nb:\n  c: [1, 2]\n  d: {e: f}\n"))
put("serializers", "yaml-directive.bin", codec(1, "%YAML 1.2\n---\n﻿key: value\n"))
put("serializers", "csv-table.bin", codec(2, 'pid,name,cpu\n1,"init, the first",0.5\n2,kthreadd,\n'))
put("serializers", "csv-quotes.bin", codec(2, 'a,b\n"he said ""hi""","line\nbreak"\n'))
put("serializers", "raw-bytes.bin", codec(3, "\x00\x01\x7f\x80\xff é\n"))

# --------------------------------------------------------- remote protocol
# frame: version, kind, 2 reserved, stream (be32), length (be32), payload
def frame(kind, stream, payload):
    return bytes([1, kind, 0, 0]) + struct.pack(">I", stream) + struct.pack(">I", len(payload)) + payload

hello = b'{"protocol":1,"agent":"ono","capabilities":[]}'
value = b'{"$record":{"schema":"ono.test.remote/1","fields":{"name":"one"}}}'
put("remote-protocol", "hello.bin", frame(0, 0, hello))
put("remote-protocol", "two-frames.bin", frame(0, 0, hello) + frame(1, 1, value))
put("remote-protocol", "empty-payload.bin", frame(2, 7, b""))
put("remote-protocol", "credit.bin", frame(3, 1, struct.pack(">I", 32)))
put("remote-protocol", "truncated.bin", frame(1, 1, value)[:14])
put("remote-protocol", "impossible-length.bin", bytes([1, 1, 0, 0]) + struct.pack(">I", 1) + struct.pack(">I", 0xFFFFFFFF))
put("remote-protocol", "deep-value.bin", frame(1, 1, b"[" * 64 + b"]" * 64))
for kind in range(0, 10):
    put("remote-protocol", f"kind-{kind:02d}.bin", frame(kind, kind, value))

# --------------------------------------------------------- plugin protocol
# frame: 4-byte big-endian length + JSON envelope
def kframe(payload):
    return struct.pack(">I", len(payload)) + payload

hello = b'{"Hello":{"kuang_api":"11.1","package":"dev.example.echo","version":"0.1.0"}}'
request = b'{"Request":{"seq":1,"method":"lifecycle.init","params":{}}}'
response = b'{"Response":{"seq":1,"result":{"ok":true},"error":null}}'
put("plugin-protocol", "hello.bin", kframe(hello))
put("plugin-protocol", "request.bin", kframe(request))
put("plugin-protocol", "response.bin", kframe(response))
put("plugin-protocol", "stream.bin", kframe(hello) + kframe(request) + kframe(response))
put("plugin-protocol", "oversized.bin", struct.pack(">I", 0xFFFFFFFF) + b"{}")
put("plugin-protocol", "bare-envelope.bin", request)

MANIFEST = """format: kuang-package/1
package:
  id: dev.example.users
  name: users
  version: 0.1.0
  description: Accounts from the name service, as User records.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64]
roles: [adapter]
capabilities:
  optional:
    - process.exec:
        executables: [getent]
        argv_policy: declared-invocations-only
network:
  outbound: none
contributions:
  adapters: [adapters.yaml]
"""
put("plugin-protocol", "manifest.yaml", MANIFEST)
put("plugin-protocol", "manifest-runtime.yaml", """format: kuang-package/1
package:
  id: dev.example.echo
  name: echo
  version: 0.1.0
  description: Emits what it is asked to emit.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64]
runtime:
  kind: native-process
  entry: runtime/echo
  memory_max: 64MiB
  cpu_budget: interactive
  startup: lazy
roles: [provider]
capabilities:
  optional:
    - clock.read
network:
  outbound: none
""")
put("plugin-protocol", "signature.yaml", """format: kuang-signature/1
algorithm: ed25519
key: ed25519:""" + "ab" * 32 + """
signed:
  package: dev.example.users
  version: 0.1.0
  publisher: dev.example
  files:
  - path: adapters.yaml
    sha256: """ + "cd" * 32 + """
  - path: manifest.yaml
    sha256: """ + "ef" * 32 + """
signature: """ + "12" * 64 + "\n")

# ------------------------------------------------------- system decoders
NATIVE = "<"  # every supported platform is little-endian (linux-amd64, linux-arm64)

def align4(n):
    return (n + 3) // 4 * 4

def nlmsg(kind, payload, flags=2):
    length = 16 + len(payload)
    out = struct.pack(NATIVE + "IHHII", length, kind, flags, 1, 0) + payload
    return out + b"\0" * (align4(length) - length)

def attr(kind, payload):
    length = 4 + len(payload)
    out = struct.pack(NATIVE + "HH", length, kind) + payload
    return out + b"\0" * (align4(length) - length)

def ifinfomsg(family, index, flags):
    return struct.pack(NATIVE + "BBHiII", family, 0, 0, index, flags, 0xFFFFFFFF)

def ifaddrmsg(family, prefixlen, index):
    return struct.pack(NATIVE + "BBBBI", family, prefixlen, 0, 0, index)

def rtmsg(family, dst_len, table, protocol, scope, rtype, flags):
    return struct.pack(NATIVE + "BBBBBBBBI", family, dst_len, 0, 0, table, protocol, scope, rtype, flags)

def ndmsg(family, index, state, flags, ntype):
    return struct.pack(NATIVE + "BBHiHBB", family, 0, 0, index, state, flags, ntype)

def sockid(local, remote, iface):
    (laddr, lport), (raddr, rport) = local, remote
    out = struct.pack(">HH", lport, rport)
    out += bytes(laddr) + b"\0" * (16 - len(laddr))
    out += bytes(raddr) + b"\0" * (16 - len(raddr))
    out += struct.pack(NATIVE + "I", iface)
    out += struct.pack(NATIVE + "II", 0, 0)
    return out

def inet_diag_msg(family, state, sid, timer, retrans, uid, inode):
    return (struct.pack(NATIVE + "BBBB", family, state, timer, retrans) + sid
            + struct.pack(NATIVE + "IIIII", 0, 0, uid, inode, 0))

def unix_diag_msg(kind, state, inode, cookie):
    return struct.pack(NATIVE + "BBBBIII", 1, kind, state, 0, inode, cookie & 0xFFFFFFFF, cookie >> 32)

link = nlmsg(16, ifinfomsg(2, 1, 0x1 | 0x40) + attr(3, b"eth0\0") + attr(4, struct.pack(NATIVE + "I", 1500)) + attr(16, b"\x06") + attr(1, bytes([1,2,3,4,5,6])))
address = nlmsg(20, ifaddrmsg(2, 24, 2) + attr(2, bytes([10,0,0,1])))
route = nlmsg(24, rtmsg(2, 24, 254, 2, 253, 1, 0) + attr(1, bytes([10,0,0,0])) + attr(4, struct.pack(NATIVE + "I", 2)))
neighbor = nlmsg(28, ndmsg(2, 2, 0x02, 0, 1) + attr(1, bytes([10,0,0,1])) + attr(2, bytes([1,2,3,4,5,6])))
tcp = nlmsg(20, inet_diag_msg(2, 1, sockid((bytes([10,0,0,2]), 51000), (bytes([10,0,0,1]), 443), 7), 0, 0, 1000, 9001))
unix = nlmsg(20, unix_diag_msg(1, 10, 5555, 3) + attr(0, b"/run/ono\0"))
done = nlmsg(3, struct.pack(NATIVE + "i", 0))
err = nlmsg(2, struct.pack(NATIVE + "i", -13) + b"\0" * 16)

put("system-decoders", "netlink-link.bin", link)
put("system-decoders", "netlink-address.bin", address)
put("system-decoders", "netlink-route.bin", route)
put("system-decoders", "netlink-neighbor.bin", neighbor)
put("system-decoders", "netlink-tcp.bin", tcp)
put("system-decoders", "netlink-unix.bin", unix)
put("system-decoders", "netlink-dump.bin", link + address + route + neighbor + tcp + unix + done)
put("system-decoders", "netlink-error.bin", err)
put("system-decoders", "netlink-truncated.bin", link[:len(link) // 2])
put("system-decoders", "netlink-claiming-more.bin", struct.pack(NATIVE + "IHHII", 0xFFFFFFFF, 16, 2, 1, 0) + ifinfomsg(2, 1, 1))

put("system-decoders", "procfs-stat.txt", "4419 (bash) S 1 4419 4419 34816 4419 4194304 1234 0 0 0 12 34 0 0 20 0 1 0 987654 12582912 512 18446744073709551615\n")
put("system-decoders", "procfs-stat-weird-comm.txt", "4419 ((weird) name) S 1 4419 4419 0 -1 4194304 0 0 0 0 1 2 0 0 20 0 3 0 100 4096 64\n")
put("system-decoders", "procfs-status.txt", "Name:\tbash\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\n")
put("system-decoders", "procfs-cmdline.bin", b"/usr/bin/ono\0-c\0get process | to json\0")
put("system-decoders", "procfs-cgroup.txt", "0::/user.slice/user-1000.slice/session-3.scope\n")
put("system-decoders", "mountinfo.txt",
    "25 0 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw\n"
    "26 25 0:22 / /proc rw,nosuid,nodev,noexec,relatime - proc proc rw\n"
    "27 25 0:6 / /mnt/with\\040space rw master:5 - tmpfs tmpfs rw\n")
put("system-decoders", "fstab.txt",
    "# a comment\nUUID=1234-5678 /            ext4 defaults 0 1\n"
    "/dev/sda1      /boot\\040efi  vfat umask=0077 0 2\ntmpfs /tmp tmpfs rw,nosuid 0 0\n")

total = sum(1 for _ in (ROOT / "corpus").rglob("*") if _.is_file())
print(f"{total} seed files under {ROOT / 'corpus'}")
