#!/usr/bin/env python3
"""Record a real `ono` session into an asciinema v2 cast.

A tape is a small text file, one directive per line:

    title <text>            caption for the rendered GIF (read by render.py)
    size <cols> <rows>      terminal geometry (before the shell starts)
    env  <NAME> <value>     environment for the shell
    cd   <dir>              working directory for the shell
    wait <seconds>          pause
    run  <pipeline>         type the pipeline, press Enter, wait for the prompt
    type <text>             type text, no Enter
    key  <enter|ctrl-c|up|down|tab|esc>
    idle <seconds>          keep recording while the shell paints (watch, map, view)

Everything the cast contains was written by the binary. Nothing is synthesised.
"""

from __future__ import annotations

import argparse
import fcntl
import json
import os
import pty
import re
import select
import shlex
import struct
import sys
import termios
import time

KEYS = {
    "enter": "\r",
    "ctrl-c": "\x03",
    "ctrl-d": "\x04",
    "up": "\x1b[A",
    "down": "\x1b[B",
    "left": "\x1b[D",
    "right": "\x1b[C",
    "tab": "\t",
    "esc": "\x1b",
    "space": " ",
    "q": "q",
}


class Session:
    def __init__(self, command, cols, rows, env, cwd):
        self.cols, self.rows = cols, rows
        self.events: list[tuple[float, str, str]] = []
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.environ.update(env)
            os.environ["TERM"] = env.get("TERM", "xterm-256color")
            os.environ["COLUMNS"], os.environ["LINES"] = str(cols), str(rows)
            if cwd:
                os.chdir(cwd)
            os.execvp(command[0], command)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.t0 = time.time()

    def _emit(self, data: bytes):
        if data:
            self.events.append((time.time() - self.t0, "o", data.decode("utf-8", "replace")))

    def pump(self, seconds: float):
        end = time.time() + seconds
        while time.time() < end:
            r, _, _ = select.select([self.fd], [], [], min(0.05, max(0.0, end - time.time())))
            if r:
                try:
                    self._emit(os.read(self.fd, 65536))
                except OSError:
                    return

    def pump_until_quiet(self, quiet=0.45, timeout=25.0):
        end, last = time.time() + timeout, time.time()
        while time.time() < end:
            r, _, _ = select.select([self.fd], [], [], 0.05)
            if r:
                try:
                    data = os.read(self.fd, 65536)
                except OSError:
                    return
                if not data:
                    return
                self._emit(data)
                last = time.time()
            elif time.time() - last > quiet:
                return

    def send(self, text: str, per_char=0.0):
        for ch in text:
            os.write(self.fd, ch.encode())
            self.pump(per_char if per_char else 0.01)

    def close(self):
        try:
            os.write(self.fd, b"\x03exit\r")
            self.pump(0.6)
            os.close(self.fd)
        except OSError:
            pass
        try:
            os.waitpid(self.pid, os.WNOHANG)
        except ChildProcessError:
            pass


def parse_tape(path):
    header, steps = {"cols": 96, "rows": 28, "env": {}, "cwd": None, "title": None}, []
    for raw in open(path, encoding="utf-8"):
        line = raw.rstrip("\n")
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        verb, _, rest = line.strip().partition(" ")
        rest = rest.strip()
        if verb == "title":
            header["title"] = rest
        elif verb == "size":
            header["cols"], header["rows"] = (int(x) for x in rest.split())
        elif verb == "env":
            name, _, value = rest.partition(" ")
            header["env"][name] = value
        elif verb == "cd":
            header["cwd"] = os.path.expanduser(rest)
        else:
            steps.append((verb, rest))
    return header, steps


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tape")
    ap.add_argument("-o", "--out", required=True)
    ap.add_argument("--shell", default="target/release/ono",
                    help="path to the ono binary to record")
    ap.add_argument("--exec", default=None,
                    help="record this command line instead (e.g. a docker run invocation)")
    ap.add_argument("--typing", type=float, default=0.035, help="seconds per keystroke")
    args = ap.parse_args()

    header, steps = parse_tape(args.tape)
    if args.exec:
        command, shell = shlex.split(args.exec), args.exec
    else:
        shell = os.path.abspath(args.shell)
        if not os.access(shell, os.X_OK):
            sys.exit(f"no ono binary at {shell} — cargo build --release -p ono-cli")
        command = [shell]

    env = {"ONO_DEMO": "1", **header["env"]}
    session = Session(command, header["cols"], header["rows"], env, header["cwd"])
    session.pump_until_quiet(quiet=0.6, timeout=10)

    for verb, rest in steps:
        if verb == "wait" or verb == "idle":
            session.pump(float(rest))
        elif verb == "run":
            session.send(rest, per_char=args.typing)
            session.pump(0.35)
            session.send("\r")
            session.pump_until_quiet()
        elif verb == "type":
            session.send(rest, per_char=args.typing)
        elif verb == "key":
            for name in rest.split():
                session.send(KEYS[name])
                session.pump(0.25)
        else:
            sys.exit(f"unknown tape directive: {verb}")

    session.close()

    with open(args.out, "w", encoding="utf-8") as fh:
        meta = {
            "version": 2,
            "width": header["cols"],
            "height": header["rows"],
            "timestamp": int(time.time()),
            "env": {"TERM": "xterm-256color", "SHELL": shell},
        }
        fh.write(json.dumps(meta) + "\n")
        for t, kind, data in session.events:
            fh.write(json.dumps([round(t, 4), kind, data]) + "\n")
    print(f"{args.out}: {len(session.events)} chunks, {session.events[-1][0]:.1f}s")


if __name__ == "__main__":
    main()
