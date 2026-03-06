#!/usr/bin/env python3

from __future__ import annotations

import datetime as dt
import json
import os
import shutil
import sys
from pathlib import Path
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import Request, urlopen


ALLOWED_SUFFIXES = (".zip", ".zip.sha256", ".zip.sha256.sig")
ZIP_SUFFIX = ".zip"
DEFAULT_DOWNLOADS_BASE_URL = "https://download-gproxy.leenhawk.com"
DEFAULT_UPDATE_SIGNING_KEY_ID = "gproxy-release-v1"


def getenv(name: str, default: str | None = None) -> str | None:
    value = os.getenv(name)
    if value is None:
        return default
    value = value.strip()
    return value or default


REPO = getenv("GH_REPO", getenv("GITHUB_REPOSITORY"))
TOKEN = getenv("GH_TOKEN", getenv("GITHUB_TOKEN", ""))
PUBLIC_BASE_URL = (getenv("DOWNLOAD_PUBLIC_BASE_URL", DEFAULT_DOWNLOADS_BASE_URL) or DEFAULT_DOWNLOADS_BASE_URL).rstrip("/")
OUTPUT_DIR = Path(getenv("DOWNLOADS_OUTPUT_DIR", "dist/downloads-site") or "dist/downloads-site")
UPDATE_SIGNING_KEY_ID = getenv("UPDATE_SIGNING_KEY_ID", DEFAULT_UPDATE_SIGNING_KEY_ID) or DEFAULT_UPDATE_SIGNING_KEY_ID


def die(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


if not REPO:
    die("missing GH_REPO or GITHUB_REPOSITORY")


def request_json(url: str):
    req = Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "User-Agent": "gproxy-cloudflare-downloads-sync",
            **({"Authorization": f"Bearer {TOKEN}"} if TOKEN else {}),
        },
    )
    with urlopen(req) as response:
        return json.load(response)


def download_file(url: str, target: Path) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    req = Request(
        url,
        headers={
            "User-Agent": "gproxy-cloudflare-downloads-sync",
            **({"Authorization": f"Bearer {TOKEN}"} if TOKEN else {}),
        },
    )
    with urlopen(req) as response, target.open("wb") as handle:
        shutil.copyfileobj(response, handle)


def github_api(path: str) -> str:
    return f"https://api.github.com/repos/{REPO}{path}"


def fetch_release_by_tag(tag: str):
    try:
        return request_json(github_api(f"/releases/tags/{quote(tag)}"))
    except HTTPError as exc:
        if exc.code == 404:
            return None
        raise


def fetch_latest_release():
    return request_json(github_api("/releases/latest"))


def fetch_all_releases():
    releases = []
    page = 1
    while True:
        chunk = request_json(github_api(f"/releases?per_page=100&page={page}"))
        if not chunk:
            break
        releases.extend(chunk)
        if len(chunk) < 100:
            break
        page += 1
    return releases


def is_download_asset(name: str) -> bool:
    return any(name.endswith(suffix) for suffix in ALLOWED_SUFFIXES)


def release_version(tag_name: str) -> str:
    return tag_name[1:] if tag_name.startswith("v") else tag_name


def parse_checksum(checksum_path: Path, asset_name: str) -> str | None:
    if not checksum_path.exists():
        return None
    for raw_line in checksum_path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line:
            continue
        parts = line.split()
        if len(parts) == 1:
            return parts[0]
        if len(parts) >= 2 and parts[-1] == asset_name:
            return parts[0]
    return None


def public_url(path: str) -> str:
    return f"{PUBLIC_BASE_URL}/{quote(path, safe='/')}"


def manifest_asset_entry(path: str, published_at: str | None) -> dict[str, object]:
    asset_path = OUTPUT_DIR / path
    name = asset_path.name
    sha_path = asset_path.with_name(f"{name}.sha256")
    sig_path = asset_path.with_name(f"{name}.sha256.sig")
    entry: dict[str, object] = {
        "name": name,
        "path": path,
        "url": public_url(path),
        "size": asset_path.stat().st_size,
    }
    if published_at:
        entry["last_modified"] = published_at
    if sha_path.exists():
        entry["sha256_url"] = public_url(f"{path}.sha256")
        parsed = parse_checksum(sha_path, name)
        if parsed:
            entry["sha256"] = parsed
    if sig_path.exists():
        entry["sha256_sig_url"] = public_url(f"{path}.sha256.sig")
    return entry


def channel_manifest_entry(path: str) -> dict[str, object]:
    entry = manifest_asset_entry(path, None)
    trimmed = {
        "name": entry["name"],
        "url": entry["url"],
        "key_id": UPDATE_SIGNING_KEY_ID,
    }
    if "sha256" in entry:
        trimmed["sha256"] = entry["sha256"]
    if "sha256_url" in entry:
        trimmed["sha256_url"] = entry["sha256_url"]
    if "sha256_sig_url" in entry:
        trimmed["sha256_sig_url"] = entry["sha256_sig_url"]
    return trimmed


def download_release_assets(release: dict, prefix: str) -> list[str]:
    published_at = release.get("published_at") or release.get("created_at") or ""
    zip_paths: list[str] = []
    for asset in release.get("assets", []):
        name = asset.get("name")
        url = asset.get("browser_download_url")
        if not isinstance(name, str) or not isinstance(url, str) or not is_download_asset(name):
            continue
        site_path = f"{prefix}/{name}" if prefix else name
        download_file(url, OUTPUT_DIR / site_path)
        if name.endswith(ZIP_SUFFIX):
            zip_paths.append(site_path)
            RELEASE_TIMES[site_path] = published_at
    return sorted(zip_paths)


def copy_tree_entry(source: Path, target: Path) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, target)


def write_json(path: Path, payload: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def write_index() -> None:
    html = f"""<!doctype html>
<html lang=\"en\">
  <head>
    <meta charset=\"utf-8\" />
    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />
    <title>gproxy downloads</title>
    <style>
      body {{ font-family: system-ui, sans-serif; margin: 2rem auto; max-width: 48rem; line-height: 1.6; padding: 0 1rem; }}
      code {{ background: #f3f4f6; padding: 0.1rem 0.35rem; border-radius: 0.35rem; }}
    </style>
  </head>
  <body>
    <h1>gproxy downloads</h1>
    <p>This Cloudflare Pages site hosts binary update manifests and release files for gproxy.</p>
    <ul>
      <li><a href=\"/manifest.json\">/manifest.json</a></li>
      <li><a href=\"/releases/manifest.json\">/releases/manifest.json</a></li>
      <li><a href=\"/staging/manifest.json\">/staging/manifest.json</a></li>
    </ul>
    <p>Public base URL: <code>{PUBLIC_BASE_URL}</code></p>
  </body>
</html>
"""
    (OUTPUT_DIR / "index.html").write_text(html, encoding="utf-8")


RELEASE_TIMES: dict[str, str] = {}


def main() -> None:
    if OUTPUT_DIR.exists():
        shutil.rmtree(OUTPUT_DIR)
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

    latest_release = fetch_latest_release()
    all_releases = fetch_all_releases()
    stable_releases = [
        release
        for release in all_releases
        if not release.get("draft") and not release.get("prerelease") and release.get("tag_name") != "staging"
    ]

    releases_assets: list[str] = []
    for release in stable_releases:
        tag_name = release.get("tag_name")
        if not isinstance(tag_name, str) or not tag_name:
            continue
        releases_assets.extend(download_release_assets(release, f"releases/{release_version(tag_name)}"))

    latest_tag = latest_release.get("tag_name")
    if not isinstance(latest_tag, str) or not latest_tag:
        die("latest GitHub release is missing tag_name")
    latest_version = release_version(latest_tag)
    latest_release_dir = OUTPUT_DIR / "releases" / latest_version
    if not latest_release_dir.exists():
        die(f"latest release directory not found for {latest_tag}")

    root_assets: list[str] = []
    for source in sorted(latest_release_dir.iterdir()):
        target = OUTPUT_DIR / source.name
        copy_tree_entry(source, target)
        if source.name.endswith(ZIP_SUFFIX):
            root_assets.append(source.name)
            RELEASE_TIMES[source.name] = latest_release.get("published_at") or latest_release.get("created_at") or ""

    staging_release = fetch_release_by_tag("staging")
    staging_assets: list[str] = []
    if staging_release is not None:
        staging_assets = download_release_assets(staging_release, "staging")

    generated_at = dt.datetime.now(dt.UTC).isoformat()
    global_assets = [
        manifest_asset_entry(path, RELEASE_TIMES.get(path))
        for path in sorted(root_assets + releases_assets + staging_assets)
    ]
    write_json(
        OUTPUT_DIR / "manifest.json",
        {
            "generated_at": generated_at,
            "assets": global_assets,
        },
    )
    write_json(
        OUTPUT_DIR / "releases" / "manifest.json",
        {
            "tag": latest_tag,
            "channel": "releases",
            "key_id": UPDATE_SIGNING_KEY_ID,
            "assets": [channel_manifest_entry(path) for path in sorted(root_assets)],
        },
    )
    write_json(
        OUTPUT_DIR / "staging" / "manifest.json",
        {
            "tag": "staging",
            "channel": "staging",
            "key_id": UPDATE_SIGNING_KEY_ID,
            "assets": [channel_manifest_entry(path) for path in sorted(staging_assets)],
        },
    )
    write_index()
    print(f"cloudflare downloads site generated at {OUTPUT_DIR}")
    print(f"stable releases mirrored: {len(stable_releases)}")
    print(f"listed assets: {len(global_assets)}")


if __name__ == "__main__":
    main()
