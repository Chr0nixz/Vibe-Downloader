import type { TFunction } from "i18next";

import type { EnvironmentText, EnvironmentTextCode } from "@/generated/bindings";

/**
 * Backend environment copy arrives as a stable `code` plus interpolation
 * `params`, never as pre-rendered English: the Rust side used to inline English
 * sentences that every locale displayed verbatim (FUN-28).
 *
 * `satisfies` makes this table exhaustive — adding a variant to the Rust
 * `EnvironmentTextCode` enum without a key here fails `pnpm typecheck`, and
 * `environment-text.test.ts` proves each key exists in every locale bundle.
 */
export const ENVIRONMENT_TEXT_KEYS = {
  nativeHostReady: "environment.nativeHostReady",
  nativeHostMissing: "environment.nativeHostMissing",
  browserNoneDetected: "environment.browserNoneDetected",
  browserNeedsNativeHost: "environment.browserNeedsNativeHost",
  browserMissingManifests: "environment.browserMissingManifests",
  browserReady: "environment.browserReady",
  browserBridgeOffline: "environment.browserBridgeOffline",
  ffmpegReady: "environment.ffmpegReady",
  ffmpegUnprobeable: "environment.ffmpegUnprobeable",
  ffmpegMissing: "environment.ffmpegMissing",
  proxyDisabled: "environment.proxyDisabled",
  proxySystemUnprobeable: "environment.proxySystemUnprobeable",
  proxyHandshakeOk: "environment.proxyHandshakeOk",
  proxyHandshakeFailed: "environment.proxyHandshakeFailed",
  saveDirWritable: "environment.saveDirWritable",
  saveDirNotWritable: "environment.saveDirNotWritable",
  diskOk: "environment.diskOk",
  diskLow: "environment.diskLow",
  diskCritical: "environment.diskCritical",
  diskUnclassified: "environment.diskUnclassified",
  diskQueryFailed: "environment.diskQueryFailed",
  diskUsage: "environment.diskUsage",
  databaseIntegrityFailed: "environment.databaseIntegrityFailed",
  databaseBackedUp: "environment.databaseBackedUp",
  databaseNoBackup: "environment.databaseNoBackup",
  bridgeConnected: "environment.bridgeConnected",
  bridgeListening: "environment.bridgeListening",
  bridgeUnavailable: "environment.bridgeUnavailable",
  recentHandoffErrors: "environment.recentHandoffErrors",
  proxySystemInherits: "environment.proxySystemInherits",
  fixNativeHostMissing: "environment.fixNativeHostMissing",
  fixNoBrowsersToInstall: "environment.fixNoBrowsersToInstall",
  fixManifestsInstalled: "environment.fixManifestsInstalled",
  fixOpenedPath: "environment.fixOpenedPath",
  fixFocusSection: "environment.fixFocusSection",
  fixChooseBackupDestination: "environment.fixChooseBackupDestination",
  fixCheckFromUpdater: "environment.fixCheckFromUpdater",
} as const satisfies Record<Exclude<EnvironmentTextCode, "raw">, string>;

/** `raw` has no key: it is printed verbatim (paths, versions, probe errors). */
export function environmentTextKey(code: EnvironmentTextCode): string | null {
  return code === "raw" ? null : ENVIRONMENT_TEXT_KEYS[code];
}

/**
 * Renders one backend fragment in the active language. Unknown codes and `raw`
 * fall back to the English source text, so a newer backend never renders a bare
 * key name to the user.
 */
export function formatEnvironmentText(value: EnvironmentText, t: TFunction): string {
  const key = environmentTextKey(value.code);
  if (!key) return value.english;
  const params = Object.fromEntries(
    Object.entries(value.params).filter(([, param]) => param !== null && param !== undefined),
  );
  return t(key, { ...params, defaultValue: value.english });
}

/** Joins an item's detail fragments the way the report and the panel both show them. */
export function formatEnvironmentDetail(detail: EnvironmentText[], t: TFunction): string {
  return detail.map((fragment) => formatEnvironmentText(fragment, t)).join(" ");
}
