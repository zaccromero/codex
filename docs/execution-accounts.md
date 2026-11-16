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
rejects extra or mismatched environment values. A missing OS authorization fails without prompting on the RPC
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
request environment values are not forwarded. This initial implementation
refuses shell snapshots, custom argv0 and managed-network proxy requests. These
unsupported modes fail closed instead of losing isolation or proxy enforcement.
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
program = "/Library/CodexAccounts/runtime/bin/codex-exec-server"
args = ["--listen", "stdio", "--execution-account-policy", "/Library/CodexAccounts/milo-ci.json"]
cwd = "/"
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
