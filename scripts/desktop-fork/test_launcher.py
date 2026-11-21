import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from build_candidate import LAUNCHER_NAME, build_launcher

PROFILE = 'Library/Application Support/DrakoCodex/desktop-profile'


class LauncherTests(unittest.TestCase):
    def run_launcher(self, arguments, home_env=True):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw).resolve()
            macos = root / 'Fake.app/Contents/MacOS'
            macos.mkdir(parents=True)
            build_launcher(macos / LAUNCHER_NAME)
            # Stand-in for the real shell: print one argument per line.
            (macos / 'ChatGPT').write_text('#!/bin/sh\nfor a in "$0" "$@"; do printf "%s\\n" "$a"; done\n')
            (macos / 'ChatGPT').chmod(0o755)
            home = root / 'home'
            home.mkdir()
            env = {'PATH': '/usr/bin:/bin'}
            if home_env:
                env['HOME'] = str(home)
            result = subprocess.run([str(macos / LAUNCHER_NAME), *arguments], env=env,
                                    capture_output=True, text=True, timeout=30)
            mode = (home / PROFILE).stat().st_mode & 0o777 if (home / PROFILE).is_dir() else None
            return result, macos, home, mode

    def test_starts_the_real_shell_on_the_independent_profile(self):
        result, macos, home, mode = self.run_launcher([])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(),
                         [str(macos / 'ChatGPT'), f'--user-data-dir={home / PROFILE}'])
        self.assertEqual(mode, 0o700)

    def test_passes_other_arguments_and_drops_a_foreign_profile(self):
        result, macos, home, _ = self.run_launcher(
            ['--user-data-dir=/tmp/other', '--user-data-dir', 'file with space', '--flag=1'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines()[1:],
                         [f'--user-data-dir={home / PROFILE}', 'file with space', '--flag=1'])

    def test_falls_back_to_the_account_home_without_HOME(self):
        result, _, _, _ = self.run_launcher([], home_env=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines()[1],
                         f'--user-data-dir={Path(os.path.expanduser("~" + os.environ.get("USER", "")))/ PROFILE}')


if __name__ == '__main__':
    unittest.main()
