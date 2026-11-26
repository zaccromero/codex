#!/usr/bin/env python3

"""Fail a terminal CI job unless every serialized dependency succeeded.

Parent workflows pass GitHub's `toJSON(needs)` object through the NEEDS
environment variable. Treat skipped and cancelled dependencies as failures too:
for a required fan-in job, only an explicit success is safe to accept.

One exception keeps forks usable: jobs named in CI_UPSTREAM_ONLY run only in the
upstream repository, because they need runners, runner groups or actions that a
fork does not have. Outside UPSTREAM_REPOSITORY those jobs may be `skipped`.
They still may not fail or be cancelled, and upstream behavior is unchanged.
"""

import json
import os

UPSTREAM_REPOSITORY = "openai/codex"


def unsuccessful(needs, repository, upstream_only):
    """Return sorted (name, result) pairs that should fail the fan-in job."""
    exempt = set() if repository == UPSTREAM_REPOSITORY else set(upstream_only)
    return sorted(
        (name, dependency["result"])
        for name, dependency in needs.items()
        if dependency["result"] != "success"
        and not (dependency["result"] == "skipped" and name in exempt)
    )


def main() -> None:
    # Keep result policy in one script so blocking-ci and postmerge-ci cannot
    # drift in how they interpret dependency conclusions.
    needs = json.loads(os.environ["NEEDS"])
    repository = os.environ.get("GITHUB_REPOSITORY", "")
    upstream_only = [name for name in os.environ.get("CI_UPSTREAM_ONLY", "").split(",") if name]
    failures = unsuccessful(needs, repository, upstream_only)

    if failures:
        print("CI dependencies did not succeed:")
        for name, result in failures:
            print(f"{name}: {result}")
        raise SystemExit(1)

    skipped = sorted(name for name, dependency in needs.items() if dependency["result"] == "skipped")
    if skipped:
        print(f"Upstream-only jobs skipped outside {UPSTREAM_REPOSITORY}: {', '.join(skipped)}")
    print("All CI dependencies succeeded.")


if __name__ == "__main__":
    main()
