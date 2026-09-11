import { afterEach, describe, expect, it } from "vitest";

import i18n, { LOCALE_STORAGE_KEY, setLocale } from "./index";

// FUN-22: i18next must resolve {{count}} sentences through the CLDR plural
// category of the active language (en → one/other, ru → one/few/many/other)
// rather than always falling back to the bare key.
describe("plural resolution (FUN-22)", () => {
  afterEach(async () => {
    // setLocale(), not changeLanguage(): non-eager bundles are only registered
    // by the lazy loader, and setLocale is the app's own entry point for it.
    await setLocale("en");
    localStorage.removeItem(LOCALE_STORAGE_KEY);
  });

  it("en selects the _one variant at count=1 and the bare key otherwise", async () => {
    await setLocale("en");
    expect(i18n.t("toast.tasksDeleted", { count: 1 })).toBe("1 task removed");
    expect(i18n.t("toast.tasksDeleted", { count: 3 })).toBe("3 tasks removed");
  });

  it("ru selects its own one/few forms after lazy locale load", async () => {
    await setLocale("ru");
    expect(i18n.t("task.connections", { count: 1 })).toContain("соединение");
    expect(i18n.t("task.connections", { count: 3 })).toContain("соединения");
  });
});
