import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  assertReleaseSource,
  parseTagCommitOutput,
  resolveCheckedOutTagCommit,
  validateReleaseTag,
} from "./release-source.mjs";

const commit = "0123456789abcdef0123456789abcdef01234567";
const tagObject = "fedcba9876543210fedcba9876543210fedcba98";

test("prefers the peeled commit for an annotated tag", () => {
  assert.equal(
    parseTagCommitOutput(`${tagObject} refs/tags/v0.5.0\n${commit} refs/tags/v0.5.0^{}\n`, "v0.5.0"),
    commit,
  );
});

test("accepts a lightweight tag when no peeled ref exists", () => {
  assert.equal(parseTagCommitOutput(`${commit} refs/tags/v0.5.0\n`, "v0.5.0"), commit);
});

test("rejects a checkout that is different from the release tag", () => {
  assert.throws(
    () =>
      assertReleaseSource({
        tag: "v0.5.0",
        tagCommit: commit,
        checkoutCommit: tagObject,
      }),
    /checkout is/,
  );
});

test("rejects non-semver tags and abbreviated SHAs", () => {
  assert.throws(
    () => assertReleaseSource({ tag: "release", tagCommit: commit, checkoutCommit: commit }),
    /v-prefixed semver/,
  );
  assert.throws(
    () => assertReleaseSource({ tag: "v0.5.0", tagCommit: commit.slice(0, 7), checkoutCommit: commit }),
    /40-character/,
  );
});

test("real repositories bind annotated/lightweight tags and reject missing, mismatched or moved tags", async (t) => {
  const cwd = await mkdtemp(path.join(os.tmpdir(), "vibe-release-source-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));
  const git = (...args) =>
    execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  git("init");
  git("config", "user.name", "Release fixture");
  git("config", "user.email", "fixture@example.invalid");
  git("commit", "--allow-empty", "-m", "tagged source");
  const tagged = git("rev-parse", "HEAD");
  git("tag", "v1.0.0");
  git("tag", "-a", "v1.0.1", "-m", "annotated tag");
  assert.equal(await resolveCheckedOutTagCommit("v1.0.0", cwd), tagged);
  assert.equal(await resolveCheckedOutTagCommit("v1.0.1", cwd), tagged);
  await assert.rejects(resolveCheckedOutTagCommit("v9.0.0", cwd));
  git("clone", "--bare", ".", "remote.git");
  git("remote", "add", "origin", path.join(cwd, "remote.git"));
  assert.equal(await resolveCheckedOutTagCommit("v1.0.1", cwd, "origin"), tagged);
  git("commit", "--allow-empty", "-m", "different dispatch branch");
  await assert.rejects(resolveCheckedOutTagCommit("v1.0.0", cwd), /checkout is/);
  git("tag", "-f", "v1.0.0");
  git("push", "--force", "origin", "refs/tags/v1.0.0");
  git("checkout", "--detach", tagged);
  await assert.rejects(resolveCheckedOutTagCommit("v1.0.0", cwd, "origin"), /checkout is/);
});

test("tag validation rejects ref and shell syntax before invoking git", () => {
  for (const tag of ["v01.0.0", "v1.0.0-01", "v1.0.0-..", "v1.0.0/branch", "--help", "v1.0.0$(id)"]) {
    assert.throws(() => validateReleaseTag(tag), /semver/);
  }
});
