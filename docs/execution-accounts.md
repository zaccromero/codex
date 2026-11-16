# Dedicated macOS execution accounts

This opt-in executor mode addresses the account-entry dependency tracked in
[Milo issue 1366](https://github.com/zaccromero/milo.ai/issues/1366).
Implementation and unit checks do not establish installation, activation or CI
success. Native acceptance is required before using it for merge proof.

## Trust and launch order

The host starts a separate `codex-exec-server` with a private stdio connection:

```text
codex-exec-server --listen stdio --execution-account-policy /Library/CodexAccounts/milo-ci.json
```

The full CLI also accepts `codex exec-server` followed by those same arguments.
No other flags, config overrides, credential inputs, remote registration or
network listener are accepted in this mode. The policy and its ancestors must
be root-owned, not group/world-writable and free of symlinks. The installed
executor binary and its ancestors must meet the same checks. Provisioning is
an administrator action; this feature neither installs itself nor edits sudoers.

Before dotenv, helper aliases or configuration/authentication loading, the bootstrap validates the account
name, UID, primary GID, home and non-administrator group membership. It uses the
system `sudo -n` to re-exec only the packaged executor as that numeric UID/GID.
It clears the environment both before sudo and after switching UID, then sets
only HOME, USER, LOGNAME and the provisioned PATH. Direct target-account startup
ignores only the `__CF_USER_TEXT_ENCODING` marker synthesized by CoreFoundation;
that marker is not forwarded to workloads. All other extra or mismatched
environment values are rejected. A missing OS authorization fails without prompting on the RPC
stream, opening another transport or retrying unsandboxed.

After entry, both real/effective UID and GID must match. The server's existing
process manager runs under that account, so process handles, cancellation and
descendant cleanup stay within the same UID. Stdio disconnection shuts down its
session registry; this does not install a persistent listener or background job.

## Policy

Example for an already-provisioned account; verify these identity values on the
host before administrator installation:

```json
{
  "schema_version": 1,
  "username": "milo-ci",
  "uid": 502,
  "gid": 20,
  "home": "/Users/milo-ci",
  "root": "/Users/milo-ci/ci/milo-ci-v1",
  "path": "/opt/homebrew/opt/node@24/bin:/Users/milo-ci/ci/milo-ci-v1/tools/bin:/Users/milo-ci/ci/milo-ci-v1/tools/node_modules/.bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
}
```

Unknown fields, privileged/system identities, mismatched identity records,
relative/ambiguous paths and a root outside the account's home are rejected.
The CI root must already exist. At admission, each command's canonical cwd must
remain within it; an escaping symlink is refused. The native permission profile
remains the filesystem boundary. Account identity is rechecked for each job.

The account policy is a startup binding, **not permission to execute a job**.
The existing trusted controller still owns approval and the immutable session
permission floor. Each process request must carry explicit approved escalation
with retained denied reads and must resolve to the native macOS sandbox. The
existing sandbox compiler receives the request's permission profile unchanged;
file denials and native network policy are not removed to enable account entry.
No raw client may replace the trusted controller or invent approval evidence.

Process environments are rebuilt from the four provisioned values; owner and
request environment values are not forwarded. Executor-local managed proxies
use the existing network-policy callbacks and native sandbox enforcement.
The account advertises proxy launch and review-domain support, but no credential
masking. Requests for owner-side proxy contexts, credential providers, proxies
without native enforcement, shell snapshots or custom argv0 fail closed.
The account service disables configuration loading and capability prewarming.
It advertises no unsupported optional capabilities. Its RPC surface contains
only initialization, identity and process lifecycle
methods; filesystem, configuration, discovery and HTTP methods are unavailable.
Ordinary executors without the flag retain their existing behavior.

## Host integration and activation

Use the existing executor environment provider. An administrator-installed
package can be registered in the host's `environments.toml` without replacing
its default environment:

```toml
[[environments]]
id = "milo-ci"
program = "/Library/CodexAccounts/runtime-94d1ca6a711e/bin/codex-exec-server"
args = ["--listen", "stdio", "--execution-account-policy", "/Library/CodexAccounts/milo-ci.json"]
cwd = "/Users/milo-ci/ci/milo-ci-v1"
```

Do not copy owner credentials or authentication files to the account. Install a
reviewed, complete runtime package through supported administrator controls;
verify its hashes and root ownership. Do not grant blanket passwordless sudo or
remove secret denials. Select the registered environment through supported host
controls and use its private CI checkout as the command cwd. A desktop process
that cannot launch the bootstrap due to an inherited OS sandbox must be repaired
or replaced through the supported host activation path. This feature cannot
remove a sandbox inherited by its own parent.

## Acceptance evidence

### Pinned installer and native evidence (2026-11-16)

`scripts/install/execution_account_sudo.py` upgrades the existing root-owned
`runtime-eb342b19` package to the isolated candidate whose SHA-256 is
`94d1ca6a711e8e7f676e9dd1d4a86eb5714f5a8830485c58cab6558fa74663c2`.
It checks account identity, the existing root-owned policy, host registration,
the saved coding profile, all 16 private-path and 11 secret-glob denials, and
template access. Its fixed paths and hash are specific to this reviewed build;
rebuilding requires a new reviewed digest, not a caller-supplied override.

Run `check` for nonmutating preflight. Run `activate` as the owner with Python
3.11 or newer to install the immutable package and one exact sudoers bootstrap
grant through administrator authentication. It validates the rule with native
`visudo`, refuses existing foreign files, and rolls back its new rule if global
validation fails. The grant has no wildcard, general shell, root target or
SETENV. A root-owned readable receipt contains hashes only, allowing later
activation retries without another administrator prompt. Startup is still
checked on every activation; a stale receipt cannot turn a failed bootstrap
into success.

After installation, `activate` starts the account executor with immediate EOF,
exercising account identity, groups and environment admission without issuing
workloads or approval RPCs. Only after this succeeds does it atomically change
the selected environment's program in `environments.toml`; unrelated entries
and the coding configuration remain unchanged. A connection through the host
is still required to verify workload admission, effective protections and CI.
The installer neither restarts the desktop nor labels CI as passed.

For the owned checkout, the complete administrator activation entry point is:

```sh
/opt/homebrew/bin/python3.11 -I /Users/zacc/code/.worktrees/codex/account-installer-network-1366/scripts/install/execution_account_sudo.py activate
```

Rollback: run the same script with `deactivate` as the owner to restore only the
prior environment program, then `rollback` through administrator execution to
remove only its matching sudoers rule and receipt. Packages and policy remain
available for inspection; no runtime or unrelated files are deleted. Do not
remove a grant while the host is still using that executor.

The supported activation attempt reached `sudo: a password is required`;
installation, host activation and account workload verification are therefore
still incomplete. No host registration changed in that failed attempt.

Milo's native process observations no longer depend on setuid `ps`: a bounded
`libproc` helper queries the selected group and process metadata, preserves
inspection errors, and cross-checks the independent sysctl birth identity at
microsecond precision. The native process suite passed 42/42 in an explicit
Seatbelt profile retaining all 27 denials; 11 synthetic secret fixtures were
denied and three templates were readable. The full affected environment suite
passed 144/144. These are owner-account development checks, not dedicated-account
canonical CI. No denial or system executable was modified. The canonical
helper's separate `pgrep` admission check still needs dedicated-account proof.

Before calling the mechanism active, verify all of the following:

1. Stdio initialization reports the expected root and restricted capabilities.
2. An approved request runs as that account; unapproved or sandbox-free requests
   fail before launch, including when another request supplies a different env.
3. Synthetic secret-file fixtures remain unreadable to workloads and children;
   template fixtures remain readable. Do not probe real credential contents.
4. Cancellation and connection closure leave no owned live descendants.
5. Native process identity/group tests pass in that account.
6. All three exact-head Milo canonical contexts pass; validate the canonical
   receipts and log hashes before the owner publisher posts results.

Failure at any step is retained as a failure, never converted into a skipped
PASS. Do not merge or declare PR1365 ready based on this executor's unit tests.
