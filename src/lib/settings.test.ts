import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyGlobalSpeedLimit } from "@/lib/settings";

const mocks = vi.hoisted(() => ({ updateSettings: vi.fn() }));

vi.mock("@/lib/tauri", () => ({ updateSettings: mocks.updateSettings }));

describe("applyGlobalSpeedLimit", () => {
  beforeEach(() => {
    mocks.updateSettings.mockReset().mockResolvedValue({});
  });

  it("sends only the global limit patch", async () => {
    await applyGlobalSpeedLimit(512_000);

    expect(mocks.updateSettings).toHaveBeenCalledWith({ globalSpeedLimitBps: "512000" });
  });

  it("clears the global limit explicitly", async () => {
    await applyGlobalSpeedLimit(null);

    expect(mocks.updateSettings).toHaveBeenCalledWith({ globalSpeedLimitBps: null });
  });
});
