import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import fs from "node:fs";
import path from "node:path";
import { execSync } from "node:child_process";
import { fileURLToPath } from "node:url";

function parseWorkspaceInfo() {
  const currentFile = fileURLToPath(import.meta.url);
  const currentDir = path.dirname(currentFile);
  const workspaceRoot = path.resolve(currentDir, "../../..");
  const cargoTomlPath = path.join(workspaceRoot, "Cargo.toml");
  const cargoToml = fs.readFileSync(cargoTomlPath, "utf-8");

  const version = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1] ?? "dev";
  const authorRaw = cargoToml.match(/^authors\s*=\s*\["([^"]+)"\]/m)?.[1] ?? "";
  const author = (authorRaw.match(/^([^<]+)</)?.[1]?.trim() ?? authorRaw.trim()) || "unknown";
  const email = authorRaw.match(/<([^>]+)>/)?.[1] ?? "unknown";
  const homepage = cargoToml.match(/^homepage\s*=\s*"([^"]+)"/m)?.[1] ?? "";
  const repository = cargoToml.match(/^repository\s*=\s*"([^"]+)"/m)?.[1] ?? "";

  let commit = "unknown";
  try {
    commit = execSync("git rev-parse --short HEAD", {
      cwd: workspaceRoot,
      stdio: ["ignore", "pipe", "ignore"]
    })
      .toString("utf-8")
      .trim();
  } catch {
    commit = "unknown";
  }

  const buildOs = normalizeBuildOs(process.platform);
  const buildArch = normalizeBuildArch(process.arch);

  return { version, author, email, homepage, repository, commit, buildOs, buildArch };
}

const buildInfo = parseWorkspaceInfo();

function normalizeBuildOs(platform: NodeJS.Platform): string {
  switch (platform) {
    case "win32":
      return "windows";
    case "darwin":
      return "macos";
    default:
      return platform;
  }
}

function normalizeBuildArch(arch: string): string {
  switch (arch) {
    case "x64":
      return "x86_64";
    case "ia32":
      return "x86";
    case "arm64":
      return "aarch64";
    default:
      return arch;
  }
}

export default defineConfig({
  plugins: [react(), tailwindcss()],
  base: "/",
  define: {
    __APP_VERSION__: JSON.stringify(buildInfo.version),
    __APP_COMMIT__: JSON.stringify(buildInfo.commit),
    __APP_AUTHOR__: JSON.stringify(buildInfo.author),
    __APP_EMAIL__: JSON.stringify(buildInfo.email),
    __APP_HOMEPAGE__: JSON.stringify(buildInfo.homepage),
    __APP_REPOSITORY__: JSON.stringify(buildInfo.repository),
    __APP_OS__: JSON.stringify(buildInfo.buildOs),
    __APP_ARCH__: JSON.stringify(buildInfo.buildArch)
  },
  build: {
    outDir: "dist",
    assetsDir: "assets",
    emptyOutDir: true
  }
});
