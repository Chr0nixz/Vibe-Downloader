import assert from "node:assert/strict";
import test from "node:test";
import { assertReleaseIsPrivate } from "./release-state.mjs";

function check(status, body, requireDraft = false) {
  return assertReleaseIsPrivate({
    tag: "v0.5.0",
    repository: "example/project",
    token: "fixture",
    requireDraft,
    fetchImpl: async () => new Response(JSON.stringify(body), { status }),
  });
}

test("new releases and existing drafts are allowed before builds", async () => {
  await check(404, {});
  await check(200, { draft: true });
});

test("public releases and API failures fail before any asset mutation", async () => {
  await assert.rejects(check(200, { draft: false }), /already public/);
  for (const status of [401, 403, 500]) await assert.rejects(check(status, {}), /verify release draft state/);
});

test("promotion requires an existing draft", async () => {
  await assert.rejects(check(404, {}, true), /HTTP 404/);
  await check(200, { draft: true }, true);
});
