import { spawn } from "node:child_process";
import { chmod, copyFile, mkdir, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const TARGETS = new Map([
  ["windows:x86_64", "x86_64-pc-windows-msvc"],
  ["macos:x86_64", "x86_64-apple-darwin"],
  ["macos:aarch64", "aarch64-apple-darwin"],
  ["linux:x86_64", "x86_64-unknown-linux-gnu"],
]);

function normalizePlatform(value) {
  const platform = String(value ?? "")
    .trim()
    .toLowerCase();
  if (["windows", "win32"].includes(platform)) return "windows";
  if (["macos", "darwin"].includes(platform)) return "macos";
  if (platform === "linux") return "linux";
  return platform;
}

function normalizeArch(value) {
  const arch = String(value ?? "")
    .trim()
    .toLowerCase();
  if (["x64", "x86_64"].includes(arch)) return "x86_64";
  if (["arm64", "aarch64"].includes(arch)) return "aarch64";
  return arch;
}

export function resolveTargetTriple(platform = process.env.TAURI_ENV_PLATFORM, arch = process.env.TAURI_ENV_ARCH) {
  const normalizedPlatform = normalizePlatform(platform || process.platform);
  const normalizedArch = normalizeArch(arch || process.arch);
  const triple = TARGETS.get(`${normalizedPlatform}:${normalizedArch}`);
  if (!triple) {
    throw new Error(
      `Unsupported native-host bundle target: platform=${normalizedPlatform || "unknown"}, arch=${normalizedArch || "unknown"}.`,
    );
  }
  return triple;
}

export function nativeHostArtifactPaths(targetTriple, workspaceRoot = root) {
  const windows = targetTriple.endsWith("-windows-msvc");
  const fileName = windows ? "vibe-native-host.exe" : "vibe-native-host";
  const stagedName = windows ? `vibe-native-host-${targetTriple}.exe` : `vibe-native-host-${targetTriple}`;
  return {
    source: path.join(workspaceRoot, "src-tauri", "target", targetTriple, "release", fileName),
    staged: path.join(workspaceRoot, "src-tauri", "binaries", stagedName),
    stagedName,
  };
}

export function nativeHostCargoEnvironment(env = process.env) {
  const cargoEnv = { ...env };
  delete cargoEnv.TAURI_CONFIG;
  return cargoEnv;
}

export async function verifyStagedNativeHost({ targetTriple = resolveTargetTriple(), workspaceRoot = root } = {}) {
  const paths = nativeHostArtifactPaths(targetTriple, workspaceRoot);
  const stagedInfo = await stat(paths.staged).catch(() => null);
  if (!stagedInfo?.isFile() || stagedInfo.size === 0) {
    throw new Error(`Staged native host is missing or empty: ${path.relative(workspaceRoot, paths.staged)}.`);
  }
  if (!targetTriple.endsWith("-windows-msvc") && (stagedInfo.mode & 0o111) === 0) {
    throw new Error(`Staged native host is not executable: ${path.relative(workspaceRoot, paths.staged)}.`);
  }
  return { targetTriple, ...paths, bytes: stagedInfo.size };
}

function run(command, args, cwd, env = process.env) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, env, stdio: "inherit", shell: false });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`${command} exited with ${code ?? signal ?? "unknown status"}.`));
      }
    });
  });
}

export async function prepareNativeHost({ targetTriple = resolveTargetTriple(), workspaceRoot = root } = {}) {
  const paths = nativeHostArtifactPaths(targetTriple, workspaceRoot);
  await run(
    "cargo",
    [
      "build",
      "--manifest-path",
      "src-tauri/Cargo.toml",
      "--release",
      "--bin",
      "vibe-native-host",
      "--target",
      targetTriple,
    ],
    workspaceRoot,
    nativeHostCargoEnvironment(),
  );

  const sourceInfo = await stat(paths.source).catch(() => null);
  if (!sourceInfo?.isFile()) {
    throw new Error(`Native host build did not produce ${path.relative(workspaceRoot, paths.source)}.`);
  }

  await mkdir(path.dirname(paths.staged), { recursive: true });
  await copyFile(paths.source, paths.staged);
  if (!targetTriple.endsWith("-windows-msvc")) {
    await chmod(paths.staged, 0o755);
  }

  return verifyStagedNativeHost({ targetTriple, workspaceRoot });
}

async function main() {
  const targetIndex = process.argv.indexOf("--target");
  const targetTriple = targetIndex >= 0 ? process.argv[targetIndex + 1] : undefined;
  if (targetIndex >= 0 && !targetTriple) {
    throw new Error("--target requires a Rust target triple.");
  }
  if (process.argv.includes("--verify-staged")) {
    const result = await verifyStagedNativeHost({ targetTriple: targetTriple ?? resolveTargetTriple() });
    console.log(`Verified ${path.relative(root, result.staged)} (${result.bytes} bytes) for ${result.targetTriple}.`);
    return;
  }
  if (process.argv.includes("--with-frontend")) {
    await run(process.execPath, [path.join(root, "node_modules", "typescript", "bin", "tsc")], root);
    await run(process.execPath, [path.join(root, "node_modules", "vite", "bin", "vite.js"), "build"], root);
  }
  const result = await prepareNativeHost({ targetTriple: targetTriple ?? resolveTargetTriple() });
  console.log(`Prepared ${path.relative(root, result.staged)} (${result.bytes} bytes) for ${result.targetTriple}.`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  await main();
}
