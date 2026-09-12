import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { EnvironmentPanel } from "@/components/settings/EnvironmentPanel";
import type { EnvironmentHealthReport, EnvironmentText } from "@/generated/bindings";
import i18n, { setLocale } from "@/i18n";

/**
 * FUN-28 regression cover for the surface that had none. The panel is the only
 * place the code-to-copy resolution is wired to real backend data, so this test
 * renders it against a report built from codes (not `raw`) and asserts the
 * localized text, the joined detail line, and that switching language actually
 * changes what is on screen.
 */
const getEnvironmentHealth = vi.fn();

vi.mock("@/lib/tauri", () => ({
  getEnvironmentHealth: () => getEnvironmentHealth(),
  runEnvironmentFix: vi.fn(),
}));

vi.mock("@/hooks/use-app-updater", () => ({
  useAppUpdater: () => ({
    currentVersion: "0.5.0",
    updateVersion: null,
    releaseNotes: null,
    updateDate: null,
    status: "idle",
    progress: null,
    error: null,
    checking: false,
    installing: false,
    isTauri: true,
    checkForUpdate: vi.fn(),
    installUpdate: vi.fn(),
    dismissUpdate: vi.fn(),
  }),
}));

/** Spelled out rather than inferred, so `Partial<Params>` accepts real values. */
const NO_PARAMS: EnvironmentText["params"] = {
  count: null,
  names: null,
  url: null,
  errors: null,
  path: null,
  section: null,
  available: null,
  total: null,
};

function text(
  code: EnvironmentText["code"],
  english: string,
  params: Partial<EnvironmentText["params"]> = {},
): EnvironmentText {
  return { code, params: { ...NO_PARAMS, ...params }, english };
}

const NATIVE_HOST_PATH = "/usr/local/bin/vibe-native-host";
const BRIDGE_URL = "ws://127.0.0.1:8731";
const HANDOFF_ERRORS = "Chrome: port busy";

const report: EnvironmentHealthReport = {
  checkedAtMs: "1700000000000",
  appVersion: "0.5.0",
  platform: "linux-x86_64",
  items: [
    {
      id: "native_host",
      status: "ok",
      summary: text("nativeHostReady", "Native Messaging host binary is ready."),
      detail: [text("raw", NATIVE_HOST_PATH)],
      suggestedActions: [],
    },
    {
      id: "browser",
      status: "warn",
      summary: text("browserMissingManifests", "2 detected browsers are missing manifests.", {
        count: 2,
        names: "Chrome, Edge",
      }),
      // Two fragments, exactly as the browser check composes them.
      detail: [
        text("bridgeListening", "bridge listening", { url: BRIDGE_URL }),
        text("recentHandoffErrors", "recent errors", { errors: HANDOFF_ERRORS }),
      ],
      suggestedActions: [],
    },
  ],
};

describe("EnvironmentPanel localization (FUN-28)", () => {
  beforeEach(async () => {
    getEnvironmentHealth.mockReset();
    getEnvironmentHealth.mockResolvedValue(report);
    await setLocale("en");
  });

  afterEach(async () => {
    await setLocale("en");
  });

  it("renders summary copy from the active bundle, and raw values verbatim", async () => {
    render(<EnvironmentPanel onFocusSection={vi.fn()} />);

    expect(await screen.findByText("Native Messaging host binary is ready.")).toBeDefined();
    // A `raw` fragment is not a key: it must print the value untouched.
    expect(screen.getByTitle(NATIVE_HOST_PATH)).toBeDefined();
  });

  it("interpolates count and names into the summary", async () => {
    render(<EnvironmentPanel onFocusSection={vi.fn()} />);
    expect(
      await screen.findByText("2 detected browsers are missing Native Messaging manifests: Chrome, Edge."),
    ).toBeDefined();
  });

  it("joins detail fragments into one line", async () => {
    render(<EnvironmentPanel onFocusSection={vi.fn()} />);
    const detail = await screen.findByTitle(
      `Realtime bridge listening but no extension connected (${BRIDGE_URL}). Recent handoff errors: ${HANDOFF_ERRORS}`,
    );
    expect(detail.textContent).toBe(
      `Realtime bridge listening but no extension connected (${BRIDGE_URL}). Recent handoff errors: ${HANDOFF_ERRORS}`,
    );
    // One <p>, not two — the old `Option<String>` contract.
    expect(detail.tagName).toBe("P");
    expect(detail.className).toContain("truncate");
  });

  it("re-renders the card in the newly selected language", async () => {
    render(<EnvironmentPanel onFocusSection={vi.fn()} />);
    expect(await screen.findByText("Native Messaging host binary is ready.")).toBeDefined();

    await setLocale("zh-CN");

    await waitFor(() => {
      expect(screen.getByText("本机消息传递宿主程序已就绪。")).toBeDefined();
    });
    expect(screen.queryByText("Native Messaging host binary is ready.")).toBeNull();
    // The raw path is language-independent and must survive the switch.
    expect(screen.getByTitle(NATIVE_HOST_PATH)).toBeDefined();
    expect(i18n.language).toBe("zh-CN");
  });
});
