import { createHash } from "node:crypto";
import { constants } from "node:fs";
import { copyFile, lstat, mkdir, open, readdir, readFile, realpath, stat, writeFile } from "node:fs/promises";
import path from "node:path";

const sqlite = await import("node:sqlite").catch((error) => {
  if (error.code === "ERR_UNKNOWN_BUILTIN_MODULE") return null;
  throw error;
});
const DatabaseSync = sqlite?.DatabaseSync;
export const nativeSqliteAvailable = Boolean(DatabaseSync);

export const DATASET_SIZES = [1_000, 10_000];
const MARKER = "b7-native-ui-fixture.json";

export function candidateIdentifier(config) {
  if (
    !/^com\.vibe\.downloader\.b7\.[a-z][a-z0-9-]{1,39}$/.test(config.identifier ?? "") ||
    config.bundle?.createUpdaterArtifacts !== false
  ) {
    throw new Error("Native UI measurements require an isolated B7 candidate with updater artifacts disabled.");
  }
  return config.identifier;
}

export function datasetTaskId(index) {
  return `b7-ui-${String(index).padStart(5, "0")}`;
}

export function assertCandidateExecutable(bytes, identifier) {
  candidateIdentifier({ identifier, bundle: { createUpdaterArtifacts: false } });
  // Catch a stale or production executable before it can open ordinary app data.
  if (!bytes.includes(Buffer.from(identifier))) {
    throw new Error("Executable does not contain the expected isolated candidate identity.");
  }
}

async function seedDatabase(file, count, workspaceRoot, downloads) {
  const database = new DatabaseSync(file);
  try {
    database.exec(
      "PRAGMA foreign_keys=ON; CREATE TABLE _sqlx_migrations(version BIGINT PRIMARY KEY, description TEXT NOT NULL, installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, success BOOLEAN NOT NULL, checksum BLOB NOT NULL, execution_time BIGINT NOT NULL)",
    );
    const migrations = path.join(workspaceRoot, "src-tauri", "src", "db", "migrations");
    for (const name of (await readdir(migrations)).filter((name) => /^\d+_.*\.sql$/.test(name)).sort()) {
      const sql = await readFile(path.join(migrations, name));
      database.exec(sql.toString("utf8"));
      database
        .prepare("INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) VALUES(?,?,1,?,0)")
        .run(
          Number.parseInt(name, 10),
          name.replace(/^\d+_|\.sql$/g, "").replaceAll("_", " "),
          createHash("sha384").update(sql).digest(),
        );
    }
    database.exec("BEGIN IMMEDIATE");
    const setting = database.prepare("INSERT INTO settings(key,value) VALUES(?,?)");
    for (const [key, value] of Object.entries({
      default_save_dir: downloads,
      clipboard_monitor_enabled: "false",
      auto_resume_on_startup: "false",
      system_notifications: "false",
      close_to_tray: "false",
      floating_window_enabled: "false",
      start_on_boot: "false",
      completion_action: "none",
      auto_update_check_enabled: "false",
    }))
      setting.run(key, value);
    const task = database.prepare(
      "INSERT INTO tasks(id,url,final_url,protocol,file_name,save_dir,final_path,total_size,downloaded_bytes,status,source_key,created_at,updated_at,completed_at,supports_resume,supports_parallel,error_code) VALUES(?,?,?,'https',?,?,?,?,?,?,'93.184.216.34',?,?,?,?,?,?)",
    );
    const fileRecord = database.prepare(
      "INSERT INTO task_files(id,task_id,relative_path,file_name,save_dir,final_path,total_size,downloaded_bytes,status) VALUES(?,?,?,?,?,?,?,?,?)",
    );
    for (let index = 0; index < count; index++) {
      const id = datasetTaskId(index);
      const name = `${id}.${["bin", "zip", "mp4", "pdf"][index % 4]}`;
      // Only inert states: opening a fixture must never dispatch a download.
      const status = index % 10 < 5 ? "completed" : index % 10 < 9 ? "paused" : "failed";
      const size = 16_777_216 + index;
      const downloaded = status === "completed" ? size : Math.floor(size / 2);
      const time = new Date(Date.UTC(2026, 9, 1) + index * 1_000).toISOString();
      const finalPath = path.join(downloads, name);
      const url = `https://93.184.216.34/${name}`;
      task.run(
        id,
        url,
        url,
        name,
        downloads,
        finalPath,
        size,
        downloaded,
        status,
        time,
        time,
        status === "completed" ? time : null,
        1,
        1,
        null,
      );
      fileRecord.run(`${id}-file`, id, name, name, downloads, finalPath, size, downloaded, status);
    }
    database.exec("COMMIT");
    if (database.prepare("PRAGMA integrity_check").get().integrity_check !== "ok") {
      throw new Error("Native UI fixture failed SQLite integrity_check.");
    }
  } finally {
    database.close();
  }
}

export async function prepareNativeFixtures(configPath, workspaceRoot, appDataRoot = process.env.APPDATA) {
  if (!DatabaseSync)
    throw new Error("Native UI fixtures require Node with built-in node:sqlite (verified on Node 22.23.2).");
  if (!appDataRoot) throw new Error("APPDATA is required for the isolated Windows candidate.");
  const config = JSON.parse(await readFile(configPath, "utf8"));
  const identifier = candidateIdentifier(config);
  const appData = await realpath(appDataRoot);
  const requestedDataDirectory = path.join(appData, identifier);
  const evidence = path.join(path.dirname(await realpath(configPath)), "native-ui");
  // Exclusive creation refuses both previous runs and directory junctions.
  await mkdir(requestedDataDirectory);
  // Windows packaged hosts can virtualize a newly created Roaming child.
  const dataDirectory = await realpath(requestedDataDirectory);
  await mkdir(evidence);
  const downloads = path.join(evidence, "downloads");
  await mkdir(downloads);
  for (const count of DATASET_SIZES) {
    await seedDatabase(path.join(evidence, `seed-${count}.sqlite`), count, workspaceRoot, downloads);
  }
  const fixture = {
    schemaVersion: 1,
    identifier,
    dataDirectory,
    requestedDataDirectory,
    evidence,
    sizes: DATASET_SIZES,
    states: { completed: 0.5, paused: 0.4, failed: 0.1 },
    networkTransfers: false,
    actualDownloadFiles: false,
  };
  await writeFile(path.join(dataDirectory, MARKER), `${JSON.stringify(fixture, null, 2)}\n`, { flag: "wx" });
  await writeFile(path.join(evidence, "fixture.json"), `${JSON.stringify(fixture, null, 2)}\n`, { flag: "wx" });
  return fixture;
}

export async function loadNativeFixtures(configPath, appDataRoot = process.env.APPDATA) {
  if (!appDataRoot) throw new Error("APPDATA is required for the isolated Windows candidate.");
  const identifier = candidateIdentifier(JSON.parse(await readFile(configPath, "utf8")));
  const requestedDataDirectory = path.join(await realpath(appDataRoot), identifier);
  if ((await lstat(requestedDataDirectory)).isSymbolicLink())
    throw new Error("Refusing a redirected candidate data directory.");
  const dataDirectory = await realpath(requestedDataDirectory);
  const evidence = path.join(path.dirname(await realpath(configPath)), "native-ui");
  const fixture = JSON.parse(await readFile(path.join(dataDirectory, MARKER), "utf8"));
  if (fixture.identifier !== identifier || fixture.dataDirectory !== dataDirectory || fixture.evidence !== evidence) {
    throw new Error("Existing fixture does not belong to this candidate configuration.");
  }
  return fixture;
}

export async function activateNativeFixture(fixture, count, assertExited) {
  if (!DatabaseSync) throw new Error("Native UI fixtures require built-in node:sqlite.");
  if (!DATASET_SIZES.includes(count)) throw new Error("Unsupported native UI dataset size.");
  candidateIdentifier({ identifier: fixture.identifier, bundle: { createUpdaterArtifacts: false } });
  const directory = await realpath(fixture.dataDirectory);
  if (
    directory.toLowerCase() !== path.resolve(fixture.dataDirectory).toLowerCase() ||
    path.basename(directory) !== fixture.identifier
  ) {
    throw new Error("Refusing a redirected candidate data directory.");
  }
  const marker = JSON.parse(await readFile(path.join(directory, MARKER), "utf8"));
  if (
    marker.identifier !== fixture.identifier ||
    marker.evidence !== fixture.evidence ||
    marker.dataDirectory !== directory
  ) {
    throw new Error("Candidate fixture ownership marker does not match.");
  }
  const database = path.join(directory, "vibe.db");
  for (const name of ["vibe.db", "vibe.db-wal", "vibe.db-shm"]) {
    const info = await lstat(path.join(directory, name)).catch((error) => {
      if (error.code === "ENOENT") return null;
      throw error;
    });
    if (info?.isSymbolicLink()) throw new Error("Refusing a redirected candidate database.");
    if (name === "vibe.db-wal" && info && info.size > 0) {
      const file = await open(path.join(directory, name), "r");
      try {
        const header = Buffer.alloc(32);
        const { bytesRead } = await file.read(header, 0, header.length, 0);
        if (bytesRead !== 32 || ![0x377f0682, 0x377f0683].includes(header.readUInt32BE(0))) {
          throw new Error("Candidate WAL/SHM has not drained: invalid WAL header.");
        }
      } finally {
        await file.close();
      }
    }
  }
  const source = path.join(fixture.evidence, `seed-${count}.sqlite`);
  if (!(await stat(source)).isFile()) throw new Error("Missing native UI seed database.");
  await assertExited();
  const existing = await stat(database).catch((error) => {
    if (error.code === "ENOENT") return null;
    throw error;
  });
  if (existing) {
    // Recover committed WAL frames only after the owning app has exited.
    const handle = new DatabaseSync(database);
    try {
      if (handle.prepare("PRAGMA wal_checkpoint(TRUNCATE)").get().busy !== 0) {
        throw new Error("Candidate database checkpoint is still busy.");
      }
      if (handle.prepare("PRAGMA integrity_check").get().integrity_check !== "ok") {
        throw new Error("Candidate database integrity check failed.");
      }
    } finally {
      handle.close();
    }
    for (const suffix of ["-wal", "-shm"]) {
      const remaining = await stat(`${database}${suffix}`).catch((error) => {
        if (error.code === "ENOENT") return null;
        throw error;
      });
      if (remaining?.size > 0) throw new Error("Candidate WAL/SHM has not drained.");
    }
    const checkpointed = path.join(
      fixture.evidence,
      `checkpointed-${new Date().toISOString().replaceAll(/[:.]/g, "-")}.sqlite`,
    );
    await copyFile(database, checkpointed, constants.COPYFILE_EXCL);
  }
  await copyFile(source, database);
}
