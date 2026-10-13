import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, stat, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import { promisify } from "node:util";
import {
  activateNativeFixture,
  assertCandidateExecutable,
  candidateIdentifier,
  DATASET_SIZES,
  datasetTaskId,
  loadNativeFixtures,
  prepareNativeFixtures,
} from "./native-ui-fixture.mjs";

const execFileAsync = promisify(execFile);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const helper = path.join(root, "scripts/perf/native-process.ps1");

export function summarize(values) {
  if (values.length === 0 || values.some((value) => !Number.isFinite(value)))
    throw new Error("Invalid metric samples.");
  const sorted = [...values].sort((left, right) => left - right);
  const percentile = (fraction) => sorted[Math.ceil(sorted.length * fraction) - 1];
  return { samples: values.length, min: sorted[0], p50: percentile(0.5), p95: percentile(0.95), max: sorted.at(-1) };
}

async function processOperation(executable, mode, argumentsList = []) {
  const { stdout } = await execFileAsync(
    "powershell.exe",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      helper,
      "-Mode",
      mode,
      "-Executable",
      executable,
      ...argumentsList,
    ],
    { windowsHide: true, timeout: 30_000 },
  );
  return JSON.parse(stdout.trim());
}

export async function connect(port) {
  const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(1_000) });
  const pages = await response.json();
  const target = pages.find(
    (entry) =>
      entry.type === "page" &&
      /^(?:https?:\/\/tauri\.localhost|tauri:\/\/localhost)(?:\/|$)/.test(entry.url) &&
      !new URL(entry.url).searchParams.has("surface"),
  );
  if (!target) throw new Error("Candidate WebView is not ready.");
  const websocketUrl = new URL(target.webSocketDebuggerUrl);
  if (
    websocketUrl.protocol !== "ws:" ||
    !["127.0.0.1", "localhost"].includes(websocketUrl.hostname) ||
    websocketUrl.port !== String(port)
  ) {
    throw new Error("Candidate CDP target escaped the expected loopback endpoint.");
  }
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      socket.close();
      reject(new Error("WebView CDP connection timed out."));
    }, 2_000);
    socket.addEventListener(
      "open",
      () => {
        clearTimeout(timeout);
        resolve();
      },
      { once: true },
    );
    socket.addEventListener(
      "error",
      (error) => {
        clearTimeout(timeout);
        reject(error);
      },
      { once: true },
    );
  });
  let nextId = 0;
  const pending = new Map();
  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    const receiver = pending.get(message.id);
    if (!receiver) return;
    pending.delete(message.id);
    clearTimeout(receiver.timeout);
    if (message.error) receiver.reject(new Error(JSON.stringify(message.error)));
    else receiver.resolve(message.result);
  });
  socket.addEventListener("close", () => {
    for (const receiver of pending.values()) {
      clearTimeout(receiver.timeout);
      receiver.reject(new Error("WebView CDP closed."));
    }
    pending.clear();
  });
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = ++nextId;
      const timeout = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`CDP timed out: ${method}`));
      }, 20_000);
      pending.set(id, { resolve, reject, timeout });
      socket.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (expression) => {
    const result = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    if (result.exceptionDetails)
      throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
    return result.result.value;
  };
  return { send, evaluate, close: () => socket.close() };
}

async function waitFor(cdp, expression, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const value = await cdp.evaluate(expression);
    if (value) return value;
    await delay(50);
  }
  throw new Error(`Native UI did not reach the expected state: ${expression}`);
}

async function launch(executable, port, retainSession) {
  const process = await processOperation(executable, "Launch", ["-Port", String(port)]);
  const session = { ...process, cdp: null };
  retainSession(session);
  const deadline = Date.now() + 45_000;
  while (Date.now() < deadline) {
    try {
      session.cdp = await connect(port);
      return session;
    } catch {
      await delay(100);
    }
  }
  throw new Error(`Candidate PID ${process.pid} did not expose its isolated WebView within 45 seconds.`);
}

async function quit(executable, session, port) {
  const snapshot = await processOperation(executable, "Sample", ["-CandidatePid", String(session.pid)]);
  if (!snapshot.alive) {
    session.cdp?.close();
    return { alreadyExited: true, joinedProcessCount: 0 };
  }
  const ownedIds = snapshot.processes.map((entry) => entry.pid).join(",");
  const cdp = session.cdp ?? (await connect(port));
  try {
    await cdp.evaluate("window.__TAURI_INTERNALS__.invoke('run_tray_menu_action', { action: 'quit' })");
  } catch (error) {
    if (!/CDP closed/.test(error.message)) throw error;
  } finally {
    cdp.close();
  }
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    const result = await processOperation(executable, "VerifyExit", ["-OwnedProcessIds", ownedIds]);
    if (result.remaining.length === 0) return { joinedProcessCount: snapshot.processes.length };
    await delay(500);
  }
  throw new Error("Candidate or WebView processes did not drain within 30 seconds.");
}

async function press(cdp, key, code, modifiers = 0) {
  const windowsVirtualKeyCode = { KeyA: 65, Backspace: 8, ArrowDown: 40 }[code];
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key, code, modifiers, windowsVirtualKeyCode });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key, code, modifiers, windowsVirtualKeyCode });
}

async function armInteraction(cdp, eventName, predicate) {
  await cdp.evaluate(`(() => {
    window.__b7Interaction = new Promise((resolve, reject) => {
      const timeout = setTimeout(() => { observer.disconnect(); document.removeEventListener('${eventName}', start, true); reject(new Error('Interaction timeout')); }, 10000);
      let started;
      const start = () => { started = performance.now(); };
      document.addEventListener('${eventName}', start, { once: true, capture: true });
      const observer = new MutationObserver(() => {
        if (started === undefined || !(${predicate})) return;
        observer.disconnect(); clearTimeout(timeout);
        requestAnimationFrame(() => requestAnimationFrame(() => resolve(performance.now() - started)));
      });
      observer.observe(document.body, { childList: true, subtree: true, attributes: true });
    });
  })()`);
}

async function selectFirstRow(cdp) {
  const row = await cdp.evaluate(
    `(() => { const row = document.querySelector('[role=listitem]'); const rect = row.getBoundingClientRect(); return {id: row.id, x: rect.x + 140, y: rect.y + 22}; })()`,
  );
  await armInteraction(
    cdp,
    "pointerdown",
    `document.getElementById(${JSON.stringify(row.id)})?.getAttribute('aria-current') === 'true'`,
  );
  await cdp.send("Input.dispatchMouseEvent", {
    type: "mousePressed",
    x: row.x,
    y: row.y,
    button: "left",
    clickCount: 1,
  });
  await cdp.send("Input.dispatchMouseEvent", {
    type: "mouseReleased",
    x: row.x,
    y: row.y,
    button: "left",
    clickCount: 1,
  });
  return { ms: await cdp.evaluate("window.__b7Interaction"), rowId: row.id };
}

async function keyboardNextRow(cdp) {
  const next = await cdp.evaluate(
    `(() => { const rows = [...document.querySelectorAll('[role=listitem]')]; const index = rows.findIndex(row => row.getAttribute('aria-current') === 'true'); return rows[index + 1].id; })()`,
  );
  await armInteraction(
    cdp,
    "keydown",
    `document.getElementById(${JSON.stringify(next)})?.getAttribute('aria-current') === 'true'`,
  );
  await press(cdp, "ArrowDown", "ArrowDown");
  const ms = await cdp.evaluate("window.__b7Interaction");
  await waitFor(cdp, `document.activeElement?.id === ${JSON.stringify(next)}`);
  return { ms, rowId: next, focusVerified: true };
}

async function clearSearch(cdp) {
  await cdp.evaluate(`document.querySelector('input[aria-label="Search tasks"]').focus()`);
  await press(cdp, "a", "KeyA", 2);
  await press(cdp, "Backspace", "Backspace");
  await waitFor(cdp, `Number(document.querySelector('[role=listitem]')?.getAttribute('aria-setsize')) >= 100`);
}

async function searchTask(cdp, id) {
  await clearSearch(cdp);
  await armInteraction(
    cdp,
    "input",
    `document.querySelectorAll('[role=listitem]').length === 1 && document.getElementById('task-row-${id}')?.getAttribute('aria-setsize') === '1'`,
  );
  await cdp.send("Input.insertText", { text: id });
  return { query: id, ms: await cdp.evaluate("window.__b7Interaction") };
}

async function scroll(cdp) {
  const point = await cdp.evaluate(
    `(() => { const container = document.querySelector('[role=list]').parentElement; const rect = container.getBoundingClientRect(); container.scrollTop = 0; return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 }; })()`,
  );
  await cdp.evaluate(`(() => {
    const start = performance.now(); const frames = []; const rows = []; const longTasks = [];
    const observer = new PerformanceObserver(list => longTasks.push(...list.getEntries().map(entry => ({start: entry.startTime, duration: entry.duration}))));
    observer.observe({ type: 'longtask', buffered: false });
    window.__b7Scroll = new Promise(resolve => {
      function tick(now) {
        frames.push(now - start); rows.push(document.querySelectorAll('[role=listitem]').length);
        if (now - start < 10000) { requestAnimationFrame(tick); return; }
        observer.disconnect(); const buckets = Array.from({ length: 10 }, (_, second) => frames.filter(value => value >= second * 1000 && value < (second + 1) * 1000).length);
        resolve({durationMs: now - start, frameCount: frames.length, averageRafPerSecond: frames.length * 1000 / (now - start), minimumFullSecondRafCount: Math.min(...buckets), frameBuckets: buckets, maxRenderedRows: Math.max(...rows), longTasks, loadedRows: Number(document.querySelector('[role=listitem]')?.getAttribute('aria-setsize')), scrollTop: document.querySelector('[role=list]').parentElement.scrollTop });
      }
      requestAnimationFrame(tick);
    });
  })()`);
  const deadline = Date.now() + 10_100;
  const reverseAt = deadline - 5_100;
  let wheelEvents = 0;
  while (Date.now() < deadline) {
    await cdp.send("Input.dispatchMouseEvent", {
      type: "mouseWheel",
      ...point,
      deltaX: 0,
      deltaY: Date.now() < reverseAt ? 220 : -220,
    });
    wheelEvents++;
    await delay(50);
  }
  return { ...(await cdp.evaluate("window.__b7Scroll")), wheelEvents };
}

async function screenshot(cdp, file) {
  const result = await cdp.send("Page.captureScreenshot", { format: "png" });
  await writeFile(file, Buffer.from(result.data, "base64"));
}

export async function runNativeUi({
  configPath,
  executable,
  repetitions = 5,
  steadySeconds = 300,
  port = 19424,
  reuseFixture = false,
}) {
  if (process.platform !== "win32") throw new Error("Native WebView2 measurements require Windows.");
  if (!Number.isInteger(repetitions) || repetitions < 1 || repetitions > 10)
    throw new Error("Repetitions must be 1-10.");
  if (!Number.isInteger(steadySeconds) || steadySeconds < 0 || steadySeconds > 3600)
    throw new Error("Steady sampling must be 0-3600 seconds.");
  if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("Invalid loopback CDP port.");
  const expectedIdentifier = candidateIdentifier(JSON.parse(await readFile(configPath, "utf8")));
  const executableBytes = await readFile(executable);
  assertCandidateExecutable(executableBytes, expectedIdentifier);
  const occupied = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(1_000) }).then(
    () => true,
    () => false,
  );
  if (occupied) throw new Error("The native measurement CDP port is already in use.");
  const assertExited = async () => {
    if ((await processOperation(executable, "Check")).running)
      throw new Error("The candidate must exit before switching its fixture.");
  };
  await assertExited();
  const fixture = reuseFixture ? await loadNativeFixtures(configPath) : await prepareNativeFixtures(configPath, root);
  const measurement = path.join(fixture.evidence, new Date().toISOString().replaceAll(/[:.]/g, "-"));
  await mkdir(measurement);
  const { stdout: commit } = await execFileAsync("git", ["rev-parse", "HEAD"], { cwd: root });
  const { stdout: dirty } = await execFileAsync("git", ["status", "--porcelain"], { cwd: root });
  const report = {
    schemaVersion: 1,
    collectedAt: new Date().toISOString(),
    version: JSON.parse(await readFile(path.join(root, "package.json"))).version,
    identifier: fixture.identifier,
    gitCommit: commit.trim(),
    gitDirty: dirty.length > 0,
    executable,
    executableSha256: createHash("sha256").update(executableBytes).digest("hex"),
    buildProfile: 'release opt-level="s", LTO; unsigned candidate',
    host: {
      os: os.version(),
      arch: os.arch(),
      cpu: os.cpus()[0].model,
      logicalProcessors: os.cpus().length,
      ramBytes: os.totalmem(),
      node: process.version,
    },
    method: {
      repetitions,
      steadySeconds,
      cache:
        "An unmeasured 1k primer sets locale/onboarding; OS file cache is not cleared; first measured launch and subsequent launches reported separately",
      startup:
        "Observed first task after CDP attachment, measured from Start-Process call timestamp; upper bound, not exact first paint",
      interactions: "Input event to two animation frames after matching DOM mutation; includes 300ms search debounce",
      scrolling: "10 seconds of CDP wheel input; rAF cadence, not compositor/presentation FPS",
      memory: "Sum of main process and descendant WebView2 working sets; shared pages may be counted more than once",
      fixture: "50% completed, 40% paused, 10% failed; one file record per task, no network transfers or actual files",
    },
    datasets: [],
  };
  const reportPath = path.join(measurement, "native-ui.json");
  let session;
  try {
    await activateNativeFixture(fixture, DATASET_SIZES[0], assertExited);
    session = await launch(executable, port, (owned) => {
      session = owned;
    });
    await waitFor(session.cdp, "typeof window.__TAURI_INTERNALS__?.invoke === 'function'");
    await waitFor(
      session.cdp,
      "window.__TAURI_INTERNALS__.invoke('get_startup_status').then(status => status.mode === 'ready')",
    );
    const settings = await session.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('get_settings')");
    if (
      settings.defaultSaveDir !== path.join(fixture.evidence, "downloads") ||
      settings.autoResumeOnStartup ||
      settings.autoUpdateCheckEnabled ||
      settings.clipboardMonitorEnabled ||
      settings.completionAction !== "none"
    ) {
      throw new Error("The running app settings do not match the isolated inert fixture.");
    }
    report.settingsVerified = true;
    await session.cdp.evaluate(
      "localStorage.setItem('vibe-onboarding-completed','1'); localStorage.setItem('vibe-locale','en'); localStorage.setItem('vibe-row-density','comfortable')",
    );
    await session.cdp.send("Page.reload");
    await delay(500);
    await waitFor(
      session.cdp,
      `document.querySelector('input[aria-label="Search tasks"]') && document.querySelector('[role=listitem]') && !document.querySelector('[role=dialog]')`,
    );
    report.primer = { taskCount: DATASET_SIZES[0], exit: await quit(executable, session, port) };
    session = null;
    for (const count of DATASET_SIZES) {
      await activateNativeFixture(fixture, count, assertExited);
      const dataset = { taskCount: count, launches: [], searches: [], memorySamples: [] };
      report.datasets.push(dataset);
      for (let repetition = 0; repetition < repetitions; repetition++) {
        session = await launch(executable, port, (owned) => {
          session = owned;
        });
        await waitFor(
          session.cdp,
          `document.querySelector('[role=listitem]') && document.querySelector('input[aria-label="Search tasks"]') && !document.querySelector('[role=dialog]')`,
        );
        const firstRowObservedMs = Date.now() - Date.parse(session.startedAtUtc);
        const startup = await session.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('get_startup_status')");
        if (startup.mode !== "ready") throw new Error(`Candidate startup was not ready: ${JSON.stringify(startup)}`);
        const stats = await session.cdp.evaluate("window.__TAURI_INTERNALS__.invoke('get_task_stats')");
        if (Number(stats.all) !== count || Number(stats.active) !== 0 || Number(stats.queued) !== 0) {
          throw new Error("Candidate task counts do not match the inert fixture.");
        }
        dataset.taskStats = stats;
        const viewport = await session.cdp.evaluate(
          "({width:innerWidth,height:innerHeight,dpr:devicePixelRatio,visibility:document.visibilityState})",
        );
        if (viewport.visibility !== "visible") throw new Error("Native frame measurements require a visible WebView.");
        if (dataset.viewport && JSON.stringify(dataset.viewport) !== JSON.stringify(viewport))
          throw new Error("Native UI viewport changed between repetitions.");
        dataset.viewport = viewport;
        dataset.webview = await session.cdp.send("Browser.getVersion");
        const selected = await selectFirstRow(session.cdp);
        const interactiveObservedMs = Date.now() - Date.parse(session.startedAtUtc);
        const keyboard = await keyboardNextRow(session.cdp);
        dataset.launches.push({
          repetition: repetition + 1,
          firstRowObservedMs,
          interactiveObservedMs,
          launchCallMs: session.launchCallMs,
          selected,
          keyboard,
        });
        dataset.searches.push(await searchTask(session.cdp, datasetTaskId(count - 27 - repetition * 13)));
        await clearSearch(session.cdp);
        if (repetition === repetitions - 1) {
          await screenshot(session.cdp, path.join(measurement, `${count}-first-screen.png`));
          dataset.scroll = await scroll(session.cdp);
          await screenshot(session.cdp, path.join(measurement, `${count}-after-scroll.png`));
          await session.cdp.evaluate("document.querySelector('[role=list]').parentElement.scrollTop = 0");
          session.cdp.close();
          session.cdp = null;
          const steadyStart = Date.now();
          do {
            const sample = await processOperation(executable, "Sample", ["-CandidatePid", String(session.pid)]);
            if (!sample.alive) throw new Error("Candidate exited during steady memory sampling.");
            const databaseBytes = (await stat(path.join(fixture.dataDirectory, "vibe.db"))).size;
            const walBytes = (
              await stat(path.join(fixture.dataDirectory, "vibe.db-wal")).catch((error) => {
                if (error.code === "ENOENT") return { size: 0 };
                throw error;
              })
            ).size;
            dataset.memorySamples.push({
              elapsedSeconds: (Date.now() - steadyStart) / 1_000,
              databaseBytes,
              walBytes,
              ...sample,
            });
            await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
            console.log(
              JSON.stringify({
                taskCount: count,
                elapsedSeconds: dataset.memorySamples.at(-1).elapsedSeconds,
                workingSetMiB: sample.workingSetBytes / 1_048_576,
                privateMiB: sample.privateBytes / 1_048_576,
              }),
            );
            const remaining = steadySeconds * 1_000 - (Date.now() - steadyStart);
            if (remaining <= 0) break;
            await delay(Math.min(30_000, remaining));
          } while (dataset.memorySamples.at(-1).elapsedSeconds < steadySeconds);
          dataset.steady = dataset.memorySamples.at(-1);
        }
        const exit = await quit(executable, session, port);
        session = null;
        dataset.launches.at(-1).exit = exit;
        console.log(
          JSON.stringify({
            taskCount: count,
            repetition: repetition + 1,
            firstRowObservedMs,
            searchMs: dataset.searches.at(-1).ms,
          }),
        );
      }
      dataset.summary = {
        firstMeasuredLaunchMs: dataset.launches[0].firstRowObservedMs,
        subsequentFirstRowObservedMs:
          dataset.launches.length > 1
            ? summarize(dataset.launches.slice(1).map((entry) => entry.firstRowObservedMs))
            : null,
        firstRowObservedMs: summarize(dataset.launches.map((entry) => entry.firstRowObservedMs)),
        interactiveObservedMs: summarize(dataset.launches.map((entry) => entry.interactiveObservedMs)),
        selectionMs: summarize(dataset.launches.map((entry) => entry.selected.ms)),
        keyboardMs: summarize(dataset.launches.map((entry) => entry.keyboard.ms)),
        searchMs: summarize(dataset.searches.map((entry) => entry.ms)),
      };
      await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
    }
    report.completedAt = new Date().toISOString();
    await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
    return {
      reportPath,
      datasets: report.datasets.map(({ taskCount, summary, scroll, steady }) => ({
        taskCount,
        summary,
        scroll,
        steady,
      })),
    };
  } catch (error) {
    report.error = error.stack;
    await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
    throw error;
  } finally {
    if (session) {
      try {
        await quit(executable, session, port);
      } catch (error) {
        report.cleanupPending = { pid: session.pid, message: error.message };
        await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
        console.error(`Candidate PID ${session.pid} stop is still pending; see ${reportPath}`);
      }
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const options = {};
  const names = {
    "--config": "configPath",
    "--exe": "executable",
    "--repetitions": "repetitions",
    "--steady-seconds": "steadySeconds",
    "--port": "port",
    "--reuse-fixture": "reuseFixture",
  };
  for (let index = 2; index < process.argv.length; index += 2) {
    const name = names[process.argv[index]];
    const value = process.argv[index + 1];
    if (!name || !value)
      throw new Error(
        "Usage: node scripts/perf/native-ui.mjs --config <isolated-overlay> --exe <release-exe> [--repetitions 5] [--steady-seconds 300] [--port 19424] [--reuse-fixture true]",
      );
    if (name === "reuseFixture") {
      if (value !== "true")
        throw new Error("--reuse-fixture accepts only true and verifies existing fixture ownership.");
      options[name] = true;
    } else
      options[name] = ["repetitions", "steadySeconds", "port"].includes(name) ? Number(value) : path.resolve(value);
  }
  if (!options.configPath || !options.executable) throw new Error("--config and --exe are required.");
  console.log(JSON.stringify(await runNativeUi(options), null, 2));
}
