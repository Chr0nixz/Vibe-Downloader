import { describe, expect, it } from "vitest";

import { readShellLayout } from "./use-shell-layout";

describe("shell layout helpers", () => {
  it("maps viewport widths to compact desktop layouts", () => {
    expect(readShellLayout(767)).toBe("narrow");
    expect(readShellLayout(768)).toBe("medium");
    expect(readShellLayout(1023)).toBe("medium");
    expect(readShellLayout(1024)).toBe("wide");
  });
});
