import unittest
from collections import Counter

from select_bazel_packages import exclusions_for_shard


class BazelPackageShardTest(unittest.TestCase):
    def test_each_rule_is_kept_once_across_the_matrix(self):
        labels = [
            "//:platform",
            "//core:lib",
            "//core:test",
            "//core/nested:test",
            "//tools:all",
            "//tools:binary",
            "//tools:manual-tool",
            "//third_party/v8:engine",
        ]
        kept = Counter()
        for shard in range(1, 5):
            exclusions = exclusions_for_shard(labels, shard, 4)
            self.assertEqual(exclusions, exclusions_for_shard(labels[::-1], shard, 4))
            removed = set()
            for pattern in exclusions:
                if pattern in labels:
                    removed.add(pattern)
                else:
                    prefix = pattern.removesuffix(":all") + ":"
                    removed.update(
                        label for label in labels if label.startswith(prefix)
                    )
            kept.update(set(labels) - removed)
        self.assertEqual(kept, Counter(labels))

    def test_failed_discovery_cannot_produce_an_empty_success(self):
        for labels in ([], ["@external//lib:test"], ["//missing_colon"]):
            with self.subTest(labels=labels), self.assertRaises(ValueError):
                exclusions_for_shard(labels, 1, 4)
        with self.assertRaisesRegex(ValueError, "empty Bazel package shard"):
            exclusions_for_shard(["//only:rule"], 4, 4)
