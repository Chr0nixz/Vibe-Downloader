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

Before submitting changes, run the relevant checks:

```bash
pnpm typecheck
pnpm build
pnpm check:bindings
pnpm test:rust
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Run `pnpm build:extensions` when changing `browser/extension-core` or Native
Messaging documentation.
