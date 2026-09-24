# syntax=docker/dockerfile:1.7
# The core build of `ono` (#127, ADR-0912, ADR-0913): the object shell alone, statically linked,
# in an image that holds nothing else.
#
# The binary is built on the host by `scripts/build-core.sh`, which also stages it here with the
# two account files below; the build context is that staging directory, never the repository,
# because the image is meant to contain exactly what it names.
#
#   scripts/build-core.sh --stage target/core-image
#   docker build --file docker/core/Dockerfile --target core target/core-image

# The deliverable: one static binary, the account it runs as, a home and a /tmp. No libc, no
# loader, no shell — `ono` is the only program in it, and it is the account's login shell.
FROM scratch AS core
COPY rootfs/ /
COPY --chown=1000:1000 home /home/case
USER case
WORKDIR /home/case
ENV HOME=/home/case ONO_IN_CONTAINER=1
ENTRYPOINT ["/usr/local/bin/ono"]

# What `scripts/acceptance.sh --profile core` runs its cases in: the image above, plus the
# harness's own interpreter under /opt/harness and nowhere else. A case script is a POSIX shell
# script (`docker/README.md`), so the harness needs a shell and the handful of tools a case uses
# to observe `ono` from outside; a static busybox is that, and it never stands in for `ono`
# itself — the binary under test is still the only program on the image's own paths.
FROM busybox:1.37.0-musl@sha256:5cec3fc171c87218698e85a52af7087de727372aae264a787b8112901a5b0092 AS harness

FROM core AS core-acceptance
COPY --from=harness /bin/busybox /opt/harness/bin/busybox
USER 0
RUN ["/opt/harness/bin/busybox", "--install", "-s", "/opt/harness/bin"]
USER case
ENV PATH=/usr/local/bin:/opt/harness/bin
ENTRYPOINT []
CMD ["/usr/local/bin/ono"]
