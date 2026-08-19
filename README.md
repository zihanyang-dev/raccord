# Raccord

Agent-native, headless, deterministic media post-production runtime.

## Workspace

The Rust workspace is intentionally split by stable boundaries. Domain modules
remain inside their owning crate until they need independent reuse, release, or
runtime isolation.

Install the pinned toolchain and run workspace commands through mise:

```bash
mise install
mise exec -- cargo check --workspace
mise exec -- cargo test --workspace
```

See [`docs/architecture.md`](docs/architecture.md),
[`docs/roadmap.md`](docs/roadmap.md), and
[`docs/rust-style-guide.md`](docs/rust-style-guide.md).
