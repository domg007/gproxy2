#!/usr/bin/env python3
"""Smoke-extract basic facts from fetched upstream docs.

The output is a review aid, not a generated protocol spec. v2 protocol docs
should still be authored by Operation/OperationGroup after reviewing the facts.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
import tomllib


ROOT = pathlib.Path(__file__).resolve().parents[2]
SOURCES = ROOT / "tools" / "upstream_docs" / "sources.toml"
UPSTREAM_DOCS = ROOT / "upstream_docs"

ENDPOINT_RE = re.compile(
    r"\b(GET|POST|PUT|PATCH|DELETE)\s+"
    r"(?:https?://[^\s\"'`)]+)?"
    r"(/[A-Za-z0-9_./:{}?$=&,*+-]+|https://generativelanguage.googleapis.com/[^\s\"'`)]+)",
    re.IGNORECASE,
)
HTTP_URL_RE = re.compile(
    r"https://(?:api\.openai\.com|api\.anthropic\.com|generativelanguage\.googleapis\.com)"
    r"(/[^\s\"'`)]+)"
)


def load_sources() -> dict:
    with SOURCES.open("rb") as f:
        return tomllib.load(f)["providers"]


def headings(text: str) -> list[str]:
    out: list[str] = []
    for line in text.splitlines():
        if line.startswith("#"):
            title = line.lstrip("#").strip()
            if title:
                out.append(title)
    return out[:12]


def endpoints(text: str) -> list[str]:
    found: list[str] = []
    for match in ENDPOINT_RE.finditer(text):
        item = f"{match.group(1).upper()} {match.group(2)}"
        if item not in found:
            found.append(item)
    for match in HTTP_URL_RE.finditer(text):
        item = match.group(1)
        if item not in found:
            found.append(item)
    return found[:8]


def contains_any(text: str, needles: tuple[str, ...]) -> bool:
    lower = text.lower()
    return any(needle in lower for needle in needles)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="Fail if basic facts are missing.")
    args = parser.parse_args()

    failures: list[str] = []
    print("# Upstream docs extraction smoke\n")
    for provider_id, cfg in load_sources().items():
        for doc in cfg.get("docs", []):
            path = UPSTREAM_DOCS / provider_id / "docs" / doc["file"]
            if not path.exists():
                failures.append(f"missing {path.relative_to(ROOT)}")
                continue
            text = path.read_text(encoding="utf-8", errors="replace")
            doc_endpoints = endpoints(text)
            doc_headings = headings(text)
            has_request = contains_any(text, ("request body", "request", "body"))
            has_response = contains_any(text, ("response body", "response", "returns"))

            print(f"## {provider_id}: {doc['title']}")
            print(f"- operation_group: {doc['operation_group']}")
            print(f"- source: {doc['url']}")
            print(f"- file: {path.relative_to(ROOT)}")
            print(f"- endpoints: {', '.join(doc_endpoints) if doc_endpoints else '<none found>'}")
            print(f"- has_request_text: {str(has_request).lower()}")
            print(f"- has_response_text: {str(has_response).lower()}")
            print(f"- headings: {', '.join(doc_headings) if doc_headings else '<none found>'}")
            print()

            if not doc_endpoints:
                failures.append(f"no endpoint found in {path.relative_to(ROOT)}")
            if not has_request:
                failures.append(f"no request text found in {path.relative_to(ROOT)}")
            if not has_response:
                failures.append(f"no response text found in {path.relative_to(ROOT)}")

    if failures and args.check:
        for failure in failures:
            print(f"error: {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
