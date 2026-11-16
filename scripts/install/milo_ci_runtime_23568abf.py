#!/usr/bin/env python3
"""Install the pinned Milo CI executor fix without replacing the prior runtime."""

import argparse
import hashlib
import os
from pathlib import Path
import pwd
import shutil
import stat
import subprocess
import tempfile


SOURCE = Path('/Users/zacc/code/.worktrees/codex/milo-ci-env-normalization-20261116/.drako/target/debug/codex-exec-server')
SOURCE_SHA256 = 'c46db1684a36dd650c7dce71be1d4a07ad5bec786ff4fe810190fa3816619ed1'
DESTINATION = Path('/Library/CodexAccounts/runtime-23568abf/bin/codex-exec-server')
POLICY = Path('/Library/CodexAccounts/milo-ci.json')
RULE = Path('/private/etc/sudoers.d/codex-milo-ci-23568abf')
ENVIRONMENTS = Path('/Users/zacc/.codex/environments.toml')
OLD_EXECUTOR = '/Library/CodexAccounts/runtime-eb342b19/bin/codex-exec-server'
CI_PATH = '/opt/homebrew/opt/node@24/bin:/Users/milo-ci/ci/milo-ci-v1/tools/bin:/Users/milo-ci/ci/milo-ci-v1/tools/node_modules/.bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin'


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def regular_file(path, owner=None, writable=False):
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and not stat.S_ISLNK(info.st_mode),
            f'not a regular file: {path}')
    if owner is not None:
        require(info.st_uid == owner, f'unexpected owner: {path}')
    if not writable:
        require(not info.st_mode & 0o022, f'group/world writable: {path}')


def rule_bytes():
    arguments = [
        '/usr/bin/env', '-i', 'HOME=/Users/milo-ci', 'USER=milo-ci',
        'LOGNAME=milo-ci', 'PATH=' + CI_PATH, str(DESTINATION),
        '--listen', 'stdio', '--execution-account-policy', str(POLICY),
    ]
    require(all(all(c.isalnum() or c in '_/.=:@#-' for c in value)
                for value in arguments), 'unsafe sudoers argument')
    command = ' '.join(value.replace(':', r'\:').replace('=', r'\=')
                       for value in arguments)
    return (
        '# Pinned Milo CI executor; no wildcards, shell, or arbitrary arguments.\n'
        'zacc ALL=(milo-ci:staff) NOPASSWD: NOSETENV: ' + command + '\n'
    ).encode()


def preflight():
    require(os.uname().sysname == 'Darwin', 'macOS is required')
    regular_file(SOURCE, owner=501)
    require(sha256(SOURCE) == SOURCE_SHA256, 'candidate binary hash changed')
    regular_file(POLICY, owner=0)
    owner = pwd.getpwnam('zacc')
    account = pwd.getpwnam('milo-ci')
    require((owner.pw_uid, owner.pw_gid) == (501, 20), 'owner identity changed')
    require((account.pw_uid, account.pw_gid, account.pw_dir) ==
            (502, 20, '/Users/milo-ci'), 'milo-ci identity changed')
    data = ENVIRONMENTS.read_bytes()
    require(data.count(OLD_EXECUTOR.encode()) == 1 or
            data.count(str(DESTINATION).encode()) == 1,
            'expected exactly one old or new executor entry')
    with tempfile.NamedTemporaryFile() as rule:
        rule.write(rule_bytes())
        rule.flush()
        subprocess.run(['/usr/sbin/visudo', '-c', '-f', rule.name],
                       check=True, capture_output=True)
    return data


def install_file(source, destination, mode):
    if destination.exists():
        regular_file(destination, owner=0)
        require(sha256(destination) == SOURCE_SHA256,
                f'existing destination differs: {destination}')
        return False
    destination.parent.mkdir(parents=True, mode=0o755, exist_ok=True)
    require(destination.parent.lstat().st_uid == 0 and
            not destination.parent.lstat().st_mode & 0o022,
            'runtime destination directory is not trusted')
    fd, temporary = tempfile.mkstemp(prefix='.candidate-', dir=destination.parent)
    temporary = Path(temporary)
    try:
        with os.fdopen(fd, 'wb') as output, source.open('rb') as input_stream:
            shutil.copyfileobj(input_stream, output)
            output.flush()
            os.fsync(output.fileno())
        os.chown(temporary, 0, 0)
        os.chmod(temporary, mode)
        os.link(temporary, destination, follow_symlinks=False)
        require(sha256(destination) == SOURCE_SHA256, 'installed binary hash mismatch')
        return True
    finally:
        temporary.unlink(missing_ok=True)


def install_rule():
    content = rule_bytes()
    if RULE.exists():
        regular_file(RULE, owner=0)
        require(RULE.read_bytes() == content, 'existing sudoers rule differs')
        return False
    fd, temporary = tempfile.mkstemp(prefix='.codex-milo-', dir=RULE.parent)
    temporary = Path(temporary)
    try:
        with os.fdopen(fd, 'wb') as output:
            output.write(content)
            output.flush()
            os.fsync(output.fileno())
        os.chown(temporary, 0, 0)
        os.chmod(temporary, 0o440)
        subprocess.run(['/usr/sbin/visudo', '-c', '-f', str(temporary)], check=True)
        os.link(temporary, RULE, follow_symlinks=False)
        subprocess.run(['/usr/sbin/visudo', '-c'], check=True)
        return True
    finally:
        temporary.unlink(missing_ok=True)


def switch_environment(original):
    updated = original.replace(OLD_EXECUTOR.encode(), str(DESTINATION).encode())
    if updated == original:
        require(original.count(str(DESTINATION).encode()) == 1,
                'environment entry is neither old nor new')
        return False
    info = ENVIRONMENTS.stat()
    fd, temporary = tempfile.mkstemp(prefix='.environments-', dir=ENVIRONMENTS.parent)
    temporary = Path(temporary)
    try:
        with os.fdopen(fd, 'wb') as output:
            output.write(updated)
            output.flush()
            os.fsync(output.fileno())
        os.chown(temporary, info.st_uid, info.st_gid)
        os.chmod(temporary, stat.S_IMODE(info.st_mode))
        os.replace(temporary, ENVIRONMENTS)
        return True
    finally:
        temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['check', 'install'])
    action = parser.parse_args().action
    original = preflight()
    if action == 'check':
        print('Candidate hash, identities, environment entry and sudoers syntax verified.')
        return
    require(os.geteuid() == 0, 'installation requires administrator execution')
    binary_created = install_file(SOURCE, DESTINATION, 0o755)
    rule_created = False
    try:
        rule_created = install_rule()
        switch_environment(original)
    except BaseException:
        if ENVIRONMENTS.read_bytes() != original:
            ENVIRONMENTS.write_bytes(original)
            os.chown(ENVIRONMENTS, 501, 20)
        if rule_created and RULE.exists() and RULE.read_bytes() == rule_bytes():
            RULE.unlink()
        if binary_created and DESTINATION.exists() and sha256(DESTINATION) == SOURCE_SHA256:
            DESTINATION.unlink()
        raise
    print('Installed pinned runtime 23568abf and switched only the milo-ci entry.')
    print('Prior runtime and sudoers rule remain available for rollback.')


if __name__ == '__main__':
    try:
        main()
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        raise SystemExit('Refused: ' + str(error))
