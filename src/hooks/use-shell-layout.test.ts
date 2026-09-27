import { describe, expect, it } from "vitest";

import { readChromeLayout, readShellLayout } from "./use-shell-layout";

describe("shell layout helpers", () => {
  it("maps viewport widths to compact desktop layouts", () => {
    expect(readShellLayout(559)).toBe("narrow");
    expect(readShellLayout(560)).toBe("medium");
    // A window snapped to half of a 1366–1920px screen is a desktop window
    // with a mouse, not a phone; it keeps the dense desktop list.
    expect(readShellLayout(760)).toBe("medium");
    expect(readShellLayout(1023)).toBe("medium");
    expect(readShellLayout(1024)).toBe("medium");
    expect(readShellLayout(1199)).toBe("medium");
    expect(readShellLayout(1200)).toBe("wide");
  });

  it("merges the command bar into the titlebar in snapped or short windows only", () => {
    expect(readChromeLayout(420, 800)).toBe("stacked");
    expect(readChromeLayout(760, 800)).toBe("merged");
    expect(readChromeLayout(1023, 900)).toBe("merged");
    expect(readChromeLayout(1280, 800)).toBe("stacked");
    expect(readChromeLayout(1280, 560)).toBe("merged");
  });
});
