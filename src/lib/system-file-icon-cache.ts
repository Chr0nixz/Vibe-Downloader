export const SYSTEM_FILE_ICON_CACHE_CAPACITY = 256;
export const SYSTEM_FILE_ICON_NO_EXTENSION_KEY = "__no_extension__";

export function systemFileIconCacheKey(fileName: string): string {
  const pathParts = fileName.trim().replace(/\\/gu, "/").split("/");
  const baseName = pathParts[pathParts.length - 1]?.toLowerCase() ?? "";
  const dotIndex = baseName.lastIndexOf(".");
  if (dotIndex <= 0 || dotIndex === baseName.length - 1) return SYSTEM_FILE_ICON_NO_EXTENSION_KEY;
  return baseName.slice(dotIndex);
}

export class SystemFileIconCache {
  readonly #entries = new Map<string, string | null>();

  constructor(readonly capacity = SYSTEM_FILE_ICON_CACHE_CAPACITY) {
    if (!Number.isInteger(capacity) || capacity < 1) throw new Error("Icon cache capacity must be a positive integer");
  }

  get(key: string): string | null | undefined {
    const value = this.#entries.get(key);
    if (value === undefined && !this.#entries.has(key)) return undefined;
    this.#entries.delete(key);
    this.#entries.set(key, value ?? null);
    return value ?? null;
  }

  set(key: string, value: string | null): void {
    this.#entries.delete(key);
    this.#entries.set(key, value);
    while (this.#entries.size > this.capacity) {
      const oldest = this.#entries.keys().next().value;
      if (oldest === undefined) break;
      this.#entries.delete(oldest);
    }
  }

  get size(): number {
    return this.#entries.size;
  }
}
