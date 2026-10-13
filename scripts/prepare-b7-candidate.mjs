import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export function b7CandidateConfig(runId) {
  if (!/^[a-z][a-z0-9-]{1,39}$/.test(runId ?? "")) {
    throw new Error("B7 run ID must be 2-40 lowercase letters, digits or hyphens, starting with a letter.");
  }
  return {
    $schema: "https://schema.tauri.app/config/2",
    productName: "Vibe Downloader B7",
    identifier: `com.vibe.downloader.b7.${runId}`,
    build: {
      beforeBuildCommand: "node scripts/native-host-build.mjs --with-frontend",
      beforeBundleCommand: "node scripts/native-host-build.mjs --verify-staged",
    },
    bundle: {
      createUpdaterArtifacts: false,
      externalBin: ["binaries/vibe-native-host"],
      windows: { nsis: { installMode: "currentUser" } },
    },
  };
}

export async function prepareB7Candidate(runId, workspaceRoot = root) {
  const config = b7CandidateConfig(runId);
  const directory = path.join(workspaceRoot, "logs", "b7", runId);
  await mkdir(directory, { recursive: true });
  const output = path.join(directory, "tauri.b7.generated.json");
  await writeFile(output, `${JSON.stringify(config, null, 2)}\n`, { flag: "wx" });
  return { output, identifier: config.identifier };
}

async function main() {
  const args = process.argv.slice(2);
  if (args.length !== 2 || args[0] !== "--run-id") {
    throw new Error("Usage: node scripts/prepare-b7-candidate.mjs --run-id <unique-run-id>");
  }
  const result = await prepareB7Candidate(args[1]);
  console.log(JSON.stringify({ ...result, updaterArtifacts: false }, null, 2));
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  await main();
}
