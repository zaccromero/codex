import hashlib
import json
import struct
import unittest

from build_candidate import patch_archive, read_archive, entries


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


if __name__ == '__main__':
    unittest.main()
