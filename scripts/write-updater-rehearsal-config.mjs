import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_REPOSITORY = "Chr0nixz/Vibe-Downloader";

export function updaterRehearsalEndpoint(tag, repository = DEFAULT_REPOSITORY) {
  const normalizedTag = String(tag ?? "").trim();
  if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(normalizedTag)) {
    throw new Error(`Updater rehearsal tag must be v-prefixed semver; received ${normalizedTag || "(empty)"}.`);
  }
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) {
    throw new Error(`Invalid GitHub repository: ${repository}.`);
  }
  return `https://github.com/${repository}/releases/download/${encodeURIComponent(normalizedTag)}/latest.json`;
}

export function updaterRehearsalConfig(tag, repository = DEFAULT_REPOSITORY) {
  return {
    build: {
      beforeBuildCommand: "node scripts/native-host-build.mjs --with-frontend",
      beforeBundleCommand: "node scripts/native-host-build.mjs",
    },
    bundle: {
      externalBin: ["binaries/vibe-native-host"],
    },
    plugins: {
      updater: {
        endpoints: [updaterRehearsalEndpoint(tag, repository)],
      },
    },
  };
}

async function main() {
  const tagIndex = process.argv.indexOf("--tag");
  const outputIndex = process.argv.indexOf("--output");
  const repositoryIndex = process.argv.indexOf("--repository");
  const tag = tagIndex >= 0 ? process.argv[tagIndex + 1] : undefined;
  const repository = repositoryIndex >= 0 ? process.argv[repositoryIndex + 1] : DEFAULT_REPOSITORY;
  const output = path.resolve(
    root,
    outputIndex >= 0 ? process.argv[outputIndex + 1] : "src-tauri/tauri.updater-rehearsal.generated.json",
  );
  const config = updaterRehearsalConfig(tag, repository);
  await mkdir(path.dirname(output), { recursive: true });
  await writeFile(output, `${JSON.stringify(config, null, 2)}\n`);
  console.log(`Wrote updater rehearsal config for ${tag}: ${path.relative(root, output)}.`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  await main();
}
