/**
 * FUN-17: Shared create-draft fields for single-task and batch import paths.
 * Keeps credential/proxy/hash/priority/category/speed overrides in one contract
 * so UI cannot leave backend-supported fields permanently unreachable.
 */

import type {
  ChecksumAlgorithm,
  CreateTaskInput,
  DirectoryProbeInput,
  ImportUrlsInput,
  ProbeTaskInput,
  TaskPriority,
  TaskProxyMode,
} from "@/generated/bindings";

export type CreateDraftShared = {
  username: string | null;
  password: string | null;
  privateKeyData: string | null;
  privateKeyPassphrase: string | null;
  expectedHash: string | null;
  expectedHashAlgorithm: ChecksumAlgorithm | null;
  expectedHashSha256: string | null;
  taskSpeedLimitBps: string | null;
  priority: TaskPriority | null;
  categoryKey: string | null;
  allowDuplicate: boolean | null;
  proxyMode: TaskProxyMode | null;
  proxyUrl: string | null;
  proxyUsername: string | null;
  proxyPassword: string | null;
  proxyNoProxy: string | null;
};

export const EMPTY_CREATE_DRAFT: CreateDraftShared = {
  username: null,
  password: null,
  privateKeyData: null,
  privateKeyPassphrase: null,
  expectedHash: null,
  expectedHashAlgorithm: null,
  expectedHashSha256: null,
  taskSpeedLimitBps: null,
  priority: null,
  categoryKey: null,
  allowDuplicate: null,
  proxyMode: null,
  proxyUrl: null,
  proxyUsername: null,
  proxyPassword: null,
  proxyNoProxy: null,
};

/** Credential + proxy subset used by probe and directory probe. */
export function draftAuthProxyFields(draft: CreateDraftShared) {
  return {
    username: draft.username,
    password: draft.password,
    privateKeyData: draft.privateKeyData,
    privateKeyPassphrase: draft.privateKeyPassphrase,
    proxyMode: draft.proxyMode,
    proxyUrl: draft.proxyUrl,
    proxyUsername: draft.proxyUsername,
    proxyPassword: draft.proxyPassword,
    proxyNoProxy: draft.proxyNoProxy,
  };
}

export function toProbeTaskInput(url: string, draft: CreateDraftShared, requestId?: string | null): ProbeTaskInput {
  return {
    url,
    requestId: requestId ?? null,
    ...draftAuthProxyFields(draft),
  };
}

export function toDirectoryProbeInput(url: string, draft: CreateDraftShared): DirectoryProbeInput {
  return {
    url,
    ...draftAuthProxyFields(draft),
  };
}

export function toImportUrlsInput(
  input: string,
  saveDir: string | null,
  create: boolean,
  draft: CreateDraftShared,
): ImportUrlsInput {
  return {
    input,
    saveDir,
    probe: true,
    create,
    expectedHashSha256: draft.expectedHashSha256,
    expectedHash: draft.expectedHash,
    expectedHashAlgorithm: draft.expectedHashAlgorithm,
    taskSpeedLimitBps: draft.taskSpeedLimitBps,
    priority: draft.priority,
    categoryKey: draft.categoryKey,
    allowDuplicate: draft.allowDuplicate ?? false,
    ...draftAuthProxyFields(draft),
  };
}

export function applyDraftToCreateTaskInput(
  base: Omit<CreateTaskInput, keyof CreateDraftShared>,
  draft: CreateDraftShared,
): CreateTaskInput {
  return {
    ...base,
    expectedHashSha256: draft.expectedHashSha256,
    expectedHash: draft.expectedHash,
    expectedHashAlgorithm: draft.expectedHashAlgorithm,
    taskSpeedLimitBps: draft.taskSpeedLimitBps,
    priority: draft.priority,
    categoryKey: draft.categoryKey,
    allowDuplicate: draft.allowDuplicate,
    username: draft.username,
    password: draft.password,
    privateKeyData: draft.privateKeyData,
    privateKeyPassphrase: draft.privateKeyPassphrase,
    proxyMode: draft.proxyMode,
    proxyUrl: draft.proxyUrl,
    proxyUsername: draft.proxyUsername,
    proxyPassword: draft.proxyPassword,
    proxyNoProxy: draft.proxyNoProxy,
  };
}

/** Build expected-hash draft fields from UI algorithm + digest inputs. */
export function draftHashFields(
  expectedHash: string,
  algorithm: ChecksumAlgorithm,
  skipHash: boolean,
): Pick<CreateDraftShared, "expectedHash" | "expectedHashAlgorithm" | "expectedHashSha256"> {
  if (skipHash) {
    return {
      expectedHash: null,
      expectedHashAlgorithm: null,
      expectedHashSha256: null,
    };
  }
  const digest = expectedHash.trim() || null;
  return {
    expectedHash: digest,
    expectedHashAlgorithm: algorithm,
    expectedHashSha256: algorithm === "sha256" ? digest : null,
  };
}
