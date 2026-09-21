# Vibe Hardware

An editorless desktop shell for conversational embedded development: React + Tauri v2
driving the PlatformIO Core CLI and the Claude Code CLI over a Rust command bridge.

See [`docs/spec/README.md`](./docs/spec/README.md) for the full specification pack and
[`CLAUDE.md`](./CLAUDE.md) for repo conventions.

## Development

```bash
npm install
npm run tauri dev
```

```bash
npm run test        # vitest
npm run typecheck    # tsc --noEmit
npm run test:rust    # cargo test
npm run lint:rust     # cargo clippy -D warnings
npm run check:bindings # regenerate src/lib/bindings.ts and fail on drift
```
