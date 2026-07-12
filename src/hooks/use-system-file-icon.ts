import { useEffect, useState } from "react";
import { extractSystemFileIcon } from "@/lib/tauri";

/**
 * Global in-memory cache for system file icons, keyed by file name.
 *
 * The OS icon depends only on the file extension (we use
 * `SHGFI_USEFILEATTRIBUTES` which resolves from the registry association),
 * but keying by file name keeps the lookup trivial and avoids a separate
 * extension-normalization step. The cache survives component unmounts so
 * scrolling the virtualized task list does not re-fetch icons.
 *
 * `null` is a valid cached value meaning "no system icon for this extension".
 * `undefined` means "not yet fetched".
 */
const iconCache = new Map<string, string | null>();

// Track in-flight requests so concurrent rows with the same extension share
// a single IPC call instead of spawning N duplicates on first render.
const inflight = new Map<string, Promise<string | null>>();

function fetchIcon(fileName: string): Promise<string | null> {
  const cached = iconCache.get(fileName);
  if (cached !== undefined) return Promise.resolve(cached);

  const existing = inflight.get(fileName);
  if (existing) return existing;

  const promise = extractSystemFileIcon(fileName)
    .then((result) => {
      const url = result.data_url ?? null;
      iconCache.set(fileName, url);
      inflight.delete(fileName);
      return url;
    })
    .catch(() => {
      // On error, cache null so we don't retry forever.
      iconCache.set(fileName, null);
      inflight.delete(fileName);
      return null;
    });

  inflight.set(fileName, promise);
  return promise;
}

/**
 * React hook that resolves the OS-associated file-type icon for a file name.
 *
 * Returns:
 * - `string` — a `data:image/png;base64,...` URL ready for `<img src>`.
 * - `null`   — the OS has no icon for this extension (or extraction failed).
 *
 * The hook is backed by a global cache keyed by file name, so mounting many
 * rows with the same extension only triggers a single IPC call. This is
 * important for the virtualized task list which can render 50+ rows at once.
 */
export function useSystemFileIcon(fileName: string): string | null {
  const [icon, setIcon] = useState<string | null>(() => iconCache.get(fileName) ?? null);

  useEffect(() => {
    let cancelled = false;
    const cached = iconCache.get(fileName);
    if (cached !== undefined) {
      setIcon(cached);
      return;
    }
    // Start with null while loading; the lucide fallback covers the gap.
    setIcon(null);
    void fetchIcon(fileName).then((url) => {
      if (!cancelled) setIcon(url);
    });
    return () => {
      cancelled = true;
    };
  }, [fileName]);

  return icon;
}
