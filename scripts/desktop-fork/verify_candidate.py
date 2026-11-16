#!/usr/bin/env python3
"""Verify staged desktop bytes without executing or activating the desktop app."""
import argparse
import hashlib
import json
from pathlib import Path
import plistlib

from build_candidate import read_archive, entries
from vendor_components import verify_preserved


def contents(data):
    header, base, raw = read_archive(data)
    files = {}
    for name, node in entries(header):
        offset = base + int(node['offset'])
        value = data[offset:offset + node['size']]
        integrity = node.get('integrity')
        if integrity:
            assert hashlib.sha256(value).hexdigest() == integrity['hash'], name
            size = integrity['blockSize']
            assert integrity['blocks'] == [hashlib.sha256(value[i:i+size]).hexdigest()
                                           for i in range(0, max(1, len(value)), size)], name
        files[name] = value
    return files, hashlib.sha256(raw).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    receipt = json.loads((args.output / 'receipt.json').read_text())
    source = Path(receipt['sourceApp'])
    candidate = Path(receipt['candidateApp'])
    verify_preserved(source, receipt.get('preservedVendorComponents'))
    verify_preserved(candidate, receipt.get('preservedVendorComponents'))
    original = (source / 'Contents/Resources/app.asar').read_bytes()
    assert hashlib.sha256(original).hexdigest() == receipt['sourceArchiveSha256'], 'Source app changed'
    before, _ = contents(original)
    patched = (candidate / 'Contents/Resources/app.asar').read_bytes()
    assert hashlib.sha256(patched).hexdigest() == receipt['candidateArchiveSha256'], 'Candidate changed'
    after, digest = contents(patched)
    assert before.keys() == after.keys()
    main_entry = receipt['mainEntry']
    for name in before:
        if name != main_entry:
            assert before[name] == after[name], name
    assert after[main_entry].endswith(before[main_entry])
    prefix = after[main_entry][:-len(before[main_entry])]
    assert prefix.startswith(b'/* Drako persistent fork runtime selection v1 */')
    assert (b'drako-independent-bootstrap.cjs' if receipt.get('independent') else b'.activateSelection(') in prefix
    info = plistlib.loads((candidate / 'Contents/Info.plist').read_bytes())
    assert info['ElectronAsarIntegrity']['Resources/app.asar']['hash'] == digest
    assert (candidate / 'Contents/Resources/drako-runtime-selector.cjs').read_bytes() == Path(__file__).with_name('runtime-selector.cjs').read_bytes()
    if receipt.get('independent'):
        assert info['CFBundleIdentifier'] == 'ai.drako.codex.desktop'
        assert 'CFBundleURLTypes' not in info
        assert (candidate / 'Contents/Resources/drako-independent-bootstrap.cjs').read_bytes() == Path(__file__).with_name('independent-bootstrap.cjs').read_bytes()
    # Export only our prefix for a separate Node smoke test, never vendor bootstrap.
    (args.output / 'verified-bootstrap-prefix.cjs').write_bytes(prefix)
    print(json.dumps({'archiveFilesVerified': len(after), 'onlyBootstrapChanged': True,
                      'headerAndBlockHashesValid': True, 'sourceArchiveUnchanged': True,
                      'desktopLaunched': False}))


if __name__ == '__main__':
    main()
