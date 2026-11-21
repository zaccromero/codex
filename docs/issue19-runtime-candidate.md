# Issue 19 fork runtime candidate

This is a diagnostic build, not a confirmed networking repair. It leaves filesystem and network policy behavior intact.

The task worktree selects the bundled release commit `d2c035639366e0bf2c316aa2065c24aabc12bcdc`, version `0.167.0-alpha.4.1`, from `zaccromero/codex`. The existing task branch and earlier drafts are preserved. No remote branch has been published.

The helper verifies the pinned executor source blob and adds six Boolean tracing fields describing sandbox/network/proxy inputs. The added diagnostics contain no paths, hosts, environment values, credentials or command contents.

The release requires Rust 1.95.0. Its checked-in Cargo.lock still labels 171 local workspace packages 0.0.0 although the workspace manifest has the release version. The helper normalizes only those local version labels, rejects unexpected local packages, retains every external dependency/version/checksum, and includes both changes in the recorded patch. An independent TOML comparison verified this restriction. Tests and builds continue to use --locked.

Local GitHub, official compiler downloads, localhost listeners and Chromium now work through approved execution after the host context changed. An initial Cargo libgit2 TLS failure on a Chromium build dependency was handled with Cargo's supported Git CLI transport and certificate verification retained. Failed-attempt logs and the original draft are preserved in task-owned artifacts.

The local compiler, Cargo cache, verified ARM64 V8 dependencies and build outputs live only under `.drako/artifacts/issue19-runtime-candidate/local-20261115`. Shell settings, permission profiles, installed applications and the bundled runtime are unchanged.

The build runs the existing denied-read escalation regression, macOS sandbox suite and executor process-sandbox tests before packaging. A zero-test filter or failing test blocks packaging. It builds codex, codex-exec-server from the codex-exec-server-cli package, and codex-code-mode-host. The manifest records source/patch/binary/harness hashes, compiler versions and test counts. The archive preserves executable modes. No Gatekeeper or quarantine settings are changed.

The draft CI workflow is optional and unpublished. It uses pinned actions, read-only token access, a native macOS ARM64 runner and checksum-verified upstream V8 artifacts. Publishing a remote branch still requires separately named authorization under the standing repository-restoration hold.

The Milo poster candidate has independently passed all four Home desktop checks and the full 31-check suite on both staging and the applied 8889 preview. C-13/C-14 and earlier source/build versions remain preserved. Those successful checks used the existing desktop execution context; they do not prove that this fork fixed the earlier host restriction. No custom runtime has been activated, and no merge, push or deployment has occurred.
