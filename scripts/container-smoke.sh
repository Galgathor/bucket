#!/usr/bin/env bash
set -euo pipefail
image="${1:-bucket:check}"
name="bucket-smoke-${RANDOM}-$$"
volume="${name}-data"
cleanup() {
  docker rm -f "$name" >/dev/null 2>&1 || true
  docker volume rm "$volume" >/dev/null 2>&1 || true
}
trap cleanup EXIT
start() {
  docker run -d --name "$name" --read-only --cap-drop ALL --security-opt no-new-privileges \
    --tmpfs /tmp:size=16m --mount "type=volume,src=$volume,dst=/data" \
    -e BUCKET_PUBLIC_URL=https://bucket.example.com \
    -e BUCKET_MAILCOW_URL=https://mail.invalid \
    -e BUCKET_MAILCOW_API_KEY=test-only-key-not-a-real-secret \
    -e BUCKET_IMAP_HOST=mail.invalid -e BUCKET_ADMIN_EMAIL=admin@example.com "$image" >/dev/null
  for attempt in $(seq 1 30); do
    if docker exec "$name" curl -fsS http://127.0.0.1:8080/api/health >/dev/null 2>&1; then return; fi
    sleep 1
  done
  docker logs "$name"
  return 1
}
start
test "$(docker exec "$name" id -u)" = 10001
docker exec "$name" curl -fsS http://127.0.0.1:8080/ | grep -q '<title>Bucket</title>'
docker exec "$name" curl -fsS http://127.0.0.1:8080/manifest.webmanifest | grep -q 'standalone'
# A file in the data volume must survive removal/replacement of the container.
docker exec "$name" sh -c 'printf persistent > /data/smoke-marker'
docker stop --time 45 "$name" >/dev/null
docker rm "$name" >/dev/null
start
test "$(docker exec "$name" cat /data/smoke-marker)" = persistent
docker exec "$name" test -s /data/bucket.db
echo 'Container health, static assets, non-root runtime, restart, and persistence passed.'
