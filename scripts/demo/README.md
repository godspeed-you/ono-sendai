# The recordings

The GIFs in the project README are made here. Nothing in them is drawn, retouched or re-timed:
a tape is typed into the real `ono` binary over a pty, every byte the terminal received is kept
with the moment it arrived, and the renderer replays that stream through a VT emulator into
frames. A recording is therefore evidence of what the shell answered on the machine it ran on —
which is the only property that makes it worth putting in a README (`docs/ACCEPTANCE.md` §2 makes
the same argument about the container).

```bash
scripts/demo/make.sh                 # build the image, record and render every tape
scripts/demo/make.sh spatial trace   # only these
scripts/demo/make.sh --no-build      # reuse the image that is already built
scripts/demo/make.sh --local         # record against target/release/ono, on this machine
```

Requirements: docker (unless `--local`), python3 and pillow (`apt install python3-pil`).

## The machine

`docker/demo.Dockerfile` is the acceptance runtime — a clean Debian, the release binary, an
unprivileged user whose login shell is `ono` — plus the two things the recordings need to have
something to talk about: an nginx on 8080 and a redis on 6379, both started by
`container/entrypoint.sh` and neither tuned for the camera. The container runs with
`--hostname deck`, so the frames name a machine anyone can rebuild instead of a developer's
laptop.

The spatial recording depends on that nginx being *discoverable rather than known*: it finds the
listener by port and lets the neighbourhood name the process. If you change the fixture, change
the tape's story with it — a scene that types the answer it is supposed to discover proves
nothing.

## Tapes

One directive per line, in `tapes/*.tape`:

| Directive | Meaning |
|---|---|
| `title <text>` | caption drawn into the GIF's title bar |
| `size <cols> <rows>` | terminal geometry, set before the shell starts |
| `env NAME value` | environment for the shell |
| `cd <dir>` | working directory (`--local` recordings) |
| `run <pipeline>` | type the line, press Enter, wait for the prompt to return |
| `type <text>` | type without pressing Enter |
| `key <name>` | `enter`, `ctrl-c`, `up`, `down`, `tab`, `esc`, … |
| `wait <seconds>` | hold, so a reader can read |
| `idle <seconds>` | keep recording while the shell paints — `watch`, `map`, `view` |

`record.py` writes an [asciinema v2](https://docs.asciinema.org/manual/asciicast/v2/) cast, so the
intermediate artifact is a standard format you can replay or diff. `render.py` turns a cast into
a GIF: dead air longer than `--max-idle` is collapsed, identical screens become one frame with a
longer duration, and the palette is quantised once for the whole animation.

```bash
python3 scripts/demo/record.py tapes/spatial.tape -o /tmp/spatial.cast --shell target/release/ono
python3 scripts/demo/render.py /tmp/spatial.cast -o docs/assets/spatial.gif --title "ono"
```

`render.py --speed 1.3`, `--fps`, `--font-size` and `--colors` trade file size against fidelity.
Keep a recording under ~30 s and ~350 KiB: a README that takes a second to load is part of the
product.
