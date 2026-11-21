# Drako and network repair

User authorizes activation, kill-switch restoration first, Google/Hermes repair,
and health-checker installation without weakening protection. Keep issue
https://github.com/openai/codex/issues/61765 open until verified live.

## Verify access before changes

The prior session enforced approval=never despite saved config using
approval_policy=on-request and default_permissions=coding. Preserve that coding
profile and private-path denials. A fresh CLI with --no-daemon and
--ask-for-approval on-request provides an approval route only if host policy
allows it. Do not use full-access/bypass flags or reset macOS services blindly.

Request approval for a read-only host LaunchServices diagnostic against the
installed ChatGPT app using lsregister -lint. In the sandbox it returned -10822
(server communication unavailable) for both candidate and installed app.
launchctl showed launchservicesd running and never exited. This does not prove
macOS is broken. Stop that action if effective policy still prohibits approval.

CLI host execution does not itself provide a native UI bridge. Verify actual
native app access before claiming control. Candidate open previously returned
-10827 despite its executable existing. No successful activation occurred.

## Prepared fixes

Owned checkout: this directory. Branch fix/drako-preserve-native-signatures-20261116.
Local commit 225f7240, not pushed (prior session required unavailable approval).
Candidate: .drako/artifacts/native-signatures-candidate/Drako Codex.app
Receipt: .drako/artifacts/native-signatures-candidate/receipt.json

Patch preserves original bundled cua_node and codex-cli bytes and signatures.
Verified: 7 Python tests, 9 Node tests, deep strict codesign, 20,152 archive
entries, isolated bootstrap, and preserved OpenAI identity on node_repl.
It DOES NOT resolve fork CLI native compatibility: custom ad-hoc CLI is still
selected. Do not describe signature preservation as complete bridge recovery.
Never disable peer authorization or impersonate a vendor signing identity.

Health-check replacement and two passing isolated regression tests:
/Users/zacc/Documents/Codex/NetworkSecurityHealth/network-security-health.sh
/Users/zacc/Documents/Codex/NetworkSecurityHealth/test_health_check.py
Not installed. Target:
/Users/zacc/Library/Application Support/NetworkSecurityHealth/network-security-health.sh
Existing launch agent: /Users/zacc/Library/LaunchAgents/com.zacc.network-security-health.plist
Preserve a backup and executable mode. Checker explicitly reports protection as
unverified, tests Google/Apple separately, bounds nc with -G, prevents overlapping
runs, and removes blocking alerts/sounds. Test success is not live network proof.

## Actual security/network state

Kill switch last verified OFF and disabled. Prior Bitdefender logs showed
OpenVPN TCP selected and setKillSwitch: 0. Do not toggle protocols blindly.
Tailscale DNS override was previously disabled with user authorization.
Google error: ERR_CONNECTION_CLOSED. Personal Chrome authorization is restricted
to inspecting/reloading the Google error tab. Hermes endpoint is
100.110.147.118:22. Earlier success is stale; 02:12 health report failed.
TCP success alone is not SSH authentication or application health.
Do not read denied ~/.ssh or ~/.hermes paths.

A 02:47 EST snapshot showed BDLDaemon ~95.6% CPU, BDCoreIssues ~41%, and
WindowServer ~3.1GB RAM; no thermal warning. This does not establish the earlier
lag's cause. Do not disable security processes or terminate unowned processes.

Finish only after actual app control, current kill-switch enabled evidence,
protected Google and VM connectivity, installed checker and inspected truthful
report. Distinguish configured kill-switch state from enforcement testing; do
not disconnect protection just to test. Only then close the issue.
