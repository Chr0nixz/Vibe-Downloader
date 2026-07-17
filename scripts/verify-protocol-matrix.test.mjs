import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { verifyProtocolMatrix } from "./verify-protocol-matrix.mjs";

test("the checked-in protocol matrix is complete", async () => {
  const source = await readFile(new URL("../docs/protocol-reliability-matrix.md", import.meta.url), "utf8");
  assert.doesNotThrow(() => verifyProtocolMatrix(source));
});

test("an unknown capability status is rejected", () => {
  const source = `| Protocol | Create | Probe | Pause | Resume | Cancel | Retry | Proxy | Credentials | Checksum | Restart | Diagnostics | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| FTP/FTPS | maybe | automated | partial | partial | partial | automated | automated | automated | partial | partial | partial | src-tauri/tests/ftp_engine.rs |`;
  assert.throws(() => verifyProtocolMatrix(source), /unsupported status: maybe/u);
});
