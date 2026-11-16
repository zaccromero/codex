#!/usr/bin/env python3
"""Stage a desktop integration candidate. Never install, launch, or re-sign an app."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import struct

from vendor_components import component_snapshot, is_preserved


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read_archive(data):
    header_size = struct.unpack_from('<I', data, 4)[0]
    json_size = struct.unpack_from('<I', data, 12)[0]
    raw = data[16:16 + json_size]
    return json.loads(raw), 8 + header_size, raw


def entries(header, prefix=''):
    for name, node in header['files'].items():
        key = f'{prefix}/{name}' if prefix else name
        if 'files' in node:
            yield from entries(node, key)
        elif 'offset' in node and not node.get('unpacked'):
            yield key, node


def patch_archive(data, prefix):
    header, base, _ = read_archive(data)
    files = dict(entries(header))
    def content(node):
        start = base + int(node['offset'])
        return data[start:start + node['size']]
    package = json.loads(content(files['package.json']))
    main = package['main']
    if main not in files or not main.endswith('.js'):
        raise ValueError('Unrecognized desktop main entry')
    payload = bytearray()
    for name, node in sorted(files.items(), key=lambda pair: int(pair[1]['offset'])):
        original = content(node)
        integrity = node.get('integrity')
        if integrity and (integrity['algorithm'] != 'SHA256' or sha(original) != integrity['hash']):
            raise ValueError(f'Source ASAR integrity mismatch: {name}')
        updated = prefix + original if name == main else original
        node['offset'] = str(len(payload))
        node['size'] = len(updated)
        if integrity:
            block = integrity['blockSize']
            node['integrity'] = {'algorithm': 'SHA256', 'hash': sha(updated),
                                 'blockSize': block,
                                 'blocks': [sha(updated[i:i + block]) for i in range(0, max(1, len(updated)), block)]}
        payload.extend(updated)
    raw = json.dumps(header, separators=(',', ':'), ensure_ascii=False).encode()
    padding = b'\0' * ((-len(raw)) % 4)
    header_pickle = struct.pack('<II', len(raw) + len(padding) + 4, len(raw)) + raw + padding
    archive = struct.pack('<II', 4, len(header_pickle)) + header_pickle + payload
    return bytes(archive), sha(raw), main


def manifest_for(root):
    root = root.resolve(strict=True)
    hashes = {}
    for name in ('codex', 'codex-code-mode-host', 'codex-exec-server'):
        relative = f'bin/{name}'
        target = root / relative
        if target.is_symlink() or not target.is_file() or not os.access(target, os.X_OK):
            raise ValueError(f'Invalid runtime component: {relative}')
        hashes[relative] = sha(target.read_bytes())
    return {'schemaVersion': 1, 'packageRoot': str(root), 'sha256': hashes}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-app', type=Path, required=True)
    parser.add_argument('--runtime-package', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--independent', action='store_true', help='Separate identity/profile with an embedded fork runtime')
    args = parser.parse_args()
    source = args.source_app.resolve(strict=True)
    output = args.output.resolve()
    if output.exists() or source == output or source in output.parents:
        raise ValueError('Output must be a new directory outside the source app')
    info = plistlib.loads((source / 'Contents/Info.plist').read_bytes())
    if info.get('CFBundleIdentifier') != 'com.openai.codex':
        raise ValueError('Unexpected source application identity')
    preserved = component_snapshot(source)
    archive = (source / 'Contents/Resources/app.asar').read_bytes()
    _, _, raw = read_archive(archive)
    expected = info['ElectronAsarIntegrity']['Resources/app.asar']
    if expected['algorithm'] != 'SHA256' or expected['hash'] != sha(raw):
        raise ValueError('Source application ASAR header does not match Info.plist')
    manifest = manifest_for(args.runtime_package)
    manifest['desktopVersion'] = info['CFBundleShortVersionString']
    source_runtime = json.loads(json.dumps(manifest))
    # Selection occurs in the earliest entry, before vendor code creates a backend.
    prefix = (b'/* Drako persistent fork runtime selection v1 */\n'
              b'require(require("node:path").join(process.resourcesPath,"drako-runtime-selector.cjs"))'
              b'.activateSelection(require("node:path").join(require("node:os").homedir(),'
              b'"Library/Application Support/DrakoCodex/runtime.json"),require("electron").app.getVersion());\n')
    if args.independent:
        prefix = (b'/* Drako persistent fork runtime selection v1 */\n'
                  b'require(require("node:path").join(process.resourcesPath,"drako-independent-bootstrap.cjs"));\n')
    patched, header_hash, main_entry = patch_archive(archive, prefix)
    output.mkdir(parents=True)
    candidate = output / ('Drako Codex.app' if args.independent else 'ChatGPT.app')
    shutil.copytree(source, candidate, symlinks=True)
    resources = candidate / 'Contents/Resources'
    (resources / 'app.asar').write_bytes(patched)
    shutil.copy2(Path(__file__).with_name('runtime-selector.cjs'), resources / 'drako-runtime-selector.cjs')
    if args.independent:
        shutil.copy2(Path(__file__).with_name('independent-bootstrap.cjs'), resources / 'drako-independent-bootstrap.cjs')
        shutil.copytree(args.runtime_package, resources / 'drako-runtime', symlinks=True)
        del manifest['packageRoot']
        manifest['packageRootRelative'] = 'drako-runtime'
        info['CFBundleIdentifier'] = 'ai.drako.codex.desktop'
        info['CFBundleName'] = 'Drako Codex'
        info['CFBundleDisplayName'] = 'Drako Codex'
        # Do not register competing vendor URL schemes, document handlers or
        # vendor privileged helpers for a separate local application.
        for key in ('CFBundleURLTypes', 'CFBundleDocumentTypes', 'SMPrivilegedExecutables',
                    'SUFeedURL', 'SUPublicEDKey'):
            info.pop(key, None)
        for profile in candidate.rglob('embedded.provisionprofile'):
            if not is_preserved(candidate, profile):
                profile.unlink()
    info['ElectronAsarIntegrity']['Resources/app.asar']['hash'] = header_hash
    (candidate / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
    manifest_file = output / 'runtime.json'
    manifest_file.write_text(json.dumps(manifest, indent=2) + '\n')
    manifest_file.chmod(0o600)
    if args.independent:
        shutil.copy2(manifest_file, resources / 'drako-runtime.json')
    receipt = {'status': 'staged-not-signed-not-installed', 'sourceApp': str(source),
               'sourceVersion': info['CFBundleShortVersionString'],
               'sourceArchiveSha256': sha(archive), 'candidateArchiveSha256': sha(patched),
               'mainEntry': main_entry, 'candidateApp': str(candidate),
               'manifest': str(manifest_file), 'runtime': manifest,
               'sourceRuntime': source_runtime,
               'independent': args.independent,
               'preservedVendorComponents': preserved,
               'launchPerformed': False, 'liveActivationVerified': False}
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
