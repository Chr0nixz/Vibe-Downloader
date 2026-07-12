import { createHash } from "node:crypto";
import { copyFile, cp, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { zipSync } from "fflate";

import { applyCapturePermissions, resolveExtensionIdentity } from "./release-config.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sourceDir = path.join(root, "browser", "extension-core", "src");
const manifestTemplatePath = path.join(root, "browser", "extension-core", "manifest.template.json");
const distDir = path.join(root, "browser", "dist");
const packageJson = JSON.parse(await readFile(path.join(root, "package.json"), "utf8"));
const identity = resolveExtensionIdentity(process.env, "dev");
const { profile, captureAvailable: experimentalCapture } = identity;

const variants = [
  {
    id: "chromium",
    browserKind: "chrome",
    extensionId: identity.chromeId,
  },
  {
    id: "edge",
    browserKind: "edge",
    extensionId: identity.edgeId,
  },
  {
    id: "firefox",
    browserKind: "firefox",
    firefoxId: identity.firefoxId,
  },
  {
    id: "opera",
    browserKind: "opera",
    extensionId: identity.chromeId,
  },
].filter((variant) => identity.variants.includes(variant.id));

await rm(distDir, { recursive: true, force: true });
await mkdir(distDir, { recursive: true });

const manifestTemplate = JSON.parse(await readFile(manifestTemplatePath, "utf8"));
const backgroundTemplate = await readFile(path.join(sourceDir, "background.js"), "utf8");

async function collectZipEntries(directory, prefix = "") {
  const entries = {};
  const children = await readdir(directory, { withFileTypes: true });
  children.sort((left, right) => left.name.localeCompare(right.name));
  for (const child of children) {
    const relative = prefix ? `${prefix}/${child.name}` : child.name;
    const absolute = path.join(directory, child.name);
    if (child.isDirectory()) {
      Object.assign(entries, await collectZipEntries(absolute, relative));
    } else if (child.isFile()) {
      entries[relative] = [new Uint8Array(await readFile(absolute)), { mtime: new Date("1980-01-01T00:00:00Z") }];
    }
  }
  return entries;
}

async function packageVariant(variant, sourceDirectory, packagesDirectory) {
  const artifactName = `vibe-downloader-${variant.id}-v${packageJson.version}.zip`;
  const artifactPath = path.join(packagesDirectory, artifactName);
  const archive = zipSync(await collectZipEntries(sourceDirectory), { level: 9 });
  await writeFile(artifactPath, archive);
  return {
    browser: variant.id,
    file: artifactName,
    sha256: createHash("sha256").update(archive).digest("hex"),
    bytes: archive.byteLength,
    signed: false,
  };
}

const artifacts = [];
const packagesDirectory = path.join(distDir, "packages");
await mkdir(packagesDirectory, { recursive: true });

for (const variant of variants) {
  const target = path.join(distDir, variant.id);
  await mkdir(target, { recursive: true });

  const manifest = applyCapturePermissions(
    {
      ...manifestTemplate,
      name: variant.id === "firefox" ? "Vibe Downloader (Firefox)" : manifestTemplate.name,
      version: packageJson.version,
    },
    experimentalCapture,
  );
  if (variant.extensionId && identity.chromiumPublicKey) {
    manifest.key = identity.chromiumPublicKey;
  } else {
    delete manifest.key;
  }
  if (variant.firefoxId) {
    manifest.browser_specific_settings = {
      gecko: {
        id: variant.firefoxId,
        strict_min_version: "109.0",
      },
    };
  }

  await writeFile(path.join(target, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  await writeFile(
    path.join(target, "background.js"),
    backgroundTemplate
      .replaceAll("__VIBE_BROWSER_KIND__", variant.browserKind)
      .replaceAll("__VIBE_EXPERIMENTAL_CAPTURE__", experimentalCapture ? "true" : "false"),
  );
  for (const file of [
    "logger.js",
    "popup.html",
    "popup.js",
    "popup.css",
    "options.html",
    "options.js",
    "options.css",
  ]) {
    await copyFile(path.join(sourceDir, file), path.join(target, file));
  }
  await cp(path.join(sourceDir, "_locales"), path.join(target, "_locales"), { recursive: true });
  artifacts.push(await packageVariant(variant, target, packagesDirectory));
}

const metadata = {
  version: packageJson.version,
  profile,
  captureAvailable: experimentalCapture,
  extensionIds: {
    chrome: identity.chromeId,
    edge: identity.edgeId,
    firefox: identity.firefoxId,
  },
  artifacts,
};
await writeFile(path.join(distDir, "build-metadata.json"), `${JSON.stringify(metadata, null, 2)}\n`);
await writeFile(
  path.join(packagesDirectory, "SHA256SUMS.txt"),
  `${artifacts.map((artifact) => `${artifact.sha256}  ${artifact.file}`).join("\n")}\n`,
);
await writeFile(path.join(packagesDirectory, "extension-artifacts.json"), `${JSON.stringify(metadata, null, 2)}\n`);

console.log(
  `Built ${profile} browser extensions (${artifacts.length} packages, capture=${experimentalCapture}) in ${path.relative(root, distDir)}.`,
);
