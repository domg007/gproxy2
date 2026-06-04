#!/usr/bin/env python3
"""Fetch the fixed upstream API markdown allowlist into ./upstream_docs.

This intentionally does not crawl provider indexes or download llms-full
exports. Add URLs to sources.toml only after reviewing the page scope.
"""

from __future__ import annotations

import argparse
import datetime as dt
import pathlib
import sys
import tomllib
import urllib.request


ROOT = pathlib.Path(__file__).resolve().parents[2]
SOURCES = ROOT / "tools" / "upstream_docs" / "sources.toml"
UPSTREAM_DOCS = ROOT / "upstream_docs"


def load_sources() -> dict:
    with SOURCES.open("rb") as f:
        return tomllib.load(f)


def fetch_text(url: str) -> str:
    req = urllib.request.Request(
        url,
        headers={
            "User-Agent": "gproxy-v2-upstream-docs/0.1",
            "Accept": "text/markdown,text/plain,*/*",
        },
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        charset = resp.headers.get_content_charset() or "utf-8"
        return resp.read().decode(charset, errors="replace")


def write_provider_readme(provider_dir: pathlib.Path, name: str, cfg: dict) -> None:
    today = dt.date.today().isoformat()
    lines = [
        f"# {name} upstream API docs",
        "",
        f"Downloaded from the fixed allowlist on {today}.",
        "",
        "Only add pages here after reviewing `tools/upstream_docs/sources.toml`.",
        "",
        "## Sources",
        "",
    ]
    for doc in cfg.get("docs", []):
        lines.append(
            f"- [{doc['title']}]({doc['url']}) -> `docs/{doc['file']}` "
            f"({doc['source_kind']}, operation_group={doc['operation_group']})"
        )
    lines.extend(["", f"Changelog: {cfg['changelog']}", ""])
    (provider_dir / "README.md").write_text("\n".join(lines), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--provider",
        action="append",
        help="Provider id to fetch. May be repeated. Defaults to all providers.",
    )
    args = parser.parse_args()

    sources = load_sources()["providers"]
    selected = set(args.provider or sources.keys())
    unknown = selected - set(sources.keys())
    if unknown:
        print(f"unknown provider(s): {', '.join(sorted(unknown))}", file=sys.stderr)
        return 2

    UPSTREAM_DOCS.mkdir(exist_ok=True)
    for provider_id, cfg in sources.items():
        if provider_id not in selected:
            continue
        provider_dir = UPSTREAM_DOCS / provider_id
        docs_dir = provider_dir / "docs"
        docs_dir.mkdir(parents=True, exist_ok=True)
        write_provider_readme(provider_dir, cfg["name"], cfg)

        for doc in cfg.get("docs", []):
            text = fetch_text(doc["url"])
            if not text.strip():
                raise RuntimeError(f"empty response for {doc['url']}")
            out = docs_dir / doc["file"]
            out.write_text(text, encoding="utf-8")
            print(f"fetched {provider_id}: {doc['title']} -> {out.relative_to(ROOT)}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
