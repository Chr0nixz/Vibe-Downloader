import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const policyPath = path.join(root, "browser", "extension-core", "src", "capture-policy.js");

function loadPolicy() {
  const code = readFileSync(policyPath, "utf8");
  const context = { URL, console };
  vm.createContext(context);
  vm.runInContext(code, context);
  return context.VibeCapturePolicy;
}

const policy = loadPolicy();

test("FUN-14: always / never / ask header modes", () => {
  const url = "https://cdn.example.com/video.mp4";
  assert.equal(policy.headerForwardingDecision(url, { forwardHeadersMode: "enabled", siteRules: [] }).forward, true);
  assert.equal(policy.headerForwardingDecision(url, { forwardHeadersMode: "disabled", siteRules: [] }).forward, false);
  const ask = policy.headerForwardingDecision(url, { forwardHeadersMode: "ask", siteRules: [] });
  assert.equal(ask.forward, false);
  assert.equal(ask.state, "ask");
});

test("FUN-14: site rule priority overrides global header mode", () => {
  const url = "https://auth.example.com/file.bin";
  const rules = [
    {
      hostPattern: "auth.example.com",
      includeSubdomains: false,
      forwardHeaders: true,
      mode: "auto",
    },
  ];
  assert.equal(
    policy.headerForwardingDecision(url, { forwardHeadersMode: "disabled", siteRules: rules }).forward,
    true,
  );
  assert.equal(
    policy.headerForwardingDecision(url, {
      forwardHeadersMode: "enabled",
      siteRules: [{ ...rules[0], forwardHeaders: false }],
    }).forward,
    false,
  );
});

test("FUN-14: ask capture mode never intercepts and never prompts", () => {
  const download = { url: "https://example.com/a.mp4", totalBytes: 10_000_000, filename: "a.mp4" };
  assert.equal(
    policy.shouldIntercept(download, {
      autoIntercept: true,
      minSizeBytes: "0",
      fileExtensions: ["mp4"],
      siteRules: [{ hostPattern: "example.com", includeSubdomains: true, mode: "ask" }],
    }).reason,
    "ask-rule",
  );
  assert.equal(
    policy.shouldIntercept(download, {
      autoIntercept: true,
      minSizeBytes: "0",
      fileExtensions: ["mp4"],
      siteRules: [{ hostPattern: "example.com", includeSubdomains: true, mode: "never" }],
    }).reason,
    "site-rule",
  );
  assert.equal(
    policy.shouldIntercept(download, {
      autoIntercept: true,
      minSizeBytes: "0",
      fileExtensions: ["mp4"],
      siteRules: [{ hostPattern: "example.com", includeSubdomains: true, mode: "auto" }],
    }).intercept,
    true,
  );
});
