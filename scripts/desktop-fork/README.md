# Persistent desktop fork integration candidate

This is an experimental local integration, not a fork of the proprietary desktop
shell source. It stages a copy of an installed desktop app with a small bootstrap
prefix that selects our Codex runtime before vendor initialization. The original
app is not changed or launched. Vendor app bytes are never committed.

## Current state

Source implemented; candidate staged; isolated tests available. **Not signed,
not installed, and desktop login/restore persistence is not verified.** The
runtime itself is independently installed. A running executor was verified against
the fork SHA-256, with successful ordinary directory deletion and denial of
synthetic `.env.local` creation. That does not prove this candidate is active.

The installed desktop has OpenAI team-specific entitlements for shared keychain,
notifications, associated domains and application groups. An ad-hoc signature
cannot preserve that developer identity. We do not strip entitlements, disable
Gatekeeper, re-sign the app, or substitute this candidate into a running app.
Those compatibility and signing requirements remain an actual deployment blocker.

## Selection contract

The bootstrap reads `~/Library/Application Support/DrakoCodex/runtime.json` before
the desktop starts a backend. It verifies all three executable hashes, permissions,
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
python3 -B scripts/desktop-fork/build_candidate.py \
  --source-app /Applications/ChatGPT.app \
  --runtime-package '/absolute/path/to/verified/fork/package' \
  --output .drako/artifacts/persistent-desktop-candidate-new
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
3. Stage a new candidate against the chosen desktop version. Keep the source
   commit and package build receipt alongside the candidate receipt.
4. Verify bootstrap selection and CLI identity, then resolve desktop signing and
   authenticated feature compatibility in isolation. Do not install an unsigned
   candidate or assume an ad-hoc signature is equivalent to the vendor identity.
5. Only after that gate passes, install while the app is fully quit at an
   owner-approved activation window; atomically install the validated runtime
   manifest. Do not replace resources of an in-use app.
6. Verify the actual executor path/hash and sandbox behavior after normal launch
   and macOS restoration. Retain the previous known-good app/package for rollback.

Vendor desktop updates can replace this integration. They must go through the
same staging and compatibility gate; this code does not intercept or certify
the vendor auto-updater. "Always our fork" is not a completed operational claim
until update delivery and a working signed desktop distribution are established.
