# Contributing

Thank you for your interest in contributing to Vibe Downloader.

## License

By submitting a contribution, you agree that:

- Your contribution is your original work, or you have the right to submit it.
- Your contribution is licensed under `GPL-3.0-only` for inclusion in this repository.
- You grant the project owner a perpetual, worldwide, non-exclusive, royalty-free right to use, modify, sublicense, and relicense your contribution, including under commercial license terms.

This keeps the public project available under GPL-3.0-only while preserving the
option to offer separate commercial licensing in the future.

## Development

Read [AGENTS.md](AGENTS.md) before changing code. For bug-fix work, use the stable IDs and acceptance criteria in [docs/project-improvement-audit.md](docs/project-improvement-audit.md); revalidate the cited implementation before editing and update the audit status only after the required tests pass.

Before submitting changes, run the relevant checks:

```bash
# 与 CI 等价的全量门禁（前端 + Rust）：
pnpm verify

# 快速静态检查：
pnpm check
pnpm check:bindings
pnpm verify:protocol-matrix
```

Run `pnpm verify:extensions` when changing `browser/extension-core`, Native Messaging behavior, browser permissions, or related documentation. Run `pnpm specta` before `pnpm check:bindings` when Rust IPC models or command signatures change.
