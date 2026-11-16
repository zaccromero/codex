# Persistent desktop fork integration candidate

This is an experimental local integration, not a fork of the proprietary desktop
shell source. It stages a copy of an installed desktop app with a small bootstrap
prefix that selects our Codex runtime before vendor initialization. The original
app is not changed or launched. Vendor app bytes are never committed.

## Current state

The owner selected an independent desktop identity with fresh sign-in. The
independent build is named **Drako Codex**, bundle ID `ai.drako.codex.desktop`.
It embeds the complete fork runtime and uses an app-relative manifest, so copying
or restoring this app does not require a login launcher or environment override.

The independent app was installed at `~/Applications/Drako Codex.app`. Its
ad-hoc signature verified, but live launch failed under library validation because
the main executable and framework have no Apple Team ID. The app did not reach
bootstrap or sign-in. The owner chose to retain library validation and use a real
Apple Development identity. No valid identity was available at the last check.
Desktop sign-in, UI, notifications, computer use and restoration remain unverified. The older
vendor-identity candidate is preserved as an unsigned investigation artifact;
it must not be installed as a replacement for ChatGPT.

No OpenAI signing identity, shared keychain groups, app groups or provisioning
profiles are claimed by the independent signature. It uses fresh state under
`~/Library/Application Support/DrakoCodex/{desktop-profile,codex-home}`. Existing
ChatGPT credentials, chats, preferences and running processes are untouched.
Vendor URL/document handlers are not registered, so OAuth/deep-link compatibility
must be tested before calling sign-in supported. No credentials are migrated.

The independent bootstrap sets the vendor's `CODEX_SPARKLE_ENABLED=false` gate.
Updates are manual rebuilds of this integration; the vendor updater must not
replace it with an unmodified distribution. This does not change the installed
vendor app's updater. Local code signing preserves hardened runtime/JIT support;
Gatekeeper and library validation are not disabled.

## Selection contract

The independent bootstrap reads `Contents/Resources/drako-runtime.json` before
the desktop starts a backend. The earlier vendor-identity mode reads an external
manifest at `~/Library/Application Support/DrakoCodex/runtime.json`. It verifies all three executable hashes, permissions,
ownership, and the desktop version, then sets `CODEX_CLI_PATH` in that app process.
An absent manifest, missing companion, or hash/version mismatch stops startup;
there is no silent fallback to the bundled runtime. Login ordering and inherited
environment values therefore cannot select another runtime in this candidate.

This is digest and package validation, **not protocol compatibility certification**.
The desktop still requires authenticated UI, keychain, CUA, notifications and
app-server compatibility tests before deployment. No active session is migrated.

## Build and test without launching an app

From the repository root:

```sh
node --test scripts/desktop-fork/test-runtime-selector.cjs
python3 -B -m unittest discover -s scripts/desktop-fork -p 'test_*.py'
python3 -B scripts/desktop-fork/build_candidate.py --independent \
  --source-app /Applications/ChatGPT.app \
  --runtime-package '/absolute/path/to/verified/fork/package' \
  --output .drako/artifacts/persistent-desktop-candidate-new
python3 -B scripts/desktop-fork/sign_independent.py --identity APPLE_IDENTITY_SHA1 \
  .drako/artifacts/persistent-desktop-candidate-new
python3 -B scripts/desktop-fork/verify_candidate.py \
  .drako/artifacts/persistent-desktop-candidate-new
node scripts/desktop-fork/test-bootstrap.cjs \
  .drako/artifacts/persistent-desktop-candidate-new
```

The last test executes only our extracted prefix with a temporary manifest and
then the selected CLI's `--version`; it does not execute the vendor desktop code.
The builder refuses an existing output path, verifies source ASAR integrity, and
retains a receipt. Verification checks all archive files and block digests and
ensures only the earliest bootstrap content changed. The original app's archive
must remain byte-identical.

## Updating our fork

1. Build a complete immutable runtime package from the chosen fork commit.
2. Run that commit's required tests and sandbox allow/deny regressions.
3. Stage an independent candidate against the chosen desktop version. Keep the
   source commit/build receipt alongside `receipt.json`, which retains pre-signing
   runtime hashes. Signing changes the binary digests; the final embedded manifest
   is re-pinned to the signed copies without modifying the source package.
4. Run the isolated tests, archive verifier, signing verification and extracted
   bootstrap smoke test. The latter executes only the selected CLI's `--version`.
5. Test fresh sign-in and required UI/native features in the independent app before
   treating it as a daily desktop. A valid ad-hoc signature is not notarization or
   proof of authenticated feature compatibility. Never bypass an OS trust warning.
6. When installing or upgrading, leave an existing running app untouched. Retain
   the previous independent package for rollback. Verify actual executor identity
   and sandbox behavior after normal launch and restoration.

The shell is a locally modified vendor distribution, not desktop source code we
can rebuild from scratch. Vendor upgrades require rebuilding and revalidating
this integration. The code does not change the current ChatGPT launch agent or
switch the owner's default app. Those are separate activation steps after the
independent desktop has passed its live acceptance checks.
