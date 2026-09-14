## Summary

<!-- What does this PR change and why? Reference the audit ID (e.g. `ARC-19`) from docs/project-improvement-audit.md when applicable. -->

## Checklist

- [ ] `pnpm check` passes (typecheck + Biome + i18n completeness)
- [ ] `pnpm test:frontend` passes; for UI or bundling changes also `pnpm build`
- [ ] Rust changes: `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` is clean and relevant tests under `src-tauri/tests` are added or updated
- [ ] Rust command/model changes: `pnpm specta` regenerated and `pnpm check:bindings` passes
- [ ] New user-facing copy added to all 7 locale files (`pnpm check:i18n`)
- [ ] `docs/project-improvement-audit.md` statuses updated only where acceptance tests pass
