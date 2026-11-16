#!/usr/bin/env python3
"""Ad-hoc sign only a staged independent candidate. Never launch/install it."""
import argparse
import json
from pathlib import Path
import plistlib
import subprocess

MACHO = {b'\xcf\xfa\xed\xfe', b'\xce\xfa\xed\xfe', b'\xfe\xed\xfa\xcf',
         b'\xfe\xed\xfa\xce', b'\xca\xfe\xba\xbe', b'\xbe\xba\xfe\xca',
         b'\xca\xfe\xba\xbf'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    output = args.output.resolve(strict=True)
    receipt = json.loads((output / 'receipt.json').read_text())
    app = Path(receipt['candidateApp']).resolve(strict=True)
    if not receipt.get('independent') or app.parent != output or app.name != 'Drako Codex.app':
        raise ValueError('Only a staged independent candidate may be signed')
    info = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
    if info['CFBundleIdentifier'] != 'ai.drako.codex.desktop':
        raise ValueError('Unexpected bundle identity')
    if list(app.rglob('embedded.provisionprofile')):
        raise ValueError('Vendor provisioning profiles must not be claimed')
    entitlements = output / 'independent-entitlements.plist'
    entitlements.write_bytes(plistlib.dumps({
        'com.apple.security.cs.allow-jit': True,
        'com.apple.security.cs.allow-unsigned-executable-memory': True,
        'com.apple.security.device.audio-input': True,
        'com.apple.security.device.camera': True,
        'com.apple.security.automation.apple-events': True,
    }))
    files = []
    for file in app.rglob('*'):
        if file.is_symlink() or not file.is_file():
            continue
        with file.open('rb') as stream:
            if stream.read(4) in MACHO:
                files.append(file)
    def sign(target, executable=False):
        command = ['/usr/bin/codesign', '--force', '--sign', '-', '--timestamp=none', '--options', 'runtime']
        if executable:
            command += ['--entitlements', str(entitlements)]
        subprocess.run(command + [str(target)], check=True, capture_output=True)
    for file in sorted(files, key=lambda p: len(p.parts), reverse=True):
        sign(file, executable=True)
    bundles = [p for p in app.rglob('*') if p.is_dir() and not p.is_symlink()
               and p.suffix in ('.app', '.framework', '.xpc', '.appex', '.bundle')]
    for bundle in sorted(bundles, key=lambda p: len(p.parts), reverse=True):
        # Resource-only bundles have no signable executable.
        if any(bundle in f.parents for f in files):
            sign(bundle, executable=True)
    sign(app, executable=True)
    subprocess.run(['/usr/bin/codesign', '--verify', '--deep', '--strict', str(app)], check=True)
    # Signing changes embedded runtime executable bytes. Re-pin final signed bytes.
    import hashlib
    manifest_file = app / 'Contents/Resources/drako-runtime.json'
    manifest = json.loads(manifest_file.read_text())
    for key in manifest['sha256']:
        manifest['sha256'][key] = hashlib.sha256((manifest_file.parent / 'drako-runtime' / key).read_bytes()).hexdigest()
    manifest_file.write_text(json.dumps(manifest, indent=2) + '\n')
    manifest_file.chmod(0o600)
    (output / 'runtime.json').write_bytes(manifest_file.read_bytes())
    sign(app, executable=True)
    subprocess.run(['/usr/bin/codesign', '--verify', '--deep', '--strict', str(app)], check=True)
    receipt.update(status='adhoc-signed-independent-not-installed', runtime=manifest,
                   signatureVerified=True, notarized=False, launchPerformed=False,
                   liveActivationVerified=False)
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps({'status': receipt['status'], 'signedMachOFiles': len(files),
                      'signatureVerified': True, 'notarized': False}))


if __name__ == '__main__':
    main()
