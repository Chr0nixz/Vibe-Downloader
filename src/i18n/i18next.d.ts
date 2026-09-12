import "i18next";

import type en from "./locales/en";

/**
 * Widens every leaf to `string`. i18next otherwise derives per-key interpolation
 * options from the literal values, which breaks as soon as the key is a union —
 * `t(someKey: TranslationKey, { count })` must be valid for every member.
 * Interpolation contracts are still checked, by `check:i18n`'s placeholder
 * comparison across all seven bundles.
 */
type WidenLeaves<T> = {
  [K in keyof T]: T[K] extends string ? string : WidenLeaves<T[K]>;
};

/**
 * FUN-29: makes `t()` keys type-checked against the English reference bundle.
 * Without this augmentation `TFunction` accepts any string, so a typo compiles
 * and the raw key is rendered to users; `pnpm check:i18n` catches that for
 * literal keys, but only after a round trip through CI.
 *
 * The counterpart for key *tables* (a `labelKey` field, or any array of keys) is
 * `TranslationKey` in `src/i18n/index.ts` — a union of every dotted leaf path.
 */
declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "translation";
    resources: {
      translation: WidenLeaves<typeof en>;
    };
  }
}
