import path from "node:path";
import { pathToFileURL } from "node:url";

import { validateReleaseTag } from "./release-source.mjs";

// Rebuilding a public tag would expose partially replaced assets even when
// tauri-action is configured to create drafts. Fail before the first upload.
export async function assertReleaseIsPrivate({
  tag,
  repository,
  token,
  apiUrl = "https://api.github.com",
  requireDraft = false,
  fetchImpl = fetch,
}) {
  tag = validateReleaseTag(tag);
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository ?? "")) throw new Error("Invalid release repository.");
  const response = await fetchImpl(`${apiUrl}/repos/${repository}/releases/tags/${encodeURIComponent(tag)}`, {
    headers: { Accept: "application/vnd.github+json", Authorization: `Bearer ${token}` },
    signal: AbortSignal.timeout(30_000),
  });
  if (response.status === 404 && !requireDraft) return;
  if (!response.ok) throw new Error(`Could not verify release draft state (HTTP ${response.status}).`);
  const release = await response.json();
  if (release.draft !== true) throw new Error("Release is already public; its assets must not be overwritten.");
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  await assertReleaseIsPrivate({
    tag: process.env.RELEASE_TAG,
    repository: process.env.GITHUB_REPOSITORY,
    token: process.env.GH_TOKEN,
    apiUrl: process.env.GITHUB_API_URL,
    requireDraft: process.argv.includes("--require-draft"),
  });
}
