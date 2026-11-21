# Coding directory cleanup: host-source candidate

Status: **development candidate activated and verified live on 2026-11-15**.

Scoped denied globs such as `/workspace/**/.env.*` require ancestor-move
protection, otherwise moving a directory outside `/workspace` would expose the
denied contents. Seatbelt uses directory-unlink denies for that protection,
which also blocks empty-directory removal throughout the workspace. Merely
adding writable roots or approving escalation does not remove that restriction.

This candidate extends the existing globally anchored literal-basename case to
one trailing wildcard. Global `/**/.env` and `/**/.env.*` remain effective after
an ancestor moves anywhere, so ordinary ancestors need not be denied unlink.
The generated read/write denies include descendants of a matching directory.
Scoped, multi-component and complex wildcard rules retain the existing
conservative behavior. No scoped deny is silently promoted or dropped.

After a tested compatible host release is installed, the corresponding profile
migration would replace only the two scoped `.env` entries with:

```toml
[permissions.coding.filesystem."/"]
"**/.env" = "deny"
"**/.env.*" = "deny"
```

All explicit credential-path denials and unrelated policy entries must remain.
The migration has NOT been applied. Applying this configuration to a host that
does not implement the fix could broaden the directory-cleanup problem.

## Evidence

- The production helper was compiled directly in a module test harness using
  the existing Rust 1.95.0 compiler: 2 unit tests passed (literal/prefix allow
  cases and scoped/complex rejection cases).
- `global_basename_denies_survive_directory_moves` now also covers `.env.local`
  and `.env.`. Its existing native journey checks mkdir/rmdir, build output
  rename/removal, protected files/directories, symlinks, scoped protected paths,
  ancestor moves and late-created matches. This integration test passed in the
  owner-Terminal native run on 2026-11-15.
- `git diff --check` passes.
- The owner-Terminal runner installed Rust 1.99.1 successfully. The suite has
  not reached compilation: Cargo's built-in Git transport failed its TLS
  handshake while fetching the Chromium `buildtools` submodule. The runner now
  uses system Git with normal certificate verification and HTTP/1.1. An
  in-chat dependency retry was blocked removing the partial V8 checkout by the
  existing directory-unlink restriction. The corrected owner-Terminal run
  completed successfully: 126 passed, 0 failed, 3 ignored; exit code 0.
- The installed Codex sandbox diagnostic returns `sandbox-exec: sandbox_apply:
  Operation not permitted`, including approved escalation. No alternate API was
  used to circumvent the host restriction.

Before release, run the complete sandboxing suite with this checkout's pinned
Rust toolchain on a host that supports the native sandbox tests:

```sh
cd codex-rs
cargo test --locked -p codex-sandboxing
```

Then build a compatible host release, use its supported installation path,
apply the stronger global-name profile, refresh the active Coding profile,
and prove real workspace cleanup plus protected-name denial using synthetic
fixtures. The production credential files must never be used as test fixtures.

## Packaged candidate and activation handoff

The existing `native-host/package` passes the repository's package layout
validator. All three executables match their Cargo output SHA-256 hashes.
The CLI starts, strict startup-config validation passes, and app-server
`initialize` passes with isolated state. The latest repeated builder invocation
returned 1 because the complete output directory already existed; that is not
a successful repeat build. Its linker/strip warnings did not prevent executable
startup. Version remains honestly reported as development version `0.0.0`.

`activate-candidate.py --check-only` validates package hashes and the exact
semantic config migration without changing settings. Owner-Terminal activation
requires the desktop app to be closed; it never terminates chats. Before launch
it runs a packaged-executable native cleanup/protected-fixture smoke test,
backs up SQLite databases and config, migrates only Coding's secret glob table,
validates config, and uses the desktop's `CODEX_CLI_PATH` override. The candidate
adds state migration 63, an additive table. The override persists for this
macOS login; after logout the launcher must be used again. `--rollback` restores
the unchanged activation config and prior override without overwriting chats.
Neither packaged native smoke nor desktop activation has yet run. Live parent
and child cleanup verification remains required after relaunch.

Latest owner-Terminal activation attempt passed the packaged native smoke:
directory cleanup succeeded and the synthetic protected fixture was denied.
Desktop launch then returned "Opening in existing browser session." The
launcher restored the old config and CLI override; no success receipt exists.
Live parent and fresh-child approval canaries pass under `auto_review`, but
both cleanup probes still fail at rmdir with EPERM. The running-process check
now matches full executable commands for both ChatGPT and Codex branding.
The app must be fully quit, not merely have its window closed, before retry.


## Final live verification

The next activation produced a launch receipt and loaded global `.env` deny
rules with `coding` and `auto_review`. Parent cleanup in both the Codex task
checkout and Milo passed mkdir/write/read/unlink/rmdir; loopback passed.
Fresh subagent `final_live_verification` inherited auto_review and passed the
same cleanup/loopback journey. Parent and child outside-root approval canaries
passed with require_escalated and no prefix grant. These results supersede the
previous activation/cleanup blockers. No denied files were accessed in live
verification. Existing unrelated chats were not individually inspected.

Activation uses the development package and login-session CLI override, not a
signed app replacement. After logout, use the activation launcher again before
opening the desktop. Config has subsequently changed from the activation hash;
rollback's preservation check will require reconciling those changes rather
than overwriting them. Source remains in task worktrees, not merged.
