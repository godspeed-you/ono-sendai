# Acceptance fixtures

Files copied into the acceptance image at `/opt/ono-fixtures`, owned by the unprivileged `case`
user. A case that needs input data reads it from here instead of creating it inline, so the case
file stays about the behaviour it proves.

Everything here must be deterministic: no timestamps that drift, no content that depends on the
machine that built the image.
