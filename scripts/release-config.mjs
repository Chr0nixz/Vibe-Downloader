import { createHash } from "node:crypto";

export const CANDIDATE_CHROMIUM_PUBLIC_KEY =
  "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAnzbGr4WRbECji+lvxIQXGmL+oTLh0ErT02tiYsam6DTPsaTxmTRaeUvrfjQEPLDNDbdTEBF5IsP0txSoU2BpLdyMOvjgwYspG6vptreDbEwozqqV4ZiM2pKMCEvfXWzpCQPBxqR0JaMonWmK5909iv5tQz2ynsa0o43qyid6VFF9y8o4YVADUck3RLqiRSykvnnvJGiXYuDFlNYRAJhE8rj0UBkA0oeqWYLWAJicagfNnRKpxeccdXkSd1eSSbaQ1y4hkVDissCPAxe9Yl1kwKNFc6RYcQ/98HvuMSnul/eGg/z0Ob+KfovgIsU3Hiubrl6MUHBNJFkj6X2sMQ/inwIDAQAB";
export const CANDIDATE_CHROMIUM_EXTENSION_ID = "fcjcenonhhfdblnoafphpcddpmppdeag";
export const CANDIDATE_FIREFOX_EXTENSION_ID = "vibe-downloader-candidate@local";

const CHROMIUM_ID_PATTERN = /^[a-p]{32}$/;
const TRUTHY = new Set(["1", "true", "yes", "on"]);

export function extensionIdFromPublicKey(publicKey) {
  const der = Buffer.from(publicKey, "base64");
  if (der.length === 0 || der.toString("base64") !== publicKey) {
    throw new Error("Candidate Chromium public key is not canonical base64 DER.");
  }
  return [...createHash("sha256").update(der).digest().subarray(0, 16)]
    .flatMap((byte) => [byte >> 4, byte & 0x0f])
    .map((nibble) => String.fromCharCode(97 + nibble))
    .join("");
}

export function parseBoolean(value) {
  return TRUTHY.has(
    String(value ?? "")
      .trim()
      .toLowerCase(),
  );
}

export function resolveBrowserProfile(value, fallback = "dev") {
  const profile = String(value ?? fallback)
    .trim()
    .toLowerCase();
  if (!["dev", "candidate", "release"].includes(profile)) {
    throw new Error(`Unsupported VIBE_BROWSER_PROFILE: ${profile || "(empty)"}.`);
  }
  return profile;
}

function requireChromiumId(label, value) {
  const id = String(value ?? "").trim();
  if (!CHROMIUM_ID_PATTERN.test(id)) {
    throw new Error(`${label} must be a 32-character Chromium extension ID using letters a-p.`);
  }
  return id;
}

function requireFirefoxId(value) {
  const id = String(value ?? "").trim();
  if (!id || /\s/.test(id) || id === "vibe-downloader@example.invalid") {
    throw new Error("VIBE_FIREFOX_EXTENSION_ID must be a non-placeholder Firefox extension ID.");
  }
  if (!(id.includes("@") || /^\{[0-9a-f-]+\}$/i.test(id))) {
    throw new Error("VIBE_FIREFOX_EXTENSION_ID must be an email-like ID or braced UUID.");
  }
  return id;
}

function hasConfiguredValue(value) {
  return String(value ?? "").trim().length > 0;
}

function candidateExtensionIdentity(profile, captureAvailable, usingCandidateFallback = false) {
  return {
    profile,
    captureAvailable,
    usingCandidateFallback,
    chromeId: CANDIDATE_CHROMIUM_EXTENSION_ID,
    edgeId: CANDIDATE_CHROMIUM_EXTENSION_ID,
    firefoxId: CANDIDATE_FIREFOX_EXTENSION_ID,
    chromiumPublicKey: CANDIDATE_CHROMIUM_PUBLIC_KEY,
    variants: profile === "dev" ? ["chromium", "edge", "firefox", "opera"] : ["chromium", "edge", "firefox"],
  };
}

export function resolveExtensionIdentity(env = process.env, fallbackProfile = "dev") {
  const profile = resolveBrowserProfile(env.VIBE_BROWSER_PROFILE, fallbackProfile);
  const captureRequested = parseBoolean(env.VIBE_BROWSER_EXPERIMENTAL_CAPTURE);
  if (profile !== "dev" && captureRequested) {
    throw new Error(`Experimental capture cannot be enabled for the ${profile} profile.`);
  }

  if (profile === "release") {
    const formalConfigured = [
      env.VIBE_CHROME_EXTENSION_ID,
      env.VIBE_EDGE_EXTENSION_ID,
      env.VIBE_FIREFOX_EXTENSION_ID,
    ].some(hasConfiguredValue);

    if (!formalConfigured && parseBoolean(env.VIBE_ALLOW_CANDIDATE_EXTENSION_IDS)) {
      return candidateExtensionIdentity(profile, false, true);
    }

    return {
      profile,
      captureAvailable: false,
      usingCandidateFallback: false,
      chromeId: requireChromiumId("VIBE_CHROME_EXTENSION_ID", env.VIBE_CHROME_EXTENSION_ID),
      edgeId: requireChromiumId("VIBE_EDGE_EXTENSION_ID", env.VIBE_EDGE_EXTENSION_ID),
      firefoxId: requireFirefoxId(env.VIBE_FIREFOX_EXTENSION_ID),
      chromiumPublicKey: null,
      variants: ["chromium", "edge", "firefox"],
    };
  }

  return candidateExtensionIdentity(profile, profile === "dev" && captureRequested);
}

export function applyCapturePermissions(manifest, captureAvailable) {
  if (!captureAvailable) {
    const { host_permissions: _hostPermissions, ...rest } = manifest;
    return {
      ...rest,
      permissions: (manifest.permissions ?? []).filter(
        (permission) => !["downloads", "cookies", "webRequest"].includes(permission),
      ),
    };
  }
  return {
    ...manifest,
    permissions: Array.from(new Set([...(manifest.permissions ?? []), "downloads", "cookies", "webRequest"])),
    host_permissions: ["http://*/*", "https://*/*"],
  };
}
