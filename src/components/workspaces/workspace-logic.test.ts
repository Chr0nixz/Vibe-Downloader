import { describe, expect, it } from "vitest";

import { attentionCategory } from "@/components/workspaces/AttentionCenter";
import { queueReasonTone } from "@/components/workspaces/QueueCenter";
import type { Task } from "@/types/task";

function attentionTask(recoveryActions: Task["recoveryActions"]): Task {
  return {
    recoveryActions,
    errorMessage: "Action required",
    errorCode: null,
  } as Task;
}

describe("workspace presentation logic", () => {
  it("groups attention tasks by the action the user can take", () => {
    expect(attentionCategory(attentionTask(["choose_another_folder"]))).toBe("storage");
    expect(attentionCategory(attentionTask(["check_url"]))).toBe("source");
    expect(attentionCategory(attentionTask(["configure_ffmpeg"]))).toBe("runtime");
    expect(attentionCategory(attentionTask(["retry_later"]))).toBe("retry");
  });

  it("uses structured error codes before generic recovery actions", () => {
    const task = attentionTask(["restart", "open_folder"]);
    task.errorMessage = JSON.stringify({
      code: "remote_changed",
      message: "taskDiagnostics.remoteChanged",
      recoverable: true,
      actions: ["restart", "open_folder"],
    });

    expect(attentionCategory(task)).toBe("source");
  });

  it("reserves the ready tone for work the scheduler can start", () => {
    expect(queueReasonTone("ready")).toBe("ready");
    expect(queueReasonTone("retry_delay")).toBe("muted");
    expect(queueReasonTone("host_limit")).toBe("waiting");
  });
});
