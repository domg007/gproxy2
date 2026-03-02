#!/usr/bin/env node

import { createWriteStream } from "node:fs";
import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";

const REPO = "LeenHawk/gproxy";
const API_ROOT = "https://api.github.com";
const DEFAULT_DOCS_BASE_URL = "https://gproxy.leenhawk.com";
const ASSET_NAME_RE = /\.zip(?:\.sha256)?$/;

const CHANNELS = [
  { channel: "release", apiPath: `/repos/${REPO}/releases/latest` },
  { channel: "staging", apiPath: `/repos/${REPO}/releases/tags/staging` }
];

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const docsDir = path.resolve(scriptDir, "..");
const downloadsRoot = path.join(docsDir, "public", "downloads");
const docsBaseUrl = (process.env.DOCS_BASE_URL || DEFAULT_DOCS_BASE_URL).trim().replace(/\/+$/, "");

function headersForJson() {
  return {
    accept: "application/vnd.github+json",
    "user-agent": "gproxy-docs-sync"
  };
}

function headersForBinary() {
  return {
    accept: "application/octet-stream",
    "user-agent": "gproxy-docs-sync"
  };
}

async function fetchJson(url, label) {
  const response = await fetch(url, { headers: headersForJson() });
  if (!response.ok) {
    const body = await response.text();
    throw new Error(`${label} failed: status=${response.status} body=${body}`);
  }
  return response.json();
}

async function downloadFile(url, filePath) {
  const response = await fetch(url, { headers: headersForBinary() });
  if (!response.ok || !response.body) {
    const body = await response.text();
    throw new Error(`download failed: url=${url} status=${response.status} body=${body}`);
  }
  await pipeline(response.body, createWriteStream(filePath));
}

async function readSha256(dir, zipName) {
  const shaPath = path.join(dir, `${zipName}.sha256`);
  try {
    const raw = (await readFile(shaPath, "utf8")).trim();
    if (!raw) {
      return null;
    }
    return raw.split(/\s+/)[0] || null;
  } catch {
    return null;
  }
}

async function syncChannel({ channel, apiPath }) {
  const releaseUrl = `${API_ROOT}${apiPath}`;
  const release = await fetchJson(releaseUrl, `fetch ${channel} release`);
  const tag = String(release.tag_name || "").trim();
  if (!tag) {
    throw new Error(`missing tag_name for channel=${channel}`);
  }

  const assets = Array.isArray(release.assets) ? release.assets : [];
  const selected = assets.filter((item) => ASSET_NAME_RE.test(String(item.name || "")));
  if (selected.length === 0) {
    throw new Error(`no zip assets found for channel=${channel}`);
  }

  await mkdir(downloadsRoot, { recursive: true });
  const tempDir = path.join(
    downloadsRoot,
    `.tmp-${channel}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
  );
  await rm(tempDir, { recursive: true, force: true });
  await mkdir(tempDir, { recursive: true });

  for (const asset of selected) {
    const name = String(asset.name || "").trim();
    const url = String(asset.browser_download_url || "").trim();
    if (!name || !url) {
      continue;
    }
    const filePath = path.join(tempDir, name);
    await downloadFile(url, filePath);
  }

  const zipFiles = selected
    .map((item) => String(item.name || "").trim())
    .filter((name) => name.endsWith(".zip"))
    .sort();

  const manifestAssets = [];
  for (const zipName of zipFiles) {
    const item = {
      name: zipName,
      url: `${docsBaseUrl}/downloads/${channel}/${zipName}`
    };
    const sha256 = await readSha256(tempDir, zipName);
    if (sha256) {
      item.sha256 = sha256;
    }
    manifestAssets.push(item);
  }

  const manifest = {
    channel,
    tag,
    assets: manifestAssets
  };
  await writeFile(path.join(tempDir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);

  const targetDir = path.join(downloadsRoot, channel);
  await rm(targetDir, { recursive: true, force: true });
  await rename(tempDir, targetDir);
  console.log(`synced ${channel}: tag=${tag} assets=${manifestAssets.length}`);
}

async function main() {
  for (const config of CHANNELS) {
    await syncChannel(config);
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
