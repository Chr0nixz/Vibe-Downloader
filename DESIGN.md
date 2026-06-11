# Vibe Downloader Design System Context

## Design Register

Product UI. Design serves repeated task management, download diagnosis, and daily desktop utility.

## Visual Direction

The design language is Fluent-inspired + Geek Chic.

Default surfaces should feel familiar, native, and attractive to mainstream users. Advanced engine information should appear through deliberate expansion, not in the default scan path.

Design keywords:

- quiet
- precise
- native
- fast
- trustworthy
- refined

## Theme Strategy

Dark mode is the primary craft target, but the app should follow the system theme by default.

Dark theme scene:

A user keeps the app open beside a browser or code editor while downloading large files in the evening. The app should be low-glare, readable, and calm, with vivid highlights only when activity deserves attention.

Light theme scene:

A user checks downloads during office work in a bright environment. The app should look clean and native, with enough contrast for dense task rows and status indicators.

## Color Strategy

Use a restrained base palette with purposeful high-energy accents.

Rules:

- Neutrals form most of the interface.
- Accent color is used for primary actions, active navigation, progress, focus, and selected states.
- High-energy colors are reserved for active speed moments, completion feedback, and engine visualizations.
- Paused, queued, inactive, and low-priority states use desaturated gray.
- Error and warning colors must remain immediately recognizable and accessible.
- Avoid a one-note purple, blue, beige, brown, or slate-only palette.

Recommended dark tokens:

- `surface.root`: `oklch(0.145 0.006 255)`
- `surface.base`: `oklch(0.18 0.008 255)`
- `surface.raised`: `oklch(0.235 0.01 255)`
- `surface.overlay`: `oklch(0.245 0.012 255 / 0.78)`
- `border.subtle`: `oklch(0.33 0.012 255)`
- `text.primary`: `oklch(0.93 0.006 255)`
- `text.secondary`: `oklch(0.72 0.008 255)`
- `text.muted`: `oklch(0.78 0.008 255)`
- `accent.primary`: `oklch(0.72 0.14 235)`
- `accent.energy`: `oklch(0.78 0.18 170)`
- `accent.peak`: `oklch(0.74 0.18 305)`
- `status.success`: `oklch(0.72 0.14 150)`
- `status.warning`: `oklch(0.78 0.14 75)`
- `status.danger`: `oklch(0.68 0.18 25)`

Recommended light tokens:

- `surface.root`: `oklch(0.975 0.004 255)`
- `surface.base`: `oklch(0.955 0.006 255)`
- `surface.raised`: `oklch(0.99 0.003 255)`
- `border.subtle`: `oklch(0.84 0.01 255)`
- `text.primary`: `oklch(0.22 0.012 255)`
- `text.secondary`: `oklch(0.28 0.015 255)`
- `text.muted`: `oklch(0.39 0.01 255)`
- `accent.primary`: `oklch(0.30 0.16 235)`
- `accent.energy`: `oklch(0.26 0.14 165)`
- `status.danger`: `oklch(0.45 0.18 25)`

## Typography

Use system-native product typography.

Recommended stack:

```css
font-family: var(--vibe-font-sans);
```

For data-heavy values:

```css
font-family: "Geist Mono", "JetBrains Mono", "Cascadia Mono", ui-monospace, monospace;
font-variant-numeric: tabular-nums;
```

Rules:

- Use fixed rem sizes, not viewport-scaled type.
- Body copy should stay within 65 to 75 characters when prose is shown.
- Dense UI labels may be compact, but must remain readable at 100 percent scaling.
- Download speed, ETA, file size, percentages, and connection counts must not cause layout jitter.

## Layout

Primary structure:

- Custom titlebar and command bar.
- Left navigation.
- Dense task list.
- Optional right detail panel.
- Bottom global status bar.

Responsive behavior:

- Wide window: navigation, task list, and detail panel can be visible together.
- Medium window: detail panel becomes an inline expandable drawer or overlay panel.
- Narrow window: task rows become compact single-column cards, but the default task model remains list-based.

Do not use a default grid of large task cards for the main task manager. It reduces scan speed and weakens comparison between tasks.

## Components

Core components:

- Task row
- Task row expanded detail
- Chunk heatmap
- Connection list
- Command palette
- Navigation sidebar
- Status bar
- Info bar
- Settings controls
- Toast notification
- Confirmation sheet for destructive actions

Component requirements:

- Every interactive component needs default, hover, focus, active, disabled, loading, and error states.
- Icon buttons need tooltips and accessible labels.
- Primary destructive actions need confirmation or undo.
- Focus rings must be visible in both dark and light themes.
- Color cannot be the only state indicator.

## Motion

Motion should communicate state.

Use motion for:

- Expanding task details.
- Reordering task rows.
- Opening command palette.
- Completion feedback.
- Progress interpolation.
- Error attention when user action is required.

Avoid:

- Page-load choreography.
- Slow decorative spring animations.
- Animated layout changes for every progress tick.
- Motion that delays task actions.

Timing:

- Most UI transitions: 150 to 250 ms.
- Command palette: 120 to 180 ms.
- Detail expansion: 180 to 260 ms.
- Progress visual interpolation can be continuous, but real engine events should be batched.

## Glass And Mica

Use glass or Mica-style surfaces sparingly.

Allowed:

- Titlebar.
- Sidebar.
- Command palette.
- Floating sheets.
- Context menus.

Avoid:

- Full task list blur.
- Nested glass cards.
- Blur behind dense text.
- Decorative glass panels that do not communicate hierarchy.

## Data Visualization

Engine visualizations should feel precise and useful, not decorative.

Use:

- Chunk heatmaps for range progress.
- Small sparklines for speed history.
- Connection health indicators.
- Disk I/O and network bottleneck labels.
- Server capability badges such as Range supported, resumable, limited, changed.

Default view should summarize diagnosis in plain language. Detail view can expose technical evidence.

## Accessibility

- Keyboard navigation must cover all primary workflows.
- Command palette is an accelerator, not the only path.
- All state changes need accessible text equivalents.
- Minimum touch or click target should be 32 px for desktop dense UI and 40 px where practical.
- Text contrast should meet WCAG AA.
- The chunk heatmap needs a textual summary and tooltip details.

## Design Bans

- No gradient text.
- No large decorative glassmorphism.
- No side-stripe accent borders.
- No generic identical card grids for core task management.
- No full-saturation inactive states.
- No invented controls where standard desktop controls work better.
- No hidden advanced state that makes errors impossible to diagnose.
