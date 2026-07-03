#!/usr/bin/env bash
set -euo pipefail

tmpdir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

cargo run -q -p glance -- plan \
  --root crates/glance-core/tests/fixtures/mini-source \
  --changed src/parser/mod.rs \
  > "$tmpdir/plan.out"
diff -u <(printf 'src/parser\nsrc\n.\n') "$tmpdir/plan.out"

cp -R crates/glance-check/tests/fixtures/source/. "$tmpdir/source"
git -C "$tmpdir/source" -c core.hooksPath=/dev/null init -b main >/dev/null
git -C "$tmpdir/source" -c core.hooksPath=/dev/null add .
git -C "$tmpdir/source" \
  -c core.hooksPath=/dev/null \
  -c user.name=glance-smoke \
  -c user.email=glance-smoke@example.invalid \
  commit --no-verify -m fixture >/dev/null
sha="$(git -C "$tmpdir/source" -c core.hooksPath=/dev/null rev-parse HEAD)"

cargo run -q -p glance -- check \
  --source-root "$tmpdir/source" \
  --source-sha "$sha" \
  crates/glance-check/tests/fixtures/generated/good.html \
  > "$tmpdir/check.out"
grep -q 'checked 2 citations' "$tmpdir/check.out"

printf 'source_sha = "fixture-sha"\n' > "$tmpdir/glance.toml"
cargo run -q -p glance -- --config "$tmpdir/glance.toml" run \
  --root crates/glance-core/tests/fixtures/mini-source \
  > "$tmpdir/run.out"
grep -q 'would_generate=. tier=Frontier spend_micros=0' "$tmpdir/run.out"

mkdir -p "$tmpdir/site"
printf '<!doctype html><title>glance smoke</title><p>ok</p>' > "$tmpdir/site/index.html"
cargo run -q -p glance -- serve-local \
  --site-root "$tmpdir/site" \
  --port 0 \
  --once \
  > "$tmpdir/server.log" 2>&1 &
server_pid="$!"

address=""
for _ in 1 2 3 4 5 6 7 8 9 10; do
  address="$(grep -Eo '127\.0\.0\.1:[0-9]+' "$tmpdir/server.log" | head -n 1 || true)"
  if [[ -n "$address" ]]; then
    break
  fi
  sleep 0.2
done
if [[ -z "$address" ]]; then
  cat "$tmpdir/server.log"
  exit 1
fi

curl -fsS "http://$address/" >/dev/null
wait "$server_pid"
