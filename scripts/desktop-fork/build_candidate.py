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
import subprocess

INTEGRITY_SENTINEL = b'AGbevlPCksUGKNL8TSn7wGmJEuJsXb2A'
INTEGRITY_SLOT_SIZE = 34
LAUNCHER_NAME = 'DrakoCodex'


def build_launcher(destination):
    """Compile the main executable that starts the shell on its own profile."""
    source = Path(__file__).with_name('launcher.c')
    subprocess.run(['/usr/bin/clang', '-arch', 'arm64', '-mmacosx-version-min=13.0', '-std=c11',
                    '-O2', '-Wall', '-Wextra', '-Werror', str(source), '-o', str(destination)],
                   check=True, capture_output=True,
                   env={'PATH': '/usr/bin:/bin', 'TMPDIR': str(destination.parent)})
    destination.chmod(0o755)
    return sha(destination.read_bytes())


def sha(data):
    return hashlib.sha256(data).hexdigest()


def electron_asar_integrity_digest(integrity):
    """Match Electron Packager's sorted key/algorithm/hash digest."""
    digest = hashlib.sha256()
    for key in sorted(integrity):
        entry = integrity[key]
        algorithm = entry.get('algorithm')
        entry_hash = entry.get('hash')
        if not isinstance(algorithm, str) or not isinstance(entry_hash, str):
            raise ValueError(f'Invalid ElectronAsarIntegrity entry: {key}')
        digest.update(key.encode())
        digest.update(algorithm.encode())
        digest.update(entry_hash.encode())
    return digest.digest()


def codex_framework_binary(app):
    matches = [path for path in (app / 'Contents/Frameworks').rglob('Codex Framework')
               if path.is_file() and not path.is_symlink()]
    if len(matches) != 1:
        raise ValueError(f'Expected one Codex Framework binary, found {len(matches)}')
    return matches[0]


def framework_integrity_slots(data):
    positions = []
    start = 0
    while True:
        position = data.find(INTEGRITY_SENTINEL, start)
        if position < 0:
            break
        slot = position + len(INTEGRITY_SENTINEL)
        if slot + INTEGRITY_SLOT_SIZE > len(data):
            raise ValueError('Truncated Electron integrity slot')
        positions.append(slot)
        start = slot + INTEGRITY_SLOT_SIZE
    return positions


def patch_framework_integrity(path, digest):
    if len(digest) != 32:
        raise ValueError('Electron integrity digest must be 32 bytes')
    data = bytearray(path.read_bytes())
    positions = framework_integrity_slots(data)
    if not positions:
        raise ValueError('Electron integrity sentinel not found in Codex Framework')
    payload = b'\x01\x01' + digest
    for position in positions:
        data[position:position + INTEGRITY_SLOT_SIZE] = payload
    path.write_bytes(data)
    return len(positions)


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
        desktop_version = json.dumps(info['CFBundleShortVersionString']).encode()
        prefix = (b'/* Drako persistent fork runtime selection v1 */\n'
                  b'require(require("node:path").join(process.resourcesPath,"drako-independent-bootstrap.cjs"))'
                  b'.activate(' + desktop_version + b');\n')
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
        # Finder cannot pass switches, and the shell takes its single-instance
        # lock in the vendor profile unless one names ours. See launcher.c.
        real_executable = info['CFBundleExecutable']
        if real_executable != 'ChatGPT' or not (candidate / 'Contents/MacOS' / real_executable).is_file():
            raise ValueError('Unexpected desktop main executable')
        launcher_sha = build_launcher(candidate / 'Contents/MacOS' / LAUNCHER_NAME)
        info['CFBundleExecutable'] = LAUNCHER_NAME
        # Do not register competing vendor URL schemes, document handlers or
        # vendor privileged helpers for a separate local application.
        for key in ('CFBundleURLTypes', 'CFBundleDocumentTypes', 'SMPrivilegedExecutables',
                    'SUFeedURL', 'SUPublicEDKey'):
            info.pop(key, None)
        for profile in candidate.rglob('embedded.provisionprofile'):
            profile.unlink()
        # The copied notarization ticket belongs to the vendor's signature and
        # cannot describe this locally signed bundle.
        (candidate / 'Contents/CodeResources').unlink(missing_ok=True)
    info['ElectronAsarIntegrity']['Resources/app.asar']['hash'] = header_hash
    (candidate / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
    integrity_digest = electron_asar_integrity_digest(info['ElectronAsarIntegrity'])
    integrity_slots = patch_framework_integrity(codex_framework_binary(candidate), integrity_digest)
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
               'electronAsarIntegrityDigest': integrity_digest.hex(),
               'electronAsarIntegritySlots': integrity_slots,
               'independent': args.independent,
               **({'launcher': LAUNCHER_NAME, 'launcherSourceSha256': sha(Path(__file__).with_name('launcher.c').read_bytes()),
                   'launcherUnsignedSha256': launcher_sha} if args.independent else {}),
               'launchPerformed': False, 'liveActivationVerified': False}
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
