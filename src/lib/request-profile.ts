import type { TaskRequestProfileInput, TaskRequestProfileView } from "@/generated/bindings";

export type RequestProfileDraft = { userAgent: string; referer: string; customHeaders: string };
export const EMPTY_REQUEST_PROFILE: RequestProfileDraft = { userAgent: "", referer: "", customHeaders: "" };

function invalidProfile(): never {
  throw JSON.stringify({
    code: "request_profile_invalid",
    message: "Invalid task request profile.",
    recoverable: false,
    actions: [],
  });
}

function hasControlCharacters(value: string): boolean {
  return Array.from(value).some((char) => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127);
}

export function parseRequestProfile(draft: RequestProfileDraft): TaskRequestProfileInput | null {
  const lines = draft.customHeaders.split(/\r?\n/).filter((line) => line.trim());
  if (lines.length > 16) invalidProfile();
  const names = new Set<string>();
  const customHeaders = lines.map((line) => {
    const split = line.indexOf(":");
    if (split < 1) invalidProfile();
    const rawName = line.slice(0, split);
    if (hasControlCharacters(rawName) || rawName.length > 128) invalidProfile();
    const name = rawName.trim().toLowerCase();
    const value = line.slice(split + 1).trim();
    const allowed =
      ["accept", "accept-language", "origin", "dnt", "cache-control", "pragma", "cookie"].includes(name) ||
      (name.startsWith("x-") &&
        !name.startsWith("x-forwarded-") &&
        !name.startsWith("x-proxy-") &&
        name !== "x-real-ip");
    if (
      !allowed ||
      name.length > 128 ||
      !/^[!#$%&'*+.^_`|~0-9a-z-]+$/.test(name) ||
      !value ||
      hasControlCharacters(line.slice(split + 1)) ||
      names.has(name)
    )
      invalidProfile();
    names.add(name);
    return { name, value };
  });
  if (hasControlCharacters(draft.userAgent) || hasControlCharacters(draft.referer)) invalidProfile();
  const userAgent = draft.userAgent.trim() || null;
  const referer = draft.referer.trim() || null;
  if (referer) {
    let url: URL;
    try {
      url = new URL(referer);
    } catch {
      invalidProfile();
    }
    if (!["http:", "https:"].includes(url.protocol) || url.username || url.password || url.hash) invalidProfile();
  }
  const values = [
    ["user-agent", userAgent ?? ""],
    ["referer", referer ?? ""],
    ...customHeaders.map(({ name, value }) => [name, value]),
  ];
  const encoder = new TextEncoder();
  if (
    values.some(([, value]) => encoder.encode(value).length > 8192) ||
    values.reduce((size, [name, value]) => size + encoder.encode(name + value).length, 0) > 16384
  )
    invalidProfile();
  return userAgent || referer || customHeaders.length ? { userAgent, referer, customHeaders } : null;
}

export function requestProfileDraft(view: TaskRequestProfileView): RequestProfileDraft {
  return {
    userAgent: view.userAgent ?? "",
    referer: view.referer ?? "",
    customHeaders: view.customHeaders.map(({ name, value }) => `${name}: ${value}`).join("\n"),
  };
}
