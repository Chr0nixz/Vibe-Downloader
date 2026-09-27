import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { parse } from "yaml";

async function workflow(name) {
  return parse(await readFile(new URL(`../.github/workflows/${name}`, import.meta.url), "utf8"));
}

for (const name of ["release.yml", "release-candidate.yml"]) {
  test(`${name}: tagged source, full CI and private assets precede publication`, async () => {
    const { jobs, concurrency, permissions } = await workflow(name);
    // biome-ignore lint/suspicious/noTemplateCurlyInString: GitHub Actions expression, not a JS template literal
    const sha = "${{ needs.preflight.outputs.source_sha }}";
    assert.equal(permissions.contents, "read");
    assert.match(concurrency.group, /^release-/);
    assert.equal(concurrency["cancel-in-progress"], false);
    assert.match(jobs.preflight.steps[0].with.ref, /^refs\/tags\//);
    // biome-ignore lint/suspicious/noTemplateCurlyInString: GitHub Actions expression, not a JS template literal
    assert.equal(jobs.preflight.outputs.source_sha, "${{ steps.source.outputs.sha }}");
    assert.match(jobs.preflight.steps.find((step) => step.id === "source").run, /--remote origin/);
    assert.ok(jobs.preflight.steps.some((step) => step.run === "node scripts/release-state.mjs"));
    assert.equal(jobs.quality.uses, "./.github/workflows/ci.yml");
    assert.equal(jobs.quality.with.source_sha, sha);
    assert.deepEqual(jobs.publish.needs, ["preflight", "quality"]);
    for (const job of [jobs.publish, jobs["build-extensions"], jobs["verify-release"], jobs.promote]) {
      assert.equal(job.steps.find((step) => step.uses?.startsWith("actions/checkout@")).with.ref, sha);
      assert.ok(!job["continue-on-error"]);
      assert.ok(job.steps.every((step) => !step["continue-on-error"]));
    }
    const tauri = jobs.publish.steps.find((step) => step.uses?.startsWith("tauri-apps/tauri-action@"));
    assert.equal(tauri.with.releaseDraft, true);
    assert.equal(tauri.with.prerelease, name.includes("candidate"));
    const verifySteps = jobs["verify-release"].steps;
    const sourceIndex = verifySteps.findIndex((step) => step.run?.includes("SOURCE_COMMIT.txt"));
    const verifyIndex = verifySteps.findIndex((step) => step.run?.includes("verify:release-assets"));
    assert.ok(sourceIndex >= 0 && sourceIndex < verifyIndex);
    assert.match(verifySteps[verifyIndex].run, /--source-sha/);
    // Actions implicitly requires success() here, so failed, cancelled and
    // skipped dependencies cannot expose a partially assembled release.
    assert.deepEqual(jobs.promote.needs, ["preflight", "quality", "publish", "build-extensions", "verify-release"]);
    assert.doesNotMatch(jobs.promote.if, /always\(|failure\(|cancelled\(/);
    assert.match(jobs.promote.if, /release_draft == false/);
    const promote = jobs.promote.steps.map((step) => step.run ?? "").join("\n");
    assert.ok(promote.indexOf("--require-draft") < promote.indexOf("--draft=false"));
    assert.match(promote, /release-source.mjs.*--remote origin/);
    assert.match(promote, name.includes("candidate") ? /--prerelease=true --latest=false/ : /--prerelease=false/);
    const publicSteps = Object.entries(jobs).flatMap(([id, job]) =>
      (job.steps ?? []).filter((step) => step.run?.includes("--draft=false")).map(() => id),
    );
    assert.deepEqual(publicSteps, ["promote"]);
  });
}

test("reusable quality gate checks the exact source on all supported operating systems", async () => {
  const ci = await workflow("ci.yml");
  assert.equal(ci.on.workflow_call.inputs.source_sha.required, true);
  assert.deepEqual(ci.jobs.rust.strategy.matrix.os, ["ubuntu-latest", "windows-latest", "macos-latest"]);
  for (const [id, job] of Object.entries(ci.jobs)) {
    // biome-ignore lint/suspicious/noTemplateCurlyInString: GitHub Actions expression, not a JS template literal
    assert.equal(job.steps[0].with.ref, "${{ inputs.source_sha || github.sha }}");
    assert.ok(job.steps.some((step) => step.run === `pnpm verify:${id}`));
  }
  const rustCommands = ci.jobs.rust.steps.map((step) => step.run ?? "").join("\n");
  assert.match(rustCommands, /cargo deny .*licenses advisories bans sources/);
  assert.match(rustCommands, /pnpm check:bindings/);
});
