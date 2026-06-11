## Vibe Downloader — Technical Audit Report

### Audit Health Score

| # | Dimension | Score | Key Finding |
|---|-----------|-------|-------------|
| 1 | Accessibility | 2 | Light mode contrast failures across nearly all semantic colors; floating window lacks keyboard path |
| 2 | Performance | 3 | Good virtualization and lazy loading; minor concerns with undebounced resize and unbounded state growth |
| 3 | Responsive Design | 3 | Solid breakpoint strategy and adaptive layout; touch targets acceptable for desktop-first Tauri app |
| 4 | Theming | 3 | Excellent OKLCH token architecture; critical light mode contrast gap due to tokens diverging from DESIGN.md |
| 5 | Anti-Patterns | 4 | No AI tells detected; distinctive, intentional design with zero slop markers |
| **Total** | | **15/20** | **Good — address weak dimensions** |

**Rating:** 14-17 = Good (address weak dimensions)

---

### Anti-Patterns Verdict

**Pass.** This interface does not look AI-generated. The detect script returned zero hits.

Specifically: no gradient text, no decorative glassmorphism, no side-stripe borders, no hero-metric template, no identical card grids (the task list is a proper dense list), no tiny uppercase eyebrows on every section, no numbered section markers, no hand-drawn SVG illustrations, no diagonal stripe backgrounds. The `backdrop-blur-xl` on TrayMenu and `bg-surface-overlay` with transparency on dialogs are used purposefully for floating surfaces — this is structural glass, not decorative. The border-radius values stay within 4–8px on cards/inputs and full-pill only on progress bars and scrollbars. The `shadow-xl` on CommandBar's dropdown and TaskDetails' drawer are paired with borders in a few places, but they serve real depth separation for overlay UI, not decoration.

The design has a clear, intentional identity: Fluent-inspired desktop utility with restrained accent palette and dense information layout. This reads as a human-designed product.

---

### Executive Summary

- **Audit Health Score:** 15/20 (Good)
- **Total issues found:** 18 (P0: 1, P1: 5, P2: 8, P3: 4)
- **Top critical issues:**
  1. Light mode tokens fail WCAG AA contrast on almost every semantic color pair
  2. `index.html` has hard-coded `lang="en"` for a 7-locale application
  3. Floating status window has no keyboard interaction path
  4. Dynamic error/status messages lack ARIA live regions in multiple components
  5. Toast auto-dismiss (4.8s) has no pause-on-hover/focus mechanism

---

### Detailed Findings by Severity

#### [P0] Light mode contrast fails WCAG AA across all semantic colors

- **Location:** `src/styles/tokens.css`, light mode (`:root` block)
- **Category:** Accessibility / Theming
- **Impact:** Nearly all secondary text, muted text, accent colors, and status colors fail the 4.5:1 minimum contrast ratio in light mode. This makes the light theme unusable for users with low vision and violates WCAG AA.
- **Measured ratios:**

| Pair | Ratio | AA Normal | AA Large |
|------|-------|-----------|----------|
| text.secondary on surface.root | 2.81:1 | FAIL | FAIL |
| text.muted on surface.root | 2.32:1 | FAIL | FAIL |
| accent.primary on surface.root | 2.51:1 | FAIL | FAIL |
| accent.energy on surface.root | 2.40:1 | FAIL | FAIL |
| status.danger on surface.root | 2.67:1 | FAIL | FAIL |
| status.success on surface.root | 2.66:1 | FAIL | FAIL |

- **Root cause:** The light mode tokens in `tokens.css` diverge from DESIGN.md. DESIGN.md specifies `text.muted` at L=0.58, `accent.primary` at L=0.58, `accent.energy` at L=0.58, but `tokens.css` uses L=0.50 for all three. Even the DESIGN.md values may not be dark enough — 0.58 on 0.975 yields approximately 3.5:1.
- **Recommendation:** Darken all light mode chromatic tokens. For body text, target L ≤ 0.42 for secondary and L ≤ 0.48 for muted. For accent colors used as text (buttons with transparent backgrounds, links, status labels), target L ≤ 0.40. For accent colors used only on opaque backgrounds (filled buttons), the current values are fine.
- **WCAG:** 1.4.3 Contrast (Minimum)
- **Suggested command:** `$impeccable adapt`

---

#### [P1] `index.html` hard-codes `lang="en"` despite 7 supported locales

- **Location:** `index.html`, line 2
- **Category:** Accessibility
- **Impact:** Screen readers in Chinese, Japanese, Korean, Russian, or Spanish will attempt to pronounce all content as English, producing garbled audio output.
- **Recommendation:** Dynamically set `document.documentElement.lang` when the locale changes in the i18n module. The `setLocale` function in `src/i18n` should sync the `lang` attribute.
- **WCAG:** 3.1.1 Language of Page
- **Suggested command:** `$impeccable harden`

---

#### [P1] FloatingStatusWindow has no keyboard interaction path

- **Location:** `src/components/shell/FloatingStatusWindow.tsx`
- **Category:** Accessibility
- **Impact:** Keyboard-only users cannot close the floating window, refocus the main window, or interact with any information it displays. The close button is reachable by Tab, but the double-click-to-focus-main-window action has no keyboard equivalent.
- **Recommendation:** Add a keyboard shortcut (e.g., Escape to close, Enter to focus main window) and ensure the close button is reachable via Tab.
- **WCAG:** 2.1.1 Keyboard
- **Suggested command:** `$impeccable adapt`

---

#### [P1] Dynamic error and status messages lack ARIA live regions

- **Location:** Multiple components:
  - `NewDownloadDialog.tsx` — error div, submit status, batch results
  - `TaskRecoveryActions.tsx` — error container
  - `TaskDetails.tsx` — error displays in ChunkList, ConnectionList, EventList, RequestList
- **Category:** Accessibility
- **Impact:** When errors or status messages appear dynamically, screen readers do not announce them. Users are unaware of failures or state changes unless they navigate to the element.
- **Recommendation:** Add `role="alert"` to error containers and `role="status"` with `aria-live="polite"` to status indicators.
- **WCAG:** 4.1.3 Status Messages
- **Suggested command:** `$impeccable harden`

---

#### [P1] Toast auto-dismiss lacks pause mechanism

- **Location:** `src/components/ui/toast.tsx` (4800ms timer)
- **Category:** Accessibility
- **Impact:** Users who need more time to read or interact with toasts (especially those using assistive technology) cannot extend the display duration. If a user focuses the action button inside a toast and the timer fires, focus is lost.
- **Recommendation:** Pause the auto-dismiss timer on hover or focus within the toast. Manage focus when a toast auto-dismisses (return to previous focus target or the toast container).
- **WCAG:** 2.2.1 Timing Adjustable
- **Suggested command:** `$impeccable harden`

---

#### [P1] Dark mode `text.muted` on `surface.raised` fails AA for normal text

- **Location:** `src/styles/tokens.css`, dark mode
- **Category:** Accessibility / Theming
- **Impact:** `text.muted` (L=0.62) on `surface.raised` (L=0.235) yields 3.44:1, which fails the 4.5:1 requirement for normal-sized text. This combination is used for labels, descriptions, and helper text in cards and dialogs.
- **Recommendation:** Either darken `text.muted` to L=0.68+ in dark mode, or ensure muted text is only used at large sizes (≥18px or bold ≥14px) where the 3:1 threshold applies.
- **WCAG:** 1.4.3 Contrast (Minimum)
- **Suggested command:** `$impeccable adapt`

---

#### [P2] TrayMenu lacks proper ARIA menu semantics

- **Location:** `src/components/shell/TrayMenu.tsx`
- **Category:** Accessibility
- **Impact:** The menu items are plain buttons in a div without `role="menu"` / `role="menuitem"`. Screen readers do not announce this as a menu, and arrow-key navigation between items is not implemented.
- **Recommendation:** Wrap items in `role="menu"` and use `role="menuitem"` on each button. Add arrow-key navigation.
- **Suggested command:** `$impeccable harden`

---

#### [P2] `bulkDelete` uses native `window.confirm()` instead of design system dialog

- **Location:** `src/components/shell/AppShell.tsx`
- **Category:** Accessibility / Theming
- **Impact:** The native confirm dialog is not themeable, does not match the app's design, and is not accessible in a consistent way with the rest of the application.
- **Recommendation:** Use the existing `DeleteTaskDialog` component with a bulk variant, or create a generic confirmation dialog.
- **Suggested command:** `$impeccable polish`

---

#### [P2] TaskDetails `<aside>` lacks accessible name

- **Location:** `src/components/shell/TaskDetails.tsx`
- **Category:** Accessibility
- **Impact:** The detail panel uses `<aside>` without `aria-label` or `aria-labelledby`. Screen readers cannot identify this landmark's purpose.
- **Recommendation:** Add `aria-labelledby` pointing to the `<h2>` heading inside.
- **Suggested command:** `$impeccable harden`

---

#### [P2] StatusBar lacks live region for changing download statistics

- **Location:** `src/components/shell/StatusBar.tsx`
- **Category:** Accessibility
- **Impact:** Speed and count values update frequently but are not announced to screen readers.
- **Recommendation:** Wrap the speed/count area in a `role="status"` span with `aria-live="polite"` and a throttled update.
- **Suggested command:** `$impeccable harden`

---

#### [P2] Recovery action icons lack `aria-hidden`

- **Location:** `src/components/tasks/TaskRecoveryActions.tsx`
- **Category:** Accessibility
- **Impact:** Icons inside buttons with text labels may be announced by screen readers, causing redundant output.
- **Recommendation:** Add `aria-hidden` to all decorative icons inside labeled buttons.
- **Suggested command:** `$impeccable harden`

---

#### [P2] Tab active indicator relies solely on subtle background color

- **Location:** `src/components/ui/tabs.tsx`
- **Category:** Accessibility
- **Impact:** The active tab is indicated only by a `bg-surface-raised` background change, which may not be perceivable for users with low vision.
- **WCAG:** 1.4.1 Use of Color
- **Recommendation:** Add a bottom border, increased font weight, or underline to the active tab trigger.
- **Suggested command:** `$impeccable polish`

---

#### [P2] `use-shell-layout.ts` resize handler has no debounce

- **Location:** `src/hooks/use-shell-layout.ts`
- **Category:** Performance
- **Impact:** Every pixel of window resize triggers a state update and re-render. During drag-resize, this fires dozens of times per second.
- **Recommendation:** Debounce or throttle the resize handler (e.g., `requestAnimationFrame` or 100ms throttle).
- **Suggested command:** `$impeccable optimize`

---

#### [P2] `notifiedStatuses` Set grows unboundedly

- **Location:** `src/hooks/use-task-events.ts`
- **Category:** Performance
- **Impact:** The `Set<string>` tracking notification state never prunes entries for completed or removed tasks. Over long sessions with many tasks, this leaks memory.
- **Recommendation:** Prune entries when tasks are removed from the store, or cap the Set size.
- **Suggested command:** `$impeccable optimize`

---

#### [P2] TaskRow selected state uses `ring-1` + `border` double outline

- **Location:** `src/components/tasks/TaskRow.tsx`
- **Category:** Theming
- **Impact:** The selected row shows both `ring-1 ring-accent-primary/45` and `border-accent-primary/35`, creating a double outline that can look like a rendering artifact.
- **Recommendation:** Choose one (ring or border) for the selected state indicator.
- **Suggested command:** `$impeccable polish`

---

#### [P3] `<textarea>` used instead of `<Input>` component for batch URLs

- **Location:** `src/components/shell/NewDownloadDialog.tsx`
- **Category:** Theming (consistency)
- **Impact:** The raw textarea may have subtly different focus ring styling vs. the Input component. Currently mitigated by explicit `focus-visible:ring-2` classes.
- **Suggested command:** `$impeccable polish`

---

#### [P3] Tooltip and speed menu use `shadow-*` + `border` simultaneously

- **Location:** `src/components/ui/tooltip.tsx`, `src/components/shell/CommandBar.tsx`
- **Category:** Theming
- **Impact:** The shadow+border combination on tooltips and the speed menu dropdown may create visually heavy overlays.
- **Recommendation:** Consider removing the border from tooltips and relying on shadow alone.
- **Suggested command:** `$impeccable polish`

---

#### [P3] `opacity-65` magic number on disabled Palette commands

- **Location:** `src/components/shell/Palette.tsx`
- **Category:** Theming
- **Impact:** The Button component uses `opacity-50` for disabled state while the Palette uses `opacity-65`. Inconsistent disabled opacity across the app.
- **Recommendation:** Standardize on one disabled opacity value via a token or utility class.
- **Suggested command:** `$impeccable polish`

---

#### [P3] Hard-coded oklch shadow in FloatingStatusWindow

- **Location:** `src/components/shell/FloatingStatusWindow.tsx`
- **Category:** Theming
- **Impact:** `shadow-[0_16px_34px_oklch(0.12_0.01_255_/_0.28)]` is an arbitrary Tailwind value rather than a named token.
- **Suggested command:** `$impeccable polish`

---

### Patterns & Systemic Issues

**Light mode token drift from DESIGN.md.** The most impactful systemic issue is that the light mode tokens in `tokens.css` do not match the values specified in `DESIGN.md`. Three key tokens (`text.muted`, `accent.primary`, `accent.energy`) have L values 0.08 lower than designed, causing widespread contrast failures. Even the DESIGN.md values may need further darkening for text usage. Fixing this at the token level resolves many contrast issues in one change.

**Missing ARIA live regions for dynamic content.** Error messages, status updates, and recovery actions appear dynamically across multiple components but consistently lack `role="alert"` or `aria-live` attributes. This indicates a systemic gap in the accessibility layer rather than isolated oversights. A utility wrapper or convention for dynamic feedback regions would address this holistically.

**Inconsistent disabled state opacity.** The Button component uses `opacity-50` while the Palette uses `opacity-65`. Standardizing on a single disabled opacity token would improve visual consistency.

---

### Positive Findings

**Excellent token architecture.** The OKLCH-based token system with Tailwind v4's `@theme inline` bridge is clean, modern, and well-structured. Dark mode tokens are well-calibrated and pass WCAG AA across the board. The design token coverage is comprehensive: surfaces, borders, text, accents, and status colors are all accounted for.

**Strong ARIA on complex patterns.** The command Palette implements a textbook combobox/listbox pattern with `aria-activedescendant`, `aria-controls`, and proper `role="option"`. The speed limit dropdown implements the full WAI-ARIA menu pattern with arrow keys, Home, End, and Escape. The task list uses a proper roving-tabindex listbox with `aria-posinset` and `aria-setsize`. These are difficult patterns to get right, and they are implemented correctly.

**Consistent token usage across all components.** Not a single hard-coded hex or rgb color was found in any component file. Every color reference goes through the design token system via Tailwind utility classes. This is exceptional discipline.

**Reduced motion support.** The `useReducedMotion` hook is used in `TaskRow`, `SpeedSparkline`, `dialog.tsx`, and `toast.tsx`. The progress bar uses Tailwind's `motion-reduce:` variant. This is a thoughtful, consistent approach to motion accessibility.

**Semantic HTML throughout.** Proper use of `<nav>`, `<header>`, `<main>`, `<footer>`, `<section>`, `<form>`, `<label>`, `<kbd>`, and heading elements. No `<div>` buttons or missing landmarks.

**No AI aesthetic tells.** The design is distinctive and intentional. The Fluent-inspired + Geek Chic direction reads as a genuine desktop utility, not a generic SaaS template.

---

### Recommended Actions

1. **[P0] `$impeccable adapt`**: Fix light mode contrast tokens in `tokens.css` — darken `text.secondary`, `text.muted`, `accent.primary`, `accent.energy`, and all status colors for light mode to meet WCAG AA (4.5:1 for text, 3:1 for large text/UI components).

2. **[P1] `$impeccable harden`**: Add dynamic `lang` attribute syncing when locale changes; add `role="alert"` and `aria-live` regions to all dynamic error/status containers across `NewDownloadDialog`, `TaskRecoveryActions`, `TaskDetails`; add pause-on-hover/focus to toast auto-dismiss; add ARIA menu semantics to `TrayMenu`; add `aria-label`/`aria-labelledby` to `TaskDetails` aside; add `aria-hidden` to recovery action icons.

3. **[P1] `$impeccable adapt`**: Fix dark mode `text.muted` contrast on `surface.raised` (3.44:1 → needs 4.5:1 for normal text).

4. **[P2] `$impeccable optimize`**: Debounce the resize handler in `use-shell-layout.ts`; cap or prune the `notifiedStatuses` Set in `use-task-events.ts`.

5. **[P2] `$impeccable polish`**: Replace `window.confirm()` in bulk delete with a proper dialog; add non-color indicator to active tab; standardize disabled opacity; remove border from tooltips; fix double-outline on selected TaskRow.

6. **[P2] `$impeccable harden`**: Add keyboard interaction path to `FloatingStatusWindow` (Escape to close, Enter to focus main window).

7. **[P3] `$impeccable polish`**: Final consistency pass — standardize textarea/Input usage, magic number opacity, hard-coded shadow values.

> You can ask me to run these one at a time, all at once, or in any order you prefer.
>
> Re-run `$impeccable audit` after fixes to see your score improve.
