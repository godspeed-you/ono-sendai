#!/usr/bin/env python3
"""Render an asciinema v2 cast into an animated GIF.

A small VT emulator (SGR, cursor motion, erase, alternate screen) drives a
character grid; every distinct grid state becomes one GIF frame whose duration
is the time the terminal actually spent in it.
"""

from __future__ import annotations

import argparse
import json
import re
import sys

from PIL import Image, ImageDraw, ImageFont

FONT_CANDIDATES = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
]
BOLD_CANDIDATES = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Bold.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansMono-Bold.ttf",
]

BG = (10, 12, 16)
FG = (198, 208, 219)
CURSOR = (120, 200, 220)

ANSI16 = [
    (30, 34, 40), (224, 108, 117), (140, 190, 140), (208, 184, 128),
    (110, 170, 220), (190, 150, 210), (110, 190, 200), (198, 208, 219),
    (95, 105, 118), (230, 130, 140), (160, 210, 160), (226, 205, 150),
    (135, 190, 235), (205, 170, 225), (140, 210, 220), (235, 242, 250),
]


def xterm256(i: int):
    if i < 16:
        return ANSI16[i]
    if i < 232:
        i -= 16
        steps = [0, 95, 135, 175, 215, 255]
        return (steps[i // 36 % 6], steps[i // 6 % 6], steps[i % 6])
    v = 8 + (i - 232) * 10
    return (v, v, v)


class Cell:
    __slots__ = ("ch", "fg", "bg", "bold", "dim", "italic", "underline")

    def __init__(self):
        self.ch, self.fg, self.bg = " ", None, None
        self.bold = self.dim = self.italic = self.underline = False

    def key(self):
        return (self.ch, self.fg, self.bg, self.bold, self.dim, self.italic, self.underline)


class Screen:
    def __init__(self, cols, rows):
        self.cols, self.rows = cols, rows
        self.grid = [[Cell() for _ in range(cols)] for _ in range(rows)]
        self.x = self.y = 0
        self.saved = (0, 0)
        self.cursor_visible = True
        self.reset_attrs()

    def reset_attrs(self):
        self.fg = self.bg = None
        self.bold = self.dim = self.italic = self.underline = self.reverse = False

    def blank_row(self):
        return [Cell() for _ in range(self.cols)]

    def put(self, ch):
        if self.x >= self.cols:
            self.x = 0
            self.newline()
        c = self.grid[self.y][self.x]
        c.ch = ch
        fg, bg = self.fg, self.bg
        if self.reverse:
            fg, bg = bg if bg is not None else BG, fg if fg is not None else FG
        c.fg, c.bg = fg, bg
        c.bold, c.dim, c.italic, c.underline = self.bold, self.dim, self.italic, self.underline
        self.x += 1

    def newline(self):
        self.y += 1
        if self.y >= self.rows:
            self.grid.pop(0)
            self.grid.append(self.blank_row())
            self.y = self.rows - 1

    def erase(self, cells):
        for c in cells:
            c.ch, c.fg, c.bg = " ", None, self.bg if self.reverse is False else None
            c.bold = c.dim = c.italic = c.underline = False

    def snapshot(self):
        return tuple(tuple(c.key() for c in row) for row in self.grid) + (
            (self.x, self.y, self.cursor_visible),
        )


CSI_RE = re.compile(r"\x1b\[([0-9;?]*)([@-~])")
OSC_RE = re.compile(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")


class Terminal:
    def __init__(self, cols, rows):
        self.main = Screen(cols, rows)
        self.alt = Screen(cols, rows)
        self.screen = self.main
        self.cols, self.rows = cols, rows

    def feed(self, data: str):
        i, n = 0, len(data)
        s = self.screen
        while i < n:
            ch = data[i]
            if ch == "\x1b":
                m = CSI_RE.match(data, i)
                if m:
                    self.csi(m.group(1), m.group(2))
                    s = self.screen
                    i = m.end()
                    continue
                m = OSC_RE.match(data, i)
                if m:
                    i = m.end()
                    continue
                if data.startswith("\x1b]", i):        # unterminated OSC
                    i = n
                    continue
                if data.startswith("\x1b(", i) or data.startswith("\x1b)", i):
                    i += 3
                    continue
                if data.startswith("\x1b=", i) or data.startswith("\x1b>", i):
                    i += 2
                    continue
                if data.startswith("\x1b7", i):
                    s.saved = (s.x, s.y); i += 2; continue
                if data.startswith("\x1b8", i):
                    s.x, s.y = s.saved; i += 2; continue
                if data.startswith("\x1bM", i):
                    s.y = max(0, s.y - 1); i += 2; continue
                i += 1
                continue
            if ch == "\r":
                s.x = 0
            elif ch == "\n":
                s.newline()
            elif ch == "\b":
                s.x = max(0, s.x - 1)
            elif ch == "\t":
                s.x = min(self.cols - 1, (s.x // 8 + 1) * 8)
            elif ch in ("\x07", "\x00"):
                pass
            else:
                s.put(ch)
            i += 1

    def csi(self, params, final):
        s = self.screen
        private = params.startswith("?")
        raw = params[1:] if private else params
        nums = [int(p) if p else 0 for p in raw.split(";")] if raw else []
        one = nums[0] if nums else 0

        if private and final in "hl":
            for p in nums:
                if p == 25:
                    s.cursor_visible = final == "h"
                elif p in (1049, 47, 1047):
                    if final == "h":
                        self.alt = Screen(self.cols, self.rows)
                        self.screen = self.alt
                    else:
                        self.screen = self.main
            return
        if final == "m":
            self.sgr(nums or [0])
        elif final == "A":
            s.y = max(0, s.y - max(1, one))
        elif final == "B":
            s.y = min(self.rows - 1, s.y + max(1, one))
        elif final == "C":
            s.x = min(self.cols - 1, s.x + max(1, one))
        elif final == "D":
            s.x = max(0, s.x - max(1, one))
        elif final == "E":
            s.y = min(self.rows - 1, s.y + max(1, one)); s.x = 0
        elif final == "F":
            s.y = max(0, s.y - max(1, one)); s.x = 0
        elif final == "G" or final == "`":
            s.x = min(self.cols - 1, max(0, one - 1 if one else 0))
        elif final in "Hf":
            row = (nums[0] if len(nums) > 0 and nums[0] else 1) - 1
            col = (nums[1] if len(nums) > 1 and nums[1] else 1) - 1
            s.y, s.x = min(self.rows - 1, max(0, row)), min(self.cols - 1, max(0, col))
        elif final == "J":
            if one == 0:
                s.erase(s.grid[s.y][s.x:])
                for r in s.grid[s.y + 1:]:
                    s.erase(r)
            elif one == 1:
                s.erase(s.grid[s.y][: s.x + 1])
                for r in s.grid[: s.y]:
                    s.erase(r)
            else:
                for r in s.grid:
                    s.erase(r)
        elif final == "K":
            if one == 0:
                s.erase(s.grid[s.y][s.x:])
            elif one == 1:
                s.erase(s.grid[s.y][: s.x + 1])
            else:
                s.erase(s.grid[s.y])
        elif final == "X":
            s.erase(s.grid[s.y][s.x: s.x + max(1, one)])
        elif final == "L":
            for _ in range(max(1, one)):
                s.grid.insert(s.y, s.blank_row()); s.grid.pop()
        elif final == "M":
            for _ in range(max(1, one)):
                s.grid.pop(s.y); s.grid.append(s.blank_row())
        elif final == "P":
            row = s.grid[s.y]
            del row[s.x: s.x + max(1, one)]
            row.extend(Cell() for _ in range(self.cols - len(row)))
        elif final == "S":
            for _ in range(max(1, one)):
                s.grid.pop(0); s.grid.append(s.blank_row())
        elif final == "T":
            for _ in range(max(1, one)):
                s.grid.insert(0, s.blank_row()); s.grid.pop()
        elif final == "d":
            s.y = min(self.rows - 1, max(0, (one or 1) - 1))

    def sgr(self, nums):
        s, i = self.screen, 0
        while i < len(nums):
            p = nums[i]
            if p == 0:
                s.reset_attrs()
            elif p == 1:
                s.bold = True
            elif p == 2:
                s.dim = True
            elif p == 3:
                s.italic = True
            elif p == 4:
                s.underline = True
            elif p == 7:
                s.reverse = True
            elif p == 22:
                s.bold = s.dim = False
            elif p == 23:
                s.italic = False
            elif p == 24:
                s.underline = False
            elif p == 27:
                s.reverse = False
            elif 30 <= p <= 37:
                s.fg = ANSI16[p - 30]
            elif p == 39:
                s.fg = None
            elif 40 <= p <= 47:
                s.bg = ANSI16[p - 40]
            elif p == 49:
                s.bg = None
            elif 90 <= p <= 97:
                s.fg = ANSI16[p - 90 + 8]
            elif 100 <= p <= 107:
                s.bg = ANSI16[p - 100 + 8]
            elif p in (38, 48):
                target = "fg" if p == 38 else "bg"
                if i + 1 < len(nums) and nums[i + 1] == 5:
                    setattr(s, target, xterm256(nums[i + 2] if i + 2 < len(nums) else 0))
                    i += 2
                elif i + 1 < len(nums) and nums[i + 1] == 2:
                    setattr(s, target, tuple(nums[i + 2: i + 5]) or FG)
                    i += 4
            i += 1


def load_font(paths, size):
    for p in paths:
        try:
            return ImageFont.truetype(p, size)
        except OSError:
            continue
    sys.exit("no monospace font found")


class Renderer:
    def __init__(self, cols, rows, font_size=15, pad=14, title=None):
        self.font = load_font(FONT_CANDIDATES, font_size)
        self.bold = load_font(BOLD_CANDIDATES, font_size)
        box = self.font.getbbox("M")
        self.cw = int(round(self.font.getlength("M")))
        self.ch = font_size + 6
        self.pad = pad
        self.top = pad + (26 if title else 0)
        self.title = title
        self.baseline = -box[1] + 3
        self.w = cols * self.cw + 2 * pad
        self.h = rows * self.ch + self.top + pad
        self.cols, self.rows = cols, rows

    def draw(self, screen: Screen, cursor=True):
        img = Image.new("RGB", (self.w, self.h), BG)
        d = ImageDraw.Draw(img)
        if self.title:
            d.rectangle([0, 0, self.w, self.top - 6], fill=(16, 19, 25))
            for i, c in enumerate([(255, 95, 86), (255, 189, 46), (39, 201, 63)]):
                d.ellipse([self.pad + i * 18, 10, self.pad + i * 18 + 10, 20], fill=c)
            d.text((self.pad + 70, 8), self.title, font=self.font, fill=(120, 132, 148))
        for y, row in enumerate(screen.grid):
            py = self.top + y * self.ch
            x = 0
            while x < self.cols:
                cell = row[x]
                if cell.bg is not None:
                    run = x
                    while run < self.cols and row[run].bg == cell.bg:
                        run += 1
                    d.rectangle(
                        [self.pad + x * self.cw, py, self.pad + run * self.cw, py + self.ch],
                        fill=cell.bg,
                    )
                    x = run
                    continue
                x += 1
            for x, cell in enumerate(row):
                if cell.ch == " ":
                    continue
                fg = cell.fg if cell.fg is not None else FG
                if cell.dim:
                    fg = tuple(int(v * 0.62 + BG[i] * 0.38) for i, v in enumerate(fg))
                px = self.pad + x * self.cw
                d.text((px, py + self.baseline), cell.ch,
                       font=self.bold if cell.bold else self.font, fill=fg)
                if cell.underline:
                    d.line([px, py + self.ch - 2, px + self.cw, py + self.ch - 2], fill=fg)
        if cursor and screen.cursor_visible and screen.y < self.rows:
            cx, cy = self.pad + screen.x * self.cw, self.top + screen.y * self.ch
            d.rectangle([cx, cy + 1, cx + self.cw - 1, cy + self.ch - 2], fill=CURSOR)
            cell = screen.grid[screen.y][screen.x] if screen.x < self.cols else None
            if cell and cell.ch != " ":
                d.text((cx, cy + self.baseline), cell.ch, font=self.font, fill=BG)
        return img


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cast")
    ap.add_argument("-o", "--out", required=True)
    ap.add_argument("--fps", type=float, default=16.0)
    ap.add_argument("--font-size", type=int, default=15)
    ap.add_argument("--speed", type=float, default=1.0, help=">1 plays faster")
    ap.add_argument("--max-idle", type=float, default=1.2, help="cap dead air, seconds")
    ap.add_argument("--tail", type=float, default=2.0, help="hold the last frame, seconds")
    ap.add_argument("--title", default=None)
    ap.add_argument("--colors", type=int, default=128)
    args = ap.parse_args()

    with open(args.cast, encoding="utf-8") as fh:
        meta = json.loads(fh.readline())
        events = [json.loads(line) for line in fh if line.strip()]

    cols, rows = meta["width"], meta["height"]
    term = Terminal(cols, rows)
    renderer = Renderer(cols, rows, font_size=args.font_size, title=args.title)

    # collapse dead air, then sample the stream on a fixed clock
    adjusted, prev, shift = [], 0.0, 0.0
    for t, kind, data in events:
        gap = t - prev
        if gap > args.max_idle:
            shift += gap - args.max_idle
        prev = t
        adjusted.append((max(0.0, (t - shift) / args.speed), data))

    total = adjusted[-1][0] if adjusted else 0.0
    step = 1.0 / args.fps
    frames, durations, last_key = [], [], None
    idx, clock = 0, 0.0
    while clock <= total + step:
        while idx < len(adjusted) and adjusted[idx][0] <= clock:
            term.feed(adjusted[idx][1])
            idx += 1
        key = term.screen.snapshot()
        if key == last_key and frames:
            durations[-1] += step
        else:
            frames.append(renderer.draw(term.screen))
            durations.append(step)
            last_key = key
        clock += step
    durations[-1] += args.tail

    palette_src = frames[len(frames) // 2].quantize(colors=args.colors, method=Image.MEDIANCUT)
    quantized = [f.quantize(palette=palette_src, dither=Image.NONE) for f in frames]
    quantized[0].save(
        args.out,
        save_all=True,
        append_images=quantized[1:],
        duration=[max(20, int(d * 1000)) for d in durations],
        loop=0,
        optimize=True,
        disposal=1,
    )
    size = __import__("os").path.getsize(args.out)
    print(f"{args.out}: {len(frames)} frames, {sum(durations):.1f}s, {size/1024:.0f} KiB")


if __name__ == "__main__":
    main()
