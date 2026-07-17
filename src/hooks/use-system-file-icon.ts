import { useEffect, useState } from "react";
import {
  SYSTEM_FILE_ICON_NO_EXTENSION_KEY,
  SystemFileIconCache,
  systemFileIconCacheKey,
} from "@/lib/system-file-icon-cache";
import { extractSystemFileIcon } from "@/lib/tauri";

/**
 * Global bounded cache for system file icons, keyed by normalized extension.
 *
 * The OS icon depends only on the file extension (we use
 * `SHGFI_USEFILEATTRIBUTES` which resolves from the registry association),
 * The cache survives component unmounts so scrolling the virtualized task
 * list does not re-fetch icons, while its LRU bound prevents long sessions
 * with unusual extensions from retaining data URLs indefinitely.
 *
 * `null` is a valid cached value meaning "no system icon for this extension".
 * `undefined` means "not yet fetched".
 */
const iconCache = new SystemFileIconCache();

// Track in-flight requests so concurrent rows with the same extension share
// a single IPC call instead of spawning N duplicates on first render.
const inflight = new Map<string, Promise<string | null>>();

function fetchIcon(cacheKey: string): Promise<string | null> {
  const cached = iconCache.get(cacheKey);
  if (cached !== undefined) return Promise.resolve(cached);
  if (cacheKey === SYSTEM_FILE_ICON_NO_EXTENSION_KEY) {
    iconCache.set(cacheKey, null);
    return Promise.resolve(null);
  }

  const existing = inflight.get(cacheKey);
  if (existing) return existing;

  const promise = extractSystemFileIcon(`file${cacheKey}`)
    .then((result) => {
      const url = result.data_url ?? null;
      iconCache.set(cacheKey, url);
      inflight.delete(cacheKey);
      return url;
    })
    .catch(() => {
      // On error, cache null so we don't retry forever.
      iconCache.set(cacheKey, null);
      inflight.delete(cacheKey);
      return null;
    });

  inflight.set(cacheKey, promise);
  return promise;
}

/**
 * React hook that resolves the OS-associated file-type icon for a file name.
 *
 * Returns:
 * - `string` — a `data:image/png;base64,...` URL ready for `<img src>`.
 * - `null`   — the OS has no icon for this extension (or extraction failed).
 *
 * The hook is backed by a global cache keyed by extension, so mounting many
 * rows with the same extension only triggers a single IPC call. This is
 * important for the virtualized task list which can render 50+ rows at once.
 */
export function useSystemFileIcon(fileName: string): string | null {
  const cacheKey = systemFileIconCacheKey(fileName);
  const [icon, setIcon] = useState<string | null>(() => iconCache.get(cacheKey) ?? null);

  useEffect(() => {
    let cancelled = false;
    const cached = iconCache.get(cacheKey);
    if (cached !== undefined) {
      setIcon(cached);
      return;
    }
    // Start with null while loading; the lucide fallback covers the gap.
    setIcon(null);
    void fetchIcon(cacheKey).then((url) => {
      if (!cancelled) setIcon(url);
    });
    return () => {
      cancelled = true;
    };
  }, [cacheKey]);

  return icon;
}
