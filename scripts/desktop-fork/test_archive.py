import hashlib
import json
import struct
import tempfile
import unittest
from pathlib import Path

from build_candidate import (INTEGRITY_SENTINEL, electron_asar_integrity_digest,
                             entries, framework_integrity_slots,
                             patch_archive, patch_framework_integrity,
                             read_archive)


def archive_fixture():
    files = {}
    payload = b''
    for name, data in [('package.json', b'{"main":"main.js"}'),
                       ('main.js', b'originalBootstrap();'), ('unchanged.txt', b'preserve me'), ('empty.txt', b'')]:
        files[name] = {'offset': str(len(payload)), 'size': len(data),
                       'integrity': {'algorithm': 'SHA256', 'hash': hashlib.sha256(data).hexdigest(),
                                     'blockSize': 8, 'blocks': []}}
        payload += data
    raw = json.dumps({'files': files}).encode()
    padding = b'\0' * (-len(raw) % 4)
    header = struct.pack('<II', len(raw) + len(padding) + 4, len(raw)) + raw + padding
    return struct.pack('<II', 4, len(header)) + header + payload


class ArchiveTests(unittest.TestCase):
    def test_prefix_runs_before_original_and_other_files_survive(self):
        patched, digest, main = patch_archive(archive_fixture(), b'selectFork();\n')
        header, base, raw = read_archive(patched)
        self.assertEqual(digest, hashlib.sha256(raw).hexdigest())
        self.assertEqual(main, 'main.js')
        contents = {}
        for name, node in entries(header):
            start = base + int(node['offset'])
            data = patched[start:start + node['size']]
            contents[name] = data
            self.assertEqual(node['integrity']['hash'], hashlib.sha256(data).hexdigest())
            self.assertEqual(node['integrity']['blocks'],
                             [hashlib.sha256(data[i:i+8]).hexdigest() for i in range(0, max(1, len(data)), 8)])
        self.assertEqual(contents['main.js'], b'selectFork();\noriginalBootstrap();')
        self.assertEqual(contents['unchanged.txt'], b'preserve me')

    def test_corrupt_source_rejected(self):
        data = archive_fixture()
        with self.assertRaisesRegex(ValueError, 'integrity mismatch'):
            patch_archive(data[:-1] + b'X', b'prefix;')

    def test_electron_integrity_digest_is_sorted_and_framework_slots_are_patched(self):
        integrity = {
            'Resources/z.asar': {'algorithm': 'SHA256', 'hash': 'z-hash'},
            'Resources/app.asar': {'algorithm': 'SHA256', 'hash': 'app-hash'},
        }
        expected = hashlib.sha256(
            b'Resources/app.asarSHA256app-hashResources/z.asarSHA256z-hash').digest()
        self.assertEqual(electron_asar_integrity_digest(integrity), expected)
        with tempfile.TemporaryDirectory() as temporary:
            framework = Path(temporary) / 'Codex Framework'
            framework.write_bytes(b'prefix' + INTEGRITY_SENTINEL + b'\0' * 34 +
                                  b'middle' + INTEGRITY_SENTINEL + b'\0' * 34)
            self.assertEqual(patch_framework_integrity(framework, expected), 2)
            patched = framework.read_bytes()
            self.assertEqual(len(framework_integrity_slots(patched)), 2)
            for position in framework_integrity_slots(patched):
                self.assertEqual(patched[position:position + 34], b'\x01\x01' + expected)

    def test_missing_framework_integrity_sentinel_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            framework = Path(temporary) / 'Codex Framework'
            framework.write_bytes(b'not an Electron framework')
            with self.assertRaisesRegex(ValueError, 'sentinel not found'):
                patch_framework_integrity(framework, b'\0' * 32)


if __name__ == '__main__':
    unittest.main()
