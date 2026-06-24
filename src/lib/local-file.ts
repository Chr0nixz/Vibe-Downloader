/**
 * Shared helpers for resolving local files dropped onto the window or picked
 * via the native file dialog. Kept here so the New Download dialog and the
 * drag-and-drop entry point in AppShell stay in sync.
 *
 * Supported file kinds:
 *  - torrent  -> converted to a `file://` URL and probed by the BT engine
 *  - metalink -> converted to a `file://` URL and probed by the Metalink engine
 *  - dash     -> converted to a `file://` URL and probed by the DASH engine
 *  - text     -> read as text and used as batch URL input
 */

export type LocalFileKind = "torrent" | "metalink" | "dash" | "text";

const MANIFEST_EXTENSIONS = ["torrent", "meta4", "metalink", "mpd", "txt"];

export function getLocalFileKind(name: string): LocalFileKind {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (ext === "torrent") return "torrent";
  if (ext === "meta4" || ext === "metalink") return "metalink";
  if (ext === "mpd") return "dash";
  return "text";
}

/** True when a file name matches a manifest/text kind the app can ingest. */
export function isSupportedLocalFile(name: string): boolean {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  return MANIFEST_EXTENSIONS.includes(ext);
}

function encodePathSegments(path: string): string {
  return path
    .split("/")
    .map((segment) => encodeURIComponent(segment))
    .join("/");
}

/**
 * Convert a native file path to a `file://` URL. Mirrors the encoding the
 * Rust side expects when probing `file://*.torrent` / `.metalink` / `.mpd`.
 */
export function pathToFileUrl(filePath: string): string {
  const normalized = filePath.replace(/\\/g, "/");
  if (/^[A-Za-z]:\//.test(normalized)) {
    const drive = normalized.slice(0, 2);
    return `file:///${drive}${encodePathSegments(normalized.slice(2))}`;
  }
  if (normalized.startsWith("//")) {
    const [host = "", ...segments] = normalized.slice(2).split("/");
    return `file://${encodeURIComponent(host)}/${segments.map((segment) => encodeURIComponent(segment)).join("/")}`;
  }
  return `file://${encodePathSegments(normalized.startsWith("/") ? normalized : `/${normalized}`)}`;
}

/** Read a local file as UTF-8 text via the Tauri fs plugin. */
export function readFileAsText(filePath: string): Promise<string> {
  return import("@tauri-apps/plugin-fs").then(({ readTextFile }) => readTextFile(filePath));
}

/** Resolve a dropped/picked local file into inputs the New Download dialog understands. */
export async function resolveLocalFile(
  filePath: string,
  name: string,
): Promise<{ kind: LocalFileKind; url?: string; batchInput?: string }> {
  const kind = getLocalFileKind(name);
  if (kind === "torrent" || kind === "metalink" || kind === "dash") {
    return { kind, url: pathToFileUrl(filePath) };
  }
  const text = await readFileAsText(filePath);
  return { kind, batchInput: text };
}
