# Contributing

Thanks for helping. Spool is small on purpose; please open an issue to
discuss larger changes before writing them.

## Setup

- Rust stable with the `wasm32-unknown-unknown` target, plus clang (ring
  compiles C for WebAssembly)
- `wasm-bindgen-cli` 0.2.129 (must match the `wasm-bindgen` crate) and
  binaryen's `wasm-opt`
- Node 22 and pnpm 10
- For the E2E tests: `par2` and `7z`

```sh
pnpm install
pnpm build:wasm
pnpm dev
```

## Checks

Everything CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --filter @spool/web check
pnpm --filter @spool/web test
bash e2e/run.sh
```

## Guidelines

- **Commits** use [Conventional Commits](https://www.conventionalcommits.org/)
  (`feat(engine): ...`, `fix(relay): ...`), one logical change each.
- **The relay** must stay log-free: one startup line, no logging crate, no
  files. CI enforces this.
- **The engine** stays sans-IO. Test new protocol behaviour natively against
  `tools/mock-nntp`.
- **The UI** follows the black-and-white design: sentence case, no ALL CAPS,
  red only for errors and missing articles, Geist Mono only for technical
  values. Run `bash e2e/run.sh --screens` and look at both themes before
  sending UI changes.
- **Neutral examples only.** Spool is a general NNTP client: use file names
  like `ubuntu-24.04.iso` in code, docs and tests.
- Record decisions that depart from the plan, or fill a gap in it, in
  `docs/decisions.md`.

## Updating vendored builds

- Fonts: `scripts/update-fonts.sh`
- libarchive: `scripts/build-libarchive.sh` (needs Emscripten)
- PAR2 fixtures: `scripts/make-fixtures.sh` (needs par2cmdline)
- README wordmark: `scripts/make-wordmark.py` (needs fontTools)
