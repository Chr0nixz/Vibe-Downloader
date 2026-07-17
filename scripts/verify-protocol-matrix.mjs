import { readFile } from "node:fs/promises";
import process from "node:process";
import { pathToFileURL } from "node:url";

const MATRIX_PATH = new URL("../docs/protocol-reliability-matrix.md", import.meta.url);
const REQUIRED_PROTOCOLS = ["FTP/FTPS", "SFTP", "BitTorrent", "HLS", "DASH", "WebDAV/WebDAVS", "Metalink4"];
const STATUS_COLUMNS = 11;
const ALLOWED_STATUSES = new Set(["automated", "partial", "unsupported", "n/a"]);

export function verifyProtocolMatrix(source) {
  const rows = source
    .split(/\r?\n/u)
    .filter((line) => line.startsWith("|") && !line.includes("---"))
    .map((line) =>
      line
        .split("|")
        .slice(1, -1)
        .map((cell) => cell.trim()),
    );
  const header = rows.shift();
  if (header?.at(0) !== "Protocol" || header.at(-1) !== "Evidence") {
    throw new Error("Protocol matrix header is missing or invalid");
  }

  for (const protocol of REQUIRED_PROTOCOLS) {
    const row = rows.find((candidate) => candidate[0] === protocol);
    if (!row) throw new Error(`Protocol matrix is missing ${protocol}`);
    if (row.length !== header.length) throw new Error(`${protocol} has an invalid column count`);

    for (const status of row.slice(1, 1 + STATUS_COLUMNS)) {
      if (!ALLOWED_STATUSES.has(status)) {
        throw new Error(`${protocol} contains unsupported status: ${status}`);
      }
    }
    if (!row.at(-1)?.includes("src-tauri/")) {
      throw new Error(`${protocol} is missing repository test evidence`);
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const source = await readFile(MATRIX_PATH, "utf8");
  verifyProtocolMatrix(source);
  console.log("Protocol reliability matrix verification passed");
}
