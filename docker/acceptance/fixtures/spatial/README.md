# Spatial fixtures (v0.4)

The deterministic Linux fixture v0.4 §43.3 asks for, in the form an unprivileged user in a
container with no network can actually create:

| File | What it provides |
|---|---|
| `web-service.pl` | one main process that listens on a TCP port, holds a known file open, has worker children and accepts connections — the listener, the process→file edge, the multi-process service and the server half of a connection |
| `client.pl` | the client half, held open, so an established connection with two real endpoints exists |
| `systemctl` | a service manager that answers: one running web unit whose main pid is a real process, one failed backup unit. The image runs no systemd, so without this the service half of §44 could only be proved as honest unavailability |

Everything else §43.3 asks for is either already in the image (a mount boundary, several
namespaces, more than one uid in `/etc/passwd`) or cannot be created without privilege
(mounting, a second real service manager, a container runtime); the cases that need those say
so in their header and assert the honest `unknown` / `permission_denied` / `unsupported` state
of §35.2 instead of pretending.

The fixtures create real kernel state on purpose: v0.4 §43.6 forbids a live test that passes on
a timer instead of on an actual change.
