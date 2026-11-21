#!/usr/bin/env python3
"""Build a version-pinned diagnostic executor without altering its policy."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
from pathlib import Path

BASE_COMMIT = "d2c035639366e0bf2c316aa2065c24aabc12bcdc"
BASE_VERSION = "0.167.0-alpha.4.1"
SOURCE_PATH = "codex-rs/exec-server/src/process_sandbox.rs"
SOURCE_BLOB = "488b7c97f249ffd71a69def81a88f902246858f7"
TEST_PATH = "codex-rs/core/src/client_runtime_auth_tests.rs"
TEST_BLOB = "35a41b68cd308976d051d74d56faf25614bdec02"
BEFORE = "    fields(process.id = trace_process_id(params.process_id.as_str())),"
AFTER = """    fields(
        process.id = trace_process_id(params.process_id.as_str()),
        network.managed_required = params.enforce_managed_network,
        network.proxy_launch_supplied = params.network_proxy.is_some(),
        network.prepared_context_supplied = params.managed_network.is_some(),
        network.sandbox_supplied = params.sandbox.is_some(),
        network.enabled = params.sandbox.as_ref().is_some_and(|sandbox| {
            sandbox.permissions.network_sandbox_policy().is_enabled()
        }),
        network.denied_reads = params.sandbox.as_ref().is_some_and(|sandbox| {
            sandbox
                .permissions
                .file_system_sandbox_policy()
                .has_denied_read_restrictions()
        }),
    ),"""


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def git_blob(data: bytes) -> str:
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()


def instrument_source(data: bytes) -> bytes:
    if git_blob(data) != SOURCE_BLOB:
        raise ValueError("diagnostic source does not match the pinned release blob")
    text = data.decode("utf-8")
    if text.count(BEFORE) != 1:
        raise ValueError("diagnostic insertion point is ambiguous")
    changed = text.replace(BEFORE, AFTER, 1)
    if changed.replace(AFTER, BEFORE, 1) != text:
        raise ValueError("diagnostic change exceeds span metadata")
    return changed.encode("utf-8")


def passed_tests(output: str) -> int:
    counts = re.findall(r"test result: ok\.\s+(\d+) passed;", output)
    count = sum(map(int, counts))
    if not count:
        raise ValueError("test filter executed no passing tests")
    return count


def run(source: Path, command: list[str]) -> str:
    return subprocess.run(
        command, cwd=source, check=True, text=True, capture_output=True
    ).stdout.strip()


def apply(source: Path, output: Path) -> None:
    if run(source, ["git", "rev-parse", "HEAD"]) != BASE_COMMIT:
        raise ValueError("checkout is not the compatible release commit")
    output.mkdir(parents=True, exist_ok=True)
    target = source / SOURCE_PATH
    original = target.read_bytes()
    changed = instrument_source(original)
    target.write_bytes(changed)
    diff = run(source, ["git", "diff", "--", SOURCE_PATH]) + "\n"
    (output / "runtime.patch").write_text(diff)
    (output / "source.json").write_text(
        json.dumps(
            {
                "base_commit": BASE_COMMIT,
                "base_version": BASE_VERSION,
                "original_source_blob": SOURCE_BLOB,
                "instrumented_source_blob": git_blob(changed),
                "patch_sha256": sha256(diff.encode()),
                "harness_commit": os.environ.get("GITHUB_SHA"),
                "run_id": os.environ.get("GITHUB_RUN_ID"),
                "policy_behavior_changed": False,
                "diagnostic_fields": [
                    "managed_required", "proxy_launch_supplied",
                    "prepared_context_supplied", "sandbox_supplied",
                    "enabled", "denied_reads",
                ],
            },
            indent=2,
        ) + "\n"
    )
    normalize_release_lock(source, output)
    repair_pinned_test_fixture(source, output)
    print("Verified release source; applied span-metadata diagnostics only.")


def normalize_release_lock(source: Path, output: Path) -> None:
    lock_path = source / "codex-rs/Cargo.lock"
    original = lock_path.read_bytes()
    pinned = subprocess.run(
        ["git", "show", BASE_COMMIT + ":codex-rs/Cargo.lock"],
        cwd=source, check=True, capture_output=True,
    ).stdout
    if original != pinned:
        raise ValueError("release lockfile no longer matches the pinned source")
    local_names = set()
    for manifest in (source / "codex-rs").rglob("Cargo.toml"):
        text = manifest.read_text()
        package = re.search(r"(?ms)^\[package\]\s*\n(.*?)(?=^\[|\Z)", text)
        if package and re.search(r"(?m)^version\.workspace\s*=\s*true\s*$", package[1]):
            name = re.search(r'(?m)^name\s*=\s*"([^"\n]+)"\s*$', package[1])
            if name:
                local_names.add(name[1])
    blocks = original.decode().split("[[package]]")
    changed_names = []
    for index, block in enumerate(blocks[1:], 1):
        if re.search(r"(?m)^source\s*=", block):
            continue
        name = re.search(r'(?m)^name = "([^"\n]+)"$', block)
        version = re.search(r'(?m)^version = "([^"\n]+)"$', block)
        if not name or not version or name[1] not in local_names or version[1] != "0.0.0":
            raise ValueError("unexpected local lockfile package")
        blocks[index] = block.replace('version = "0.0.0"', 'version = "' + BASE_VERSION + '"', 1)
        changed_names.append(name[1])
    if not changed_names:
        raise ValueError("release lockfile normalization matched no local packages")
    normalized = "[[package]]".join(blocks).encode()
    lock_path.write_bytes(normalized)
    metadata_path = output / "source.json"
    metadata = json.loads(metadata_path.read_text())
    metadata["lockfile_normalization"] = {
        "original_sha256": sha256(original),
        "normalized_sha256": sha256(normalized),
        "local_packages": sorted(changed_names),
        "external_dependencies_unchanged": True,
        "change": "local workspace version labels only: 0.0.0 to " + BASE_VERSION,
    }
    metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")
    diff = run(source, ["git", "diff", "--", SOURCE_PATH, "codex-rs/Cargo.lock"]) + "\n"
    (output / "runtime.patch").write_text(diff)
    metadata["patch_sha256"] = sha256(diff.encode())
    metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")
    print("Normalized " + str(len(changed_names)) + " local version labels; external dependencies unchanged.")


def repair_pinned_test_fixture(source: Path, output: Path) -> None:
    target = source / TEST_PATH
    original = target.read_bytes()
    if git_blob(original) != TEST_BLOB:
        raise ValueError("pinned test fixture changed before repair")
    before = "            /*parent_routing_affinity*/ None,\n        )"
    after = "            /*parent_routing_affinity*/ None,\n            /*reauth*/ false,\n        )"
    text = original.decode()
    if text.count(before) != 1:
        raise ValueError("test fixture repair is ambiguous")
    changed = text.replace(before, after, 1).encode()
    target.write_bytes(changed)
    metadata_path = output / "source.json"
    metadata = json.loads(metadata_path.read_text())
    metadata["test_fixture_repair"] = {
        "path": TEST_PATH, "original_blob": TEST_BLOB,
        "repaired_blob": git_blob(changed), "test_only": True,
        "change": "supply the required reauth=false argument for the initial-connection revocation test",
    }
    diff = run(source, ["git", "diff", "--", SOURCE_PATH, TEST_PATH, "codex-rs/Cargo.lock"]) + "\n"
    (output / "runtime.patch").write_text(diff)
    metadata["patch_sha256"] = sha256(diff.encode())
    metadata_path.write_text(json.dumps(metadata, indent=2) + "\n")
    print("Repaired pinned test-only call signature; production auth code unchanged.")


def checks(source: Path, output: Path) -> None:
    output.mkdir(parents=True, exist_ok=True)
    cases = [
        (
            "denied-read-policy",
            ["cargo", "test", "--locked", "-p", "codex-core", "--lib",
             "deny_read_preserves_the_sandbox_for_explicit_escalation_and_blocks_policy_bypass"],
        ),
        (
            "macos-sandbox",
            ["cargo", "test", "--locked", "-p", "codex-sandboxing", "--lib"],
        ),
        (
            "executor-sandbox",
            ["cargo", "test", "--locked", "-p", "codex-exec-server", "--lib",
             "process_sandbox"],
        ),
    ]
    cases.append((
        "runtime-auth-revocation",
        ["cargo", "test", "--locked", "-p", "codex-core", "--lib",
         "selected_runtime_transport_stays_revoked"],
    ))
    results = []
    for label, command in cases:
        log = output / (label + ".log")
        print("Running " + label, flush=True)
        with log.open("w") as destination:
            process = subprocess.Popen(
                command, cwd=source / "codex-rs", text=True,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            )
            assert process.stdout is not None
            for line in process.stdout:
                destination.write(line)
                sys.stdout.write(line)
            returncode = process.wait()
        if returncode != 0:
            raise RuntimeError(label + " failed; no candidate may be activated")
        count = passed_tests(log.read_text())
        results.append({"suite": label, "passed": count, "returncode": returncode})
    (output / "checks.json").write_text(json.dumps(results, indent=2) + "\n")


def package(source: Path, output: Path, target: Path) -> None:
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise ValueError("candidate requires native macOS arm64")
    checks_result = json.loads((output / "checks.json").read_text())
    if len(checks_result) != 4 or any(case["passed"] < 1 for case in checks_result):
        raise ValueError("required policy checks did not pass")
    if run(source, ["git", "diff", "--name-only"]).splitlines() != ["codex-rs/Cargo.lock", TEST_PATH, SOURCE_PATH]:
        raise ValueError("candidate changes exceed diagnostics, local version labels and the pinned test-only call repair")
    metadata = json.loads((output / "source.json").read_text())
    if sha256((source / "codex-rs/Cargo.lock").read_bytes()) != metadata["lockfile_normalization"]["normalized_sha256"]:
        raise ValueError("lockfile changed after guarded release normalization")
    if git_blob((source / TEST_PATH).read_bytes()) != metadata["test_fixture_repair"]["repaired_blob"]:
        raise ValueError("test fixture changed after its bounded repair")
    actual = git_blob((source / SOURCE_PATH).read_bytes())
    if actual != metadata["instrumented_source_blob"]:
        raise ValueError("candidate source changed after diagnostics were applied")
    files = {}
    for name in ["codex", "codex-exec-server", "codex-code-mode-host"]:
        binary = target / "debug" / name
        kind = run(source, ["file", "-b", str(binary)])
        if "arm64" not in kind:
            raise ValueError("candidate binary has the wrong architecture: " + name)
        shutil.copy2(binary, output / name)
        files[name] = sha256(binary.read_bytes())
    version = run(source, [str(output / "codex"), "--version"])
    if BASE_VERSION not in version:
        raise ValueError("candidate version does not match the compatible release")
    for name in ["LICENSE", "NOTICE"]:
        shutil.copy2(source / name, output / name)
    metadata.update({
        "version_output": version,
        "harness_sha256": sha256(Path(__file__).read_bytes()),
        "rustc_version": run(source, ["rustc", "--version"]),
        "cargo_version": run(source, ["cargo", "--version"]),
        "binary_sha256": files,
        "checks": checks_result,
        "diagnostics_only": True,
        "installed": False,
        "notarized": False,
    })
    (output / "manifest.json").write_text(json.dumps(metadata, indent=2) + "\n")
    (output / "SHA256SUMS").write_text(
        "".join(digest + "  " + name + "\n" for name, digest in sorted(files.items()))
    )
    (output / "README.txt").write_text(
        "Issue 19 diagnostic candidate. Not a confirmed networking fix.\n"
        "Compiled from the exact compatible release plus span-metadata diagnostics.\n"
        "No policy behavior was changed. The added fields contain no paths, hosts, environment values or command content.\n"
        "This artifact is not installed, activated or Apple-notarized.\n"
        "Preserve the bundled runtime and verify manifest hashes before any local evaluation.\n"
    )
    archive = output / "runtime.tar.gz"
    archive_names = [
        "codex", "codex-exec-server", "codex-code-mode-host", "LICENSE", "NOTICE",
        "manifest.json", "SHA256SUMS", "runtime.patch", "source.json", "checks.json",
        "README.txt",
    ]
    with tarfile.open(archive, "w:gz") as bundle:
        for name in archive_names:
            bundle.add(output / name, arcname=name, recursive=False)
    (output / "runtime.tar.gz.sha256").write_text(
        sha256(archive.read_bytes()) + "  runtime.tar.gz\n"
    )
    print("Packaged a tested diagnostic candidate; activation was not performed.")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("phase", choices=["apply", "lock", "test-fixture", "checks", "package"])
    parser.add_argument("--source", type=Path, default=Path.cwd())
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target", type=Path)
    args = parser.parse_args()
    if args.phase == "apply":
        apply(args.source, args.output)
    elif args.phase == "test-fixture":
        repair_pinned_test_fixture(args.source, args.output)
    elif args.phase == "lock":
        normalize_release_lock(args.source, args.output)
    elif args.phase == "checks":
        checks(args.source, args.output)
    else:
        if args.target is None:
            parser.error("--target is required for package")
        package(args.source, args.output, args.target)


if __name__ == "__main__":
    main()
