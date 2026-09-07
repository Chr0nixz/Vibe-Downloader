import { afterEach, describe, expect, it } from "vitest";

import {
  clearSettingsRecoveryReturn,
  consumeSettingsFocus,
  readSettingsRecoveryReturn,
  writeSettingsRecoveryReturn,
} from "./settings-recovery-return";

afterEach(() => {
  clearSettingsRecoveryReturn();
});

describe("settings recovery return", () => {
  it("round-trips focus, task, and action", () => {
    writeSettingsRecoveryReturn({
      focus: "ffmpeg_path",
      taskId: "task-1",
      action: "configure_ffmpeg",
    });

    expect(readSettingsRecoveryReturn()).toEqual({
      focus: "ffmpeg_path",
      taskId: "task-1",
      action: "configure_ffmpeg",
    });
    expect(consumeSettingsFocus()).toBe("ffmpeg_path");
    expect(readSettingsRecoveryReturn()?.taskId).toBe("task-1");
  });

  it("clears the return path after the user leaves settings", () => {
    writeSettingsRecoveryReturn({
      focus: "sftp_known_hosts",
      taskId: "task-2",
      action: "manage_sftp_host_keys",
    });
    clearSettingsRecoveryReturn();
    expect(readSettingsRecoveryReturn()).toBeNull();
  });
});
