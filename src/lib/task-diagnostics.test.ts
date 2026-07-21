import { describe, expect, it } from "vitest";

import {
  defaultDiagSubTab,
  diagnosticsRequestsEmptyKey,
  diagnosticsSegmentsEmptyKey,
  isHlsProtocol,
  isTorrentProtocol,
  showsHttpRequestFields,
} from "./task-diagnostics";

describe("task-diagnostics helpers", () => {
  it("classifies protocols", () => {
    expect(isTorrentProtocol("bt")).toBe(true);
    expect(isTorrentProtocol("magnet")).toBe(true);
    expect(isHlsProtocol("hls")).toBe(true);
    expect(isHlsProtocol("dash")).toBe(false);
  });

  it("defaults BT diagnostics to requests", () => {
    expect(defaultDiagSubTab("bt")).toBe("requests");
    expect(defaultDiagSubTab("https")).toBe("segments");
  });

  it("picks protocol-aware empty keys", () => {
    expect(diagnosticsSegmentsEmptyKey("hls")).toBe("taskDetails.noHlsSegments");
    expect(diagnosticsSegmentsEmptyKey("ftp")).toBe("taskDetails.noWorkUnits");
    expect(diagnosticsRequestsEmptyKey("https")).toBe("taskDetails.noRequests");
    expect(diagnosticsRequestsEmptyKey("bt")).toBe("taskDetails.noRequestsGeneric");
  });

  it("gates HTTP-only request fields", () => {
    expect(showsHttpRequestFields("GET")).toBe(true);
    expect(showsHttpRequestFields("PROPFIND")).toBe(true);
    expect(showsHttpRequestFields("FTP RETR")).toBe(false);
    expect(showsHttpRequestFields("BT SOURCE")).toBe(false);
  });
});
