import { execFile } from "node:child_process";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

export function validateReleaseTag(tag) {
  const normalized = String(tag ?? "").trim();
  if (
    !/^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*)?$/.test(
      normalized,
    )
  ) {
    throw new Error("Release tag must be v-prefixed semver.");
  }
  return normalized;
}

export function parseTagCommitOutput(output, tag) {
  const refs = String(output ?? "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const [sha, ref] = line.split(/\s+/);
      return { sha, ref };
    });
  const peeled = refs.find((entry) => entry.ref === `refs/tags/${tag}^{}`);
  const direct = refs.find((entry) => entry.ref === `refs/tags/${tag}`);
  const commit = peeled?.sha ?? direct?.sha;
  if (!commit || !/^[0-9a-f]{40}$/i.test(commit)) {
    throw new Error(`Release tag ${tag} does not resolve to a commit.`);
  }
  return commit.toLowerCase();
}

export function assertReleaseSource({ tag, tagCommit, checkoutCommit }) {
  const normalizedTag = validateReleaseTag(tag);
  const normalizedTagCommit = String(tagCommit ?? "")
    .trim()
    .toLowerCase();
  const normalizedCheckoutCommit = String(checkoutCommit ?? "")
    .trim()
    .toLowerCase();
  if (!/^[0-9a-f]{40}$/.test(normalizedTagCommit) || !/^[0-9a-f]{40}$/.test(normalizedCheckoutCommit)) {
    throw new Error("Release tag and checkout must resolve to full 40-character commit SHAs.");
  }
  if (normalizedTagCommit !== normalizedCheckoutCommit) {
    throw new Error(
      `Release tag ${normalizedTag} resolves to ${normalizedTagCommit}, but checkout is ${normalizedCheckoutCommit}.`,
    );
  }
  return normalizedCheckoutCommit;
}

export async function resolveCheckedOutTagCommit(tag, cwd = process.cwd(), remote = null) {
  tag = validateReleaseTag(tag);
  const args = remote
    ? ["ls-remote", "--exit-code", "--tags", remote, `refs/tags/${tag}`, `refs/tags/${tag}^{}`]
    : ["show-ref", "--dereference", "--tags", tag];
  const { stdout: tagOutput } = await execFileAsync("git", args, { cwd });
  const tagCommit = parseTagCommitOutput(tagOutput, tag);
  const { stdout: checkoutOutput } = await execFileAsync("git", ["rev-parse", "HEAD"], { cwd });
  const checkoutCommit = checkoutOutput.trim().toLowerCase();
  return assertReleaseSource({ tag, tagCommit, checkoutCommit });
}

async function main() {
  const index = process.argv.indexOf("--tag");
  const tag = index >= 0 ? process.argv[index + 1] : process.env.RELEASE_TAG;
  const remoteIndex = process.argv.indexOf("--remote");
  const remote = remoteIndex >= 0 ? process.argv[remoteIndex + 1] : null;
  const sha = await resolveCheckedOutTagCommit(tag, process.cwd(), remote);
  if (process.env.GITHUB_OUTPUT) {
    const { appendFile } = await import("node:fs/promises");
    await appendFile(process.env.GITHUB_OUTPUT, `sha=${sha}\n`);
  }
  console.log(sha);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  await main();
}
