/**
 * UX-43: the app's own clipboard writes must never re-enter the clipboard
 * monitor. "Copy download URL" used to bounce back as a new-download popup
 * with an automatic network probe one second later. Every app-side write goes
 * through `writeClipboardText`, which registers the text for a short window;
 * the monitor handler drops detections whose URLs came from a registered write.
 *
 * The registry is in-memory only (never persisted) and time-bounded so a
 * user who later copies the same URL from elsewhere still gets the prompt.
 */
const OWN_WRITE_TTL_MS = 5_000;
const MAX_OWN_WRITES = 16;

interface OwnWrite {
  text: string;
  at: number;
}

const ownWrites: OwnWrite[] = [];

/** Write text to the system clipboard and register it as an app-side write. */
export async function writeClipboardText(text: string): Promise<void> {
  registerOwnClipboardWrite(text);
  await navigator.clipboard.writeText(text);
}

/** Fire-and-forget variant for call sites that intentionally ignore errors. */
export function writeClipboardTextQuietly(text: string): void {
  registerOwnClipboardWrite(text);
  navigator.clipboard.writeText(text).catch(() => {});
}

function registerOwnClipboardWrite(text: string): void {
  const now = Date.now();
  pruneExpired(now);
  ownWrites.push({ text, at: now });
  // Bounded even if pruning is skipped by clock weirdness.
  while (ownWrites.length > MAX_OWN_WRITES) ownWrites.shift();
}

function pruneExpired(now: number): void {
  for (let i = ownWrites.length - 1; i >= 0; i -= 1) {
    if (now - ownWrites[i].at > OWN_WRITE_TTL_MS) ownWrites.splice(i, 1);
  }
}

/**
 * True when every detected URL appears inside a recent app-side write — the
 * "Copy download URL" bounce, "copy failed URLs" batch text, etc. A clipboard
 * that mixes app-written and foreign URLs is treated as foreign (conservative:
 * the user can still dismiss the prompt).
 *
 * The backend normalizes each detected URL through `Url::parse().to_string()`
 * (e.g. a bare origin gains a trailing slash), so compare with that trailing
 * slash folded away on both sides to avoid a miss that would re-show a prompt.
 */
export function isOwnClipboardWrite(urls: string[], now: number = Date.now()): boolean {
  if (urls.length === 0) return false;
  pruneExpired(now);
  if (ownWrites.length === 0) return false;
  return urls.every((url) => ownWrites.some((write) => writeMatchesUrl(write.text, url)));
}

function writeMatchesUrl(writeText: string, detectedUrl: string): boolean {
  if (writeText === detectedUrl) return true;
  if (writeText.includes(detectedUrl)) return true;
  const normalized = stripTrailingSlash(detectedUrl);
  return normalized !== detectedUrl && writeText.includes(normalized);
}

function stripTrailingSlash(value: string): string {
  return value.endsWith("/") ? value.slice(0, -1) : value;
}

/** Test helper: clear the registry between cases. */
export function resetOwnClipboardWrites(): void {
  ownWrites.length = 0;
}

/** Test helper: register a write with an explicit timestamp. */
export function registerOwnClipboardWriteForTest(text: string, at: number): void {
  ownWrites.push({ text, at });
}
