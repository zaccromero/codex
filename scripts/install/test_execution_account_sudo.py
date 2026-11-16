import importlib.util
import hashlib
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    'installer', Path(__file__).with_name('execution_account_sudo.py'))
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class AccountInstallerTests(unittest.TestCase):
    def test_admin_failure_preserves_registration(self):
        with tempfile.TemporaryDirectory() as directory:
            environments = Path(directory)/'environments.toml'
            original = ('[[environments]]\nid="milo-ci"\nprogram="' + str(installer.PREVIOUS_EXECUTOR) + '"\n').encode()
            environments.write_bytes(original)
            with patch.object(installer, 'ENVIRONMENTS', environments), \
                    patch.object(installer.os, 'geteuid', return_value=501), \
                    patch.object(installer, 'preflight', return_value=b'coding unchanged'), \
                    patch.object(installer, 'installed_receipt_matches', return_value=False), \
                    patch.object(installer.sys.stdin, 'isatty', return_value=False), \
                    patch.object(installer.subprocess, 'run', return_value=SimpleNamespace(returncode=1)) as run:
                with self.assertRaisesRegex(RuntimeError, 'administrator package installation did not complete'):
                    installer.activate()
                self.assertEqual(environments.read_bytes(), original)
                self.assertEqual(run.call_args.args[0][:3], ['/usr/bin/sudo', '-n', '--'])
                self.assertEqual(run.call_args.args[0][-1], 'install')

    def test_matching_install_skips_admin_but_requires_live_bootstrap(self):
        payload = b'package'
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory)/'config.toml'
            config.write_bytes(b'coding unchanged')
            environments = Path(directory)/'environments.toml'
            original = ('[[environments]]\nid="milo-ci"\nprogram="' + str(installer.PREVIOUS_EXECUTOR) + '"\n').encode()
            environments.write_bytes(original)
            with patch.object(installer, 'CONFIG', config), \
                    patch.object(installer, 'ENVIRONMENTS', environments), \
                    patch.object(installer.os, 'geteuid', return_value=501), \
                    patch.object(installer, 'preflight', return_value=config.read_bytes()), \
                    patch.object(installer, 'EXECUTOR_SHA256', hashlib.sha256(payload).hexdigest()), \
                    patch.object(installer, 'installed_receipt_matches', return_value=True), \
                    patch.object(installer, 'trusted_file', return_value=payload), \
                    patch.object(installer, 'write_registration') as write, \
                    patch.object(installer.subprocess, 'run', return_value=SimpleNamespace(returncode=1, stderr='bootstrap refused')) as run:
                with self.assertRaisesRegex(RuntimeError, 'account bootstrap failed'):
                    installer.activate()
                write.assert_not_called()
                self.assertEqual(run.call_args.args[0][0], str(installer.EXECUTOR))
                self.assertEqual(run.call_args.kwargs['input'], '')
                run.return_value = SimpleNamespace(returncode=0, stderr='')
                with patch('builtins.print'):
                    installer.activate()
                write.assert_called_once_with(original, installer.registration_bytes(original, installer.EXECUTOR))
                self.assertEqual(config.read_bytes(), b'coding unchanged')

    def test_registration_changes_only_selected_program(self):
        original = ('default = "local"\ninclude_local = true\n# retained comment\n\n'
                    '[[environments]]\nid = "milo-ci"\nprogram = "' + str(installer.PREVIOUS_EXECUTOR) + '"\n'
                    'args = ["--listen", "stdio"]\n'
                    '\n[[environments]]\nid = "other"\nprogram = "/bin/true"\n').encode()
        expected = original.replace(str(installer.PREVIOUS_EXECUTOR).encode(), str(installer.EXECUTOR).encode())
        updated = installer.registration_bytes(original, installer.EXECUTOR)
        self.assertEqual(updated, expected)
        self.assertEqual(installer.registration_bytes(updated, installer.EXECUTOR), updated)
        self.assertEqual(installer.registration_bytes(updated, installer.PREVIOUS_EXECUTOR), original)

    def test_registration_rejects_missing_duplicate_and_foreign_entries(self):
        for original in [b'default="local"\n',
                         b'[[environments]]\nid="milo-ci"\nprogram="/foreign"\n',
                         ('[[environments]]\nid="milo-ci"\nprogram="' + str(installer.PREVIOUS_EXECUTOR) + '"\n') .encode() * 2]:
            with self.subTest(original=original), self.assertRaises(RuntimeError):
                installer.registration_bytes(original, installer.EXECUTOR)

    def test_package_install_is_idempotent_and_refuses_foreign_content(self):
        payload = b'synthetic executable'
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory)/'version/bin/codex-exec-server'
            with patch.object(installer, 'EXECUTOR', binary), \
                    patch.object(installer, 'EXECUTOR_SHA256', hashlib.sha256(payload).hexdigest()), \
                    patch.object(installer, 'candidate_bytes', return_value=payload), \
                    patch.object(installer, 'trusted_directory'), \
                    patch.object(installer, 'trusted_file', side_effect=lambda p: p.read_bytes()):
                self.assertTrue(installer.install_package())
                self.assertEqual(binary.read_bytes(), payload)
                self.assertEqual(binary.stat().st_mode & 0o777, 0o755)
                self.assertFalse(installer.install_package())
                binary.write_bytes(b'foreign')
                with self.assertRaisesRegex(RuntimeError, 'existing version differs'):
                    installer.install_package()
                self.assertEqual(binary.read_bytes(), b'foreign')

    def test_candidate_hash_and_symlink_checks(self):
        payload = b'synthetic executable'
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory)/'candidate'
            binary.write_bytes(payload)
            alias = Path(directory)/'alias'
            alias.symlink_to(binary)
            with patch.object(installer, 'CANDIDATE', binary), \
                    patch.object(installer, 'EXECUTOR_SHA256', hashlib.sha256(payload).hexdigest()):
                self.assertEqual(installer.candidate_bytes(), payload)
                binary.write_bytes(b'tampered')
                with self.assertRaisesRegex(RuntimeError, 'hash differs'):
                    installer.candidate_bytes()
            with patch.object(installer, 'CANDIDATE', alias), self.assertRaises(OSError):
                installer.candidate_bytes()

    def test_native_sudoers_syntax(self):
        if not Path('/usr/sbin/visudo').exists():
            self.skipTest('native sudoers parser unavailable')
        with tempfile.NamedTemporaryFile() as file:
            file.write(installer.rule_bytes())
            file.flush()
            installer.visudo(Path(file.name))

    def test_exact_bootstrap_without_wildcards(self):
        rule = installer.rule_bytes().decode().splitlines()[-1]
        self.assertTrue(rule.startswith('zacc ALL=(milo-ci:staff) NOPASSWD: NOSETENV: '))
        self.assertNotIn('*', rule)
        self.assertNotIn('?', rule)
        self.assertIn('/usr/bin/env -i HOME\\=/Users/milo-ci USER\\=milo-ci', rule)
        self.assertTrue(rule.endswith('--execution-account-policy /Library/CodexAccounts/milo-ci.json'))

    def test_injection_rejected(self):
        for value in ['x\nALL', 'x*', 'x y', 'x;id', '$(id)', 'x\\y']:
            with self.subTest(value=value), self.assertRaises(RuntimeError):
                installer.sudoers_escape(value)

    def transaction(self, mode):
        with tempfile.TemporaryDirectory() as directory, patch.object(installer, 'trusted_directory'):
            output = Path(directory) / 'rule'

            def validate(path):
                if mode == 'parser' and path is not None:
                    raise RuntimeError('invalid')
                if mode == 'global' and path is None:
                    raise RuntimeError('invalid')
                if mode == 'race' and path is not None:
                    output.write_bytes(b'other')

            if mode == 'ok':
                self.assertTrue(installer.install_rule(output, installer.rule_bytes(), validate))
                self.assertEqual(output.read_bytes(), installer.rule_bytes())
                self.assertEqual(output.stat().st_mode & 0o777, 0o440)
            else:
                with self.assertRaises((RuntimeError, FileExistsError)):
                    installer.install_rule(output, installer.rule_bytes(), validate)
                if mode == 'race':
                    self.assertEqual(output.read_bytes(), b'other')
                else:
                    self.assertFalse(output.exists())
            self.assertEqual(sorted(p.name for p in Path(directory).iterdir()),
                             ['rule'] if output.exists() else [])

    def test_install_transaction(self):
        self.transaction('ok')

    def test_parser_failure(self):
        self.transaction('parser')

    def test_global_failure_rolls_back(self):
        self.transaction('global')

    def test_concurrent_file_preserved(self):
        self.transaction('race')

    def test_every_secret_denial_required(self):
        import json
        config = ['default_permissions = "coding"', 'approval_policy = "on-request"',
                  'approvals_reviewer = "auto_review"', '[permissions.coding.filesystem]']
        config += [json.dumps(p) + ' = "deny"' for p in installer.PRIVATE_DENIALS]
        config += ['[permissions.coding.filesystem."/"]']
        config += [json.dumps(p) + ' = "deny"' for p in installer.GLOB_DENIALS]
        original = ('\n'.join(config) + '\n').encode()
        installer.validate_coding(original)
        for path in (*installer.PRIVATE_DENIALS, *installer.GLOB_DENIALS):
            changed = original.replace((json.dumps(path) + ' = "deny"').encode(), b'')
            with self.subTest(path=path), self.assertRaises(RuntimeError):
                installer.validate_coding(changed)
        with self.assertRaisesRegex(RuntimeError, 'template'):
            installer.validate_coding(original + b'"**/.env.*" = "deny"\n')


if __name__ == '__main__':
    unittest.main()
