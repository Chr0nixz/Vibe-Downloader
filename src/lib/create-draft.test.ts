import { describe, expect, it } from "vitest";

import {
  applyDraftToCreateTaskInput,
  type CreateDraftShared,
  draftHashFields,
  toDirectoryProbeInput,
  toImportUrlsInput,
} from "./create-draft";

const sampleDraft: CreateDraftShared = {
  username: "alice",
  password: "secret",
  privateKeyData: "-----BEGIN OPENSSH PRIVATE KEY-----\n...",
  privateKeyPassphrase: null,
  expectedHash: "abc",
  expectedHashAlgorithm: "sha256",
  expectedHashSha256: "abc",
  taskSpeedLimitBps: "1048576",
  priority: "high",
  categoryKey: "videos",
  allowDuplicate: true,
  proxyMode: "custom",
  proxyUrl: "socks5://127.0.0.1:1080",
  proxyUsername: "proxy-user",
  proxyPassword: "proxy-pass",
  proxyNoProxy: "localhost",
};

describe("FUN-17 create draft contract", () => {
  it("maps the same shared fields into single create and batch import inputs", () => {
    const single = applyDraftToCreateTaskInput(
      {
        url: "https://example.com/a.bin",
        saveDir: "/tmp",
        fileName: null,
        probeSnapshot: null,
        selectedFilePaths: null,
        selectedHlsVariantUri: null,
        selectedHlsAudioTrackUris: null,
        selectedHlsSubtitleTrackUris: null,
      },
      sampleDraft,
    );
    const batch = toImportUrlsInput("https://example.com/a.bin\nhttps://example.com/b.bin", "/tmp", true, sampleDraft);

    expect(single.username).toBe(batch.username);
    expect(single.password).toBe(batch.password);
    expect(single.privateKeyData).toBe(batch.privateKeyData);
    expect(single.expectedHash).toBe(batch.expectedHash);
    expect(single.expectedHashAlgorithm).toBe(batch.expectedHashAlgorithm);
    expect(single.taskSpeedLimitBps).toBe(batch.taskSpeedLimitBps);
    expect(single.priority).toBe(batch.priority);
    expect(single.categoryKey).toBe(batch.categoryKey);
    expect(single.allowDuplicate).toBe(batch.allowDuplicate);
    expect(single.proxyMode).toBe(batch.proxyMode);
    expect(single.proxyUrl).toBe(batch.proxyUrl);
    expect(single.proxyUsername).toBe(batch.proxyUsername);
    expect(single.proxyPassword).toBe(batch.proxyPassword);
    expect(single.proxyNoProxy).toBe(batch.proxyNoProxy);
  });

  it("builds directory probe input from the same auth/proxy draft", () => {
    const input = toDirectoryProbeInput("ftp://example.com/pub/", sampleDraft);
    expect(input).toEqual({
      url: "ftp://example.com/pub/",
      username: "alice",
      password: "secret",
      privateKeyData: sampleDraft.privateKeyData,
      privateKeyPassphrase: null,
      proxyMode: "custom",
      proxyUrl: "socks5://127.0.0.1:1080",
      proxyUsername: "proxy-user",
      proxyPassword: "proxy-pass",
      proxyNoProxy: "localhost",
    });
  });

  it("mirrors sha256 digests into the legacy field", () => {
    expect(draftHashFields("deadbeef", "sha256", false)).toEqual({
      expectedHash: "deadbeef",
      expectedHashAlgorithm: "sha256",
      expectedHashSha256: "deadbeef",
    });
    expect(draftHashFields("deadbeef", "md5", false).expectedHashSha256).toBeNull();
    expect(draftHashFields("x", "sha256", true).expectedHash).toBeNull();
  });
});
