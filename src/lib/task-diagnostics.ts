/** Protocol helpers for TaskDetails diagnostics presentation. */

export function isTorrentProtocol(protocol: string): boolean {
  return protocol === "bt" || protocol === "magnet";
}

export function isHlsProtocol(protocol: string): boolean {
  return protocol === "hls";
}

export function isDashProtocol(protocol: string): boolean {
  return protocol === "dash";
}

export function isFtpSftpProtocol(protocol: string): boolean {
  return protocol === "ftp" || protocol === "ftps" || protocol === "sftp";
}

export function isHttpLikeProtocol(protocol: string): boolean {
  return protocol === "http" || protocol === "https" || protocol.startsWith("webdav");
}

/** Show If-Range / ETag only for real HTTP-ish request methods. */
export function showsHttpRequestFields(method: string): boolean {
  const normalized = method.trim().toUpperCase();
  return (
    normalized === "GET" ||
    normalized === "HEAD" ||
    normalized === "POST" ||
    normalized === "PUT" ||
    normalized === "PATCH" ||
    normalized === "DELETE" ||
    normalized === "OPTIONS" ||
    normalized === "PROPFIND" ||
    normalized.startsWith("HTTP")
  );
}

export function diagnosticsSegmentsEmptyKey(protocol: string): string {
  if (isHlsProtocol(protocol)) return "taskDetails.noHlsSegments";
  if (isDashProtocol(protocol)) return "taskDetails.noDashSegments";
  if (isHttpLikeProtocol(protocol)) return "taskDetails.noChunks";
  return "taskDetails.noWorkUnits";
}

export function diagnosticsConnectionsEmptyKey(protocol: string): string {
  if (isHttpLikeProtocol(protocol)) return "taskDetails.noConnections";
  return "taskDetails.noWorkUnits";
}

export function diagnosticsRequestsEmptyKey(protocol: string): string {
  if (isHttpLikeProtocol(protocol)) return "taskDetails.noRequests";
  return "taskDetails.noRequestsGeneric";
}

export function defaultDiagSubTab(protocol: string): "segments" | "requests" {
  return isTorrentProtocol(protocol) ? "requests" : "segments";
}

/**
 * Align with FTP engine: FTPS on port 21 is explicit TLS; other FTPS ports
 * (default 990) are implicit TLS. Plain FTP has no TLS mode.
 */
export function ftpTlsModeLabel(protocol: string, url: string): string | null {
  if (protocol === "ftp") return "plain";
  if (protocol !== "ftps") return null;
  try {
    const parsed = new URL(url);
    const port = parsed.port ? Number(parsed.port) : 990;
    return port === 21 ? "explicit" : "implicit";
  } catch {
    return "implicit";
  }
}

export function parseUrlHostPort(url: string, defaultPort: number): { host: string; port: number } | null {
  try {
    const parsed = new URL(url);
    const host = parsed.hostname;
    if (!host) return null;
    const port = parsed.port ? Number(parsed.port) : defaultPort;
    return { host, port: Number.isFinite(port) ? port : defaultPort };
  } catch {
    return null;
  }
}
