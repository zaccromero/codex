"""Split a Bazel wildcard build across runners without dropping non-test rules."""

import argparse
import sys
from collections import defaultdict

from select_windows_bazel_targets import assign_targets


def exclusions_for_shard(labels: list[str], shard: int, count: int) -> list[str]:
    if not 1 <= shard <= count:
        raise ValueError("shard number is outside the matrix")
    packages: dict[str, list[str]] = defaultdict(list)
    for label in labels:
        package, separator, name = label.rpartition(":")
        if not package.startswith("//") or not separator or not name:
            raise ValueError(f"expected a workspace rule label, got {label!r}")
        packages[package].append(label)

    # Wildcards preserve Bazel's normal handling of manual and incompatible
    # rules. Weight packages by the number of rules in the original query.
    weights = {package: len(rules) for package, rules in packages.items()}
    assignments, _ = assign_targets(list(packages), weights, count)
    selected = set(assignments[shard - 1])
    if not selected:
        raise ValueError(f"empty Bazel package shard {shard}/{count}")
    exclusions = []
    for package in sorted(packages.keys() - selected):
        wildcard = f"{package}:all"
        # An explicit target named "all" takes precedence over the wildcard.
        if wildcard in packages[package]:
            exclusions.extend(sorted(packages[package]))
        else:
            exclusions.append(wildcard)
    return exclusions


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shard", type=int, required=True)
    parser.add_argument("--count", type=int, required=True)
    args = parser.parse_args()
    try:
        exclusions = exclusions_for_shard(
            sys.stdin.read().splitlines(), args.shard, args.count
        )
    except ValueError as exc:
        parser.error(str(exc))
    for exclusion in exclusions:
        print(exclusion)
