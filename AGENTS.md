# Codex fork

`vendor/codex` is the owner's upstream-derived fork, not the installed Codex app. Preserve upstream behavior outside the requested fix. Rust source lives in `codex-rs/`; SDKs, maintenance scripts and workflows have separate checks. Read nested `AGENTS.md` files for touched crates.

## Toolchain and checks
- Use `codex-rs/rust-toolchain.toml` and root `package.json`/`pnpm-lock.yaml`; do not select a toolchain from memory. Root npm scripts are maintenance tasks, not the Rust test suite.
- `just fmt-check`: repository formatting check; `just fmt` writes formatting.
- `just test -p <crate>`: focused nextest run using the repository's local profile and stack setting. Choose the actual Cargo package name from its manifest. `just clippy -p <crate>` checks that crate; `just fix` rewrites files.
- `just test-github-scripts`: Python workflow-script tests.
- Changes to configuration/protocol/hooks types may require `just write-config-schema`, `just write-app-server-schema` or `just write-hooks-schema`; include regenerated artifacts and focused tests together.
- Full/platform-specific gates are defined in `.github/workflows/rust-ci.yml`, `rust-ci-full.yml`, `blocking-ci.yml` and the affected workflow. Do not describe one crate's tests as all-platform certification.

## Boundaries
- Preserve sandbox, approval and network-policy enforcement; test allowed and denied cases with isolated fixtures. Do not weaken a boundary to make a host-restricted test pass.
- Building a CLI/package does not replace the desktop app or install an executor. Local fork proof, installed-runtime proof and activation are distinct.
- Keep source/build artifacts in the owned worktree or existing tool cache. Do not edit another active fork checkout or change its running executor.
