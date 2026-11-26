import importlib.util
import json
import os
import pathlib
import subprocess
import sys
import unittest

SCRIPT = pathlib.Path(__file__).with_name("check_ci_results.py")
spec = importlib.util.spec_from_file_location("check_ci_results", SCRIPT)
check_ci_results = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check_ci_results)

UPSTREAM_ONLY = ["blob-size-policy", "repo-checks", "rust-ci", "sdk"]


def needs(**results):
    return {name.replace("_", "-"): {"result": result} for name, result in results.items()}


class UnsuccessfulTest(unittest.TestCase):
    def test_upstream_treats_every_skip_as_failure(self):
        deps = needs(cargo_deny="success", rust_ci="skipped")
        self.assertEqual(
            check_ci_results.unsuccessful(deps, "openai/codex", UPSTREAM_ONLY),
            [("rust-ci", "skipped")],
        )

    def test_fork_accepts_skipped_upstream_only_jobs(self):
        deps = needs(cargo_deny="success", codespell="success", rust_ci="skipped", sdk="skipped",
                     repo_checks="skipped", blob_size_policy="skipped")
        self.assertEqual(check_ci_results.unsuccessful(deps, "someone/codex", UPSTREAM_ONLY), [])

    def test_fork_still_fails_upstream_only_failures_and_cancellations(self):
        deps = needs(rust_ci="failure", sdk="cancelled", repo_checks="skipped")
        self.assertEqual(
            check_ci_results.unsuccessful(deps, "someone/codex", UPSTREAM_ONLY),
            [("rust-ci", "failure"), ("sdk", "cancelled")],
        )

    def test_fork_still_fails_skips_of_jobs_not_listed(self):
        deps = needs(cargo_deny="skipped", codespell="success")
        self.assertEqual(
            check_ci_results.unsuccessful(deps, "someone/codex", UPSTREAM_ONLY),
            [("cargo-deny", "skipped")],
        )

    def test_missing_repository_is_not_treated_as_upstream_or_exempt_by_default(self):
        deps = needs(rust_ci="skipped")
        self.assertEqual(check_ci_results.unsuccessful(deps, "", []), [("rust-ci", "skipped")])


class MainTest(unittest.TestCase):
    def run_script(self, deps, repository, upstream_only=""):
        env = {**os.environ, "NEEDS": json.dumps(deps), "GITHUB_REPOSITORY": repository,
               "CI_UPSTREAM_ONLY": upstream_only}
        return subprocess.run([sys.executable, str(SCRIPT)], env=env, capture_output=True, text=True)

    def test_exit_codes_on_fork_and_upstream(self):
        deps = needs(cargo_deny="success", rust_ci="skipped")
        fork = self.run_script(deps, "someone/codex", ",".join(UPSTREAM_ONLY))
        self.assertEqual(fork.returncode, 0, fork.stdout)
        self.assertIn("Upstream-only jobs skipped", fork.stdout)
        upstream = self.run_script(deps, "openai/codex", ",".join(UPSTREAM_ONLY))
        self.assertEqual(upstream.returncode, 1)
        self.assertIn("rust-ci: skipped", upstream.stdout)


if __name__ == "__main__":
    unittest.main()
