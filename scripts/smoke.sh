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

mkdir -p "$tmpdir/publish-site/src/parser"
printf '<!doctype html><p>root</p>' > "$tmpdir/publish-site/index.html"
printf '<!doctype html><p>parser</p>' > "$tmpdir/publish-site/src/parser/index.html"
printf '{"smoke":true}\n' > "$tmpdir/publish-site/metadata.json"
git init --bare "$tmpdir/smoke-glance.git" >/dev/null
cargo run -q -p glance -- publish \
  --site-dir "$tmpdir/publish-site" \
  --source-owner misty-step \
  --source-name smoke \
  --source-sha "$sha" \
  --mode master \
  --sister-remote "file://$tmpdir/smoke-glance.git" \
  --sister-worktree "$tmpdir/smoke-glance-worktree" \
  --run-id smoke \
  > "$tmpdir/publish-1.out"
grep -q 'changed=true' "$tmpdir/publish-1.out"
cargo run -q -p glance -- publish \
  --site-dir "$tmpdir/publish-site" \
  --source-owner misty-step \
  --source-name smoke \
  --source-sha "$sha" \
  --mode master \
  --sister-remote "file://$tmpdir/smoke-glance.git" \
  --sister-worktree "$tmpdir/smoke-glance-worktree" \
  --run-id smoke \
  > "$tmpdir/publish-2.out"
grep -q 'changed=false' "$tmpdir/publish-2.out"

cp -R crates/glance-core/tests/fixtures/mini-source/. "$tmpdir/mini-source"
git -C "$tmpdir/mini-source" -c core.hooksPath=/dev/null init -b main >/dev/null
git -C "$tmpdir/mini-source" -c core.hooksPath=/dev/null add .
git -C "$tmpdir/mini-source" \
  -c core.hooksPath=/dev/null \
  -c user.name=glance-smoke \
  -c user.email=glance-smoke@example.invalid \
  commit --no-verify -m mini-source >/dev/null
mini_sha="$(git -C "$tmpdir/mini-source" -c core.hooksPath=/dev/null rev-parse HEAD)"
printf 'source_sha = "%s"\n' "$mini_sha" > "$tmpdir/glance.toml"
cargo run -q -p glance -- --config "$tmpdir/glance.toml" run \
  --root "$tmpdir/mini-source" \
  --site-root "$tmpdir/generated-site" \
  > "$tmpdir/run.out"
grep -q 'would_generate=. kind=Root tier=Frontier provider=mock model=openai/gpt-5.5 max_tokens=16000 input_tokens=0 output_tokens=0 spend_micros=0' "$tmpdir/run.out"
grep -q 'spend_report pages=4 input_tokens=0 output_tokens=0 spend_micros=0' "$tmpdir/run.out"
test -f "$tmpdir/generated-site/index.html"
test -f "$tmpdir/generated-site/metadata.json"
test -f "$tmpdir/generated-site/docs/index.html"
test -f "$tmpdir/generated-site/src/index.html"
test -f "$tmpdir/generated-site/src/parser/index.html"
grep -q '"prompt_version": "glance-006-root-v1"' "$tmpdir/generated-site/metadata.json"
grep -q 'data-glance-section="what-this-is"' "$tmpdir/generated-site/index.html"
grep -q 'data-glance-component="hero"' "$tmpdir/generated-site/index.html"
grep -q 'data-glance-component="narrative"' "$tmpdir/generated-site/index.html"
grep -q 'data-glance-component="file_table"' "$tmpdir/generated-site/index.html"
grep -q 'data-glance-component="disclosure"' "$tmpdir/generated-site/index.html"
! grep -Eq '\[[^]]+:[0-9]+(-[0-9]+)?\]' "$tmpdir/generated-site/index.html"
grep -q 'href="docs/index.html"' "$tmpdir/generated-site/index.html"
grep -q 'href="src/index.html"' "$tmpdir/generated-site/index.html"
grep -q 'href="../index.html"' "$tmpdir/generated-site/docs/index.html"
grep -q 'href="parser/index.html"' "$tmpdir/generated-site/src/index.html"
grep -q 'glance-flow-diagram' "$tmpdir/generated-site/index.html"
grep -q 'data-theme-choice="system"' "$tmpdir/generated-site/index.html"
grep -q '<img src="glance-image-001.png"' "$tmpdir/generated-site/index.html"
test -f "$tmpdir/generated-site/glance-image-001.png"
cargo run -q -p glance -- check \
  --source-root "$tmpdir/mini-source" \
  --source-sha "$mini_sha" \
  "$tmpdir/generated-site/index.html" \
  "$tmpdir/generated-site/docs/index.html" \
  "$tmpdir/generated-site/src/index.html" \
  "$tmpdir/generated-site/src/parser/index.html" \
  > "$tmpdir/generated-check.out"
grep -q 'checked ' "$tmpdir/generated-check.out"

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
