import { describe, expect, it } from "vitest";

import { queueReasonTone } from "@/components/workspaces/QueueCenter";

describe("workspace presentation logic", () => {
  it("reserves the ready tone for work the scheduler can start", () => {
    expect(queueReasonTone("ready")).toBe("ready");
    expect(queueReasonTone("retry_delay")).toBe("muted");
    expect(queueReasonTone("host_limit")).toBe("waiting");
  });
});
