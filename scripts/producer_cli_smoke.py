#!/usr/bin/env python3
"""Live proof that glance-catalog's producer CLI (glance-929) is reachable
from a non-Rust caller: build the real binary, pipe a spec JSON in over
stdin, read rendered HTML back over stdout, and check the documented exit
codes -- no Rust dependency of this script's own. See docs/producer-cli.md
for the full contract this exercises.

Run from the repo root:
    python3 scripts/producer_cli_smoke.py
"""
import json
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def build_binary() -> Path:
    subprocess.run(
        ["cargo", "build", "-p", "glance-catalog", "--bin", "glance-catalog"],
        cwd=REPO_ROOT,
        check=True,
    )
    binary = REPO_ROOT / "target" / "debug" / "glance-catalog"
    assert binary.is_file(), f"expected a compiled binary at {binary}"
    return binary


def run_cli(binary: Path, spec: dict, extra_args=()) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(binary), *extra_args],
        input=json.dumps(spec),
        capture_output=True,
        text=True,
    )


def check(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def main() -> int:
    binary = build_binary()

    valid_spec = {
        "catalog_version": "aesthetic-catalog-001",
        "title": "Producer CLI smoke (Python)",
        "components": [
            {
                "type": "hero",
                "title": "Hello from Python",
                "summary": [{"type": "text", "text": "rendered with no Rust dependency"}],
            },
            {"type": "markdown", "content": "This page was requested by a Python script."},
        ],
    }
    result = run_cli(binary, valid_spec)
    check(result.returncode == 0, f"expected exit 0, got {result.returncode}: {result.stderr}")
    check(result.stdout.startswith("<!doctype html>"), "expected a self-contained HTML document")
    check("<title>Producer CLI smoke (Python)</title>" in result.stdout, "expected the title to round-trip")
    check('data-glance-component="hero"' in result.stdout, "expected the hero to render")
    print("ok: valid spec renders to self-contained HTML (exit 0)")

    invalid_spec = {"catalog_version": "wrong-version", "components": []}
    result = run_cli(binary, invalid_spec)
    check(result.returncode == 2, f"expected exit 2 for an invalid spec, got {result.returncode}")
    check("invalid spec" in result.stderr, "expected the documented error prefix")
    print("ok: invalid catalog_version fails closed with exit 2")

    malformed = subprocess.run(
        [str(binary)], input="not json", capture_output=True, text=True
    )
    check(malformed.returncode == 2, f"expected exit 2 for malformed json, got {malformed.returncode}")
    print("ok: malformed JSON fails closed with exit 2")

    print("producer_cli_smoke: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
