import json
from pathlib import Path
import plistlib
import sys
import tempfile
import unittest
from unittest.mock import patch

from vendor_components import component_snapshot, is_preserved, verify_preserved
import sign_independent


class VendorComponentTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.output = Path(self.temp.name).resolve()
        self.app = self.output / 'Drako Codex.app'
        for name in ('cua_node/bin/node', 'codex-cli/CodexCLI.app/Contents/MacOS/codex'):
            target = self.app / 'Contents/Resources' / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b'\xcf\xfa\xed\xfevendor-signed-fixture')
        self.profile = self.app / 'Contents/Resources/codex-cli/CodexCLI.app/Contents/embedded.provisionprofile'
        self.profile.write_bytes(b'vendor profile fixture')
        self.snapshot = component_snapshot(self.app)

    def test_signing_keeps_vendor_executables_and_profiles(self):
        info = self.app / 'Contents/Info.plist'
        info.write_bytes(plistlib.dumps({'CFBundleIdentifier': 'ai.drako.codex.desktop'}))
        executable = self.app / 'Contents/MacOS/Drako'
        executable.parent.mkdir(parents=True)
        executable.write_bytes(b'\xcf\xfa\xed\xfefork fixture')
        manifest = self.app / 'Contents/Resources/drako-runtime.json'
        manifest.write_text(json.dumps({'sha256': {}}))
        (self.output / 'receipt.json').write_text(json.dumps({
            'candidateApp': str(self.app), 'independent': True,
            'preservedVendorComponents': self.snapshot}))
        with patch.object(sys, 'argv', ['sign_independent.py', str(self.output)]), \
             patch.object(sign_independent.subprocess, 'run') as run:
            sign_independent.main()
        signed = [Path(call.args[0][-1]) for call in run.call_args_list if '--force' in call.args[0]]
        self.assertIn(executable, signed)
        self.assertIn(self.app, signed)
        self.assertTrue(all(not is_preserved(self.app, target) for target in signed))
        verify_preserved(self.app, self.snapshot)

    def test_modified_or_missing_vendor_bytes_rejected(self):
        self.profile.write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'changed'):
            verify_preserved(self.app, self.snapshot)

    def test_symlink_target_change_rejected(self):
        link = self.app / 'Contents/Resources/cua_node/current'
        link.symlink_to('bin/node')
        expected = component_snapshot(self.app)
        link.unlink()
        link.symlink_to('other')
        with self.assertRaisesRegex(ValueError, 'changed'):
            verify_preserved(self.app, expected)

    def test_legacy_receipt_rejected(self):
        with self.assertRaisesRegex(ValueError, 'rebuild'):
            verify_preserved(self.app, None)

    def test_drako_runtime_is_not_preserved_vendor_code(self):
        self.assertFalse(is_preserved(self.app, self.app / 'Contents/Resources/drako-runtime/bin/codex'))
        self.assertFalse(is_preserved(self.app, self.app / 'Contents/Resources/cua_node-other/node'))


if __name__ == '__main__':
    unittest.main()
