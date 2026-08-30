#!/bin/sh
# Start the fixtures the recordings observe, then hand the terminal to the shell.
set -e
nginx -c /etc/nginx/demo.conf 2>/dev/null || echo "demo: nginx did not start" >&2
redis-server --port 6379 --daemonize yes --save "" --dir /tmp --logfile /tmp/redis.log \
  2>/dev/null || echo "demo: redis did not start" >&2
exec "$@"
