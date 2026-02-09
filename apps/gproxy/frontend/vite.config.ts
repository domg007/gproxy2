import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";

function readFrontendPackageVersion(): string {
  try {
    const raw = readFileSync(new URL("./package.json", import.meta.url), "utf8");
    const parsed = JSON.parse(raw) as { version?: string };
    return parsed.version ?? "0.0.0";
  } catch {
    return "0.0.0";
  }
}

function readWorkspaceVersion(): string | null {
  try {
    const raw = readFileSync(new URL("../../../Cargo.toml", import.meta.url), "utf8");
    const section = raw.match(/\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/);
    if (!section?.[1]) {
      return null;
    }
    const version = section[1].match(/^\s*version\s*=\s*"([^"]+)"/m);
    return version?.[1]?.trim() || null;
  } catch {
    return null;
  }
}

function readAppVersion(): string {
  const envVersion = process.env.APP_VERSION ?? process.env.GPROXY_VERSION;
  if (envVersion && envVersion.trim()) {
    return envVersion.trim();
  }
  return readWorkspaceVersion() ?? readFrontendPackageVersion();
}

function readGitShortHash(): string {
  const envCommit =
    process.env.APP_COMMIT ??
    process.env.GPROXY_COMMIT ??
    process.env.GITHUB_SHA ??
    process.env.RENDER_GIT_COMMIT ??
    process.env.VERCEL_GIT_COMMIT_SHA ??
    process.env.CI_COMMIT_SHA;
  if (envCommit && envCommit.trim()) {
    return envCommit.trim().slice(0, 7);
  }

  try {
    return execSync("git rev-parse --short HEAD", {
      stdio: ["ignore", "pipe", "ignore"]
    })
      .toString()
      .trim();
  } catch {
    return "dev";
  }
}

const appVersion = readAppVersion();
const appCommit = readGitShortHash();

export default defineConfig({
  plugins: [react(), tailwindcss()],
  define: {
    __APP_VERSION__: JSON.stringify(appVersion),
    __APP_COMMIT__: JSON.stringify(appCommit)
  },
  base: "/",
  build: {
    outDir: "dist",
    assetsDir: "assets",
    emptyOutDir: true
  }
});
