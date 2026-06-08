import { copyFile, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sourceDir = path.join(root, "browser", "extension-core", "src");
const manifestTemplatePath = path.join(
  root,
  "browser",
  "extension-core",
  "manifest.template.json",
);
const distDir = path.join(root, "browser", "dist");
const packageJson = JSON.parse(await readFile(path.join(root, "package.json"), "utf8"));
const profile = process.env.VIBE_BROWSER_PROFILE === "release" ? "release" : "dev";

const variants = [
  {
    id: "chromium",
    browserKind: "chrome",
    extensionId:
      profile === "release"
        ? "replace-with-chrome-web-store-id"
        : "abcdefghijklmnopabcdefghijklmnop",
  },
  {
    id: "edge",
    browserKind: "edge",
    extensionId:
      profile === "release"
        ? "replace-with-edge-addons-id"
        : "abcdefghijklmnopabcdefghijklmnop",
  },
  {
    id: "firefox",
    browserKind: "firefox",
    firefoxId:
      profile === "release"
        ? "vibe-downloader@example.invalid"
        : "vibe-downloader@local",
  },
  {
    id: "opera",
    browserKind: "opera",
    extensionId:
      profile === "release"
        ? "replace-with-opera-addons-id"
        : "abcdefghijklmnopabcdefghijklmnop",
  },
];

await rm(distDir, { recursive: true, force: true });
await mkdir(distDir, { recursive: true });

const manifestTemplate = JSON.parse(await readFile(manifestTemplatePath, "utf8"));
const backgroundTemplate = await readFile(path.join(sourceDir, "background.js"), "utf8");

for (const variant of variants) {
  const target = path.join(distDir, variant.id);
  await mkdir(target, { recursive: true });

  const manifest = {
    ...manifestTemplate,
    name: variant.id === "firefox" ? "Vibe Downloader (Firefox)" : manifestTemplate.name,
    version: packageJson.version,
  };
  if (variant.extensionId) {
    manifest.key = undefined;
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
    backgroundTemplate.replaceAll("__VIBE_BROWSER_KIND__", variant.browserKind),
  );
  for (const file of ["logger.js", "popup.html", "popup.js", "popup.css"]) {
    await copyFile(path.join(sourceDir, file), path.join(target, file));
  }
}

console.log(`Built browser extensions in ${path.relative(root, distDir)}`);
