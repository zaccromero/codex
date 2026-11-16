#!/usr/bin/env python3
"""Provision only the pinned milo-ci executor bootstrap; never a general sudo grant."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import pwd
import re
import stat
import subprocess
import sys
import tempfile
import tomllib


PREVIOUS_EXECUTOR = Path('/Library/CodexAccounts/runtime-eb342b19/bin/codex-exec-server')
PREVIOUS_SHA256 = '42a37bdf852ece5790a3b1014a3932cc41e5ed3b05d9624a79e269111c739ce8'
EXECUTOR_SHA256 = '94d1ca6a711e8e7f676e9dd1d4a86eb5714f5a8830485c58cab6558fa74663c2'
EXECUTOR = Path('/Library/CodexAccounts/runtime-' + EXECUTOR_SHA256[:12] + '/bin/codex-exec-server')
CANDIDATE = Path(__file__).resolve().parents[2] / '.drako/artifacts/account-executor/target/aarch64-apple-darwin/debug/codex-exec-server'
POLICY = Path('/Library/CodexAccounts/milo-ci.json')
RULE = Path('/private/etc/sudoers.d/codex-milo-ci-' + EXECUTOR_SHA256[:12])
RECEIPT = EXECUTOR.parent / 'installation.json'
CONFIG = Path('/Users/zacc/.codex/config.toml')
ENVIRONMENTS = Path('/Users/zacc/.codex/environments.toml')
CI_PATH = '/opt/homebrew/opt/node@24/bin:/Users/milo-ci/ci/milo-ci-v1/tools/bin:/Users/milo-ci/ci/milo-ci-v1/tools/node_modules/.bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin'
EXPECTED_POLICY = dict(schema_version=1, username='milo-ci', uid=502, gid=20,
                       home='/Users/milo-ci', root='/Users/milo-ci/ci/milo-ci-v1', path=CI_PATH)
PRIVATE_DENIALS = ('~/.aws', '~/.claude/backups', '~/.claude/daemon',
                  '~/.claude/file-history', '~/.claude/projects', '~/.claude/sessions',
                  '~/.claude/shell-snapshots', '~/.claude/telemetry', '~/.codex/auth.json',
                  '~/.config/doctl', '~/.config/gcloud', '~/.docker', '~/.gnupg',
                  '~/.hermes', '~/.kube', '~/.ssh')
GLOB_DENIALS = ('**/.env', '**/.env.*.local', '**/.env.dev', '**/.env.development',
               '**/.env.local', '**/.env.prod', '**/.env.production', '**/.env.sandbox',
               '**/.env.stage', '**/.env.staging', '**/.env.test')


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def trusted_file(path, expected_uid=0):
    """Return bytes from a stable regular file after checking its whole path."""
    require(path.is_absolute() and '..' not in path.parts, 'noncanonical trusted path')
    for part in (path, *path.parents):
        info = part.lstat()
        require(not stat.S_ISLNK(info.st_mode) and info.st_uid == expected_uid
                and not info.st_mode & 0o022, f'untrusted ownership or mode: {part}')
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_uid == expected_uid
                and not info.st_mode & 0o022, f'untrusted file: {path}')
        return stream.read()


def validate_coding(data):
    cfg = tomllib.loads(data.decode())
    require(cfg.get('default_permissions') == 'coding', 'default profile is not coding')
    require(cfg.get('approval_policy') == 'on-request'
            and cfg.get('approvals_reviewer') == 'auto_review', 'approval settings changed')
    fs = cfg['permissions']['coding']['filesystem']
    require(all(fs.get(p) == 'deny' for p in PRIVATE_DENIALS), 'private-path denials changed')
    require(all(fs.get('/', {}).get(p) == 'deny' for p in GLOB_DENIALS), 'secret globs changed')
    require(fs.get('/', {}).get('**/.env.*') != 'deny', 'template access is blocked')


def sudoers_escape(value):
    # No shell quoting: sudoers has its own escaping and argument matching rules.
    require(re.fullmatch(r'[A-Za-z0-9_/.=:@#-]+', value) is not None,
            'unsafe character in bootstrap argument')
    return re.sub(r'([\\,:=])', r'\\\1', value)


def rule_bytes():
    args = ['/usr/bin/env', '-i', 'HOME=/Users/milo-ci', 'USER=milo-ci',
            'LOGNAME=milo-ci', 'PATH=' + CI_PATH, str(EXECUTOR), '--listen', 'stdio',
            '--execution-account-policy', str(POLICY)]
    command = ' '.join(sudoers_escape(arg) for arg in args)
    return ('# Managed by the pinned Codex account-bootstrap installer.\n'
            '# No wildcards, root target, shell, SETENV, or arbitrary arguments.\n'
            'zacc ALL=(milo-ci:staff) NOPASSWD: NOSETENV: ' + command + '\n').encode()


def candidate_bytes():
    fd = os.open(CANDIDATE, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_uid in (0, 501), 'untrusted candidate file')
        data = stream.read()
    require(hashlib.sha256(data).hexdigest() == EXECUTOR_SHA256, 'candidate hash differs from reviewed build')
    return data


def preflight():
    require(os.uname().sysname == 'Darwin', 'macOS is required')
    require(hashlib.sha256(trusted_file(PREVIOUS_EXECUTOR)).hexdigest() == PREVIOUS_SHA256,
            'previous executor changed; refusing an ambiguous upgrade')
    candidate_bytes()
    if EXECUTOR.exists() or EXECUTOR.is_symlink():
        require(hashlib.sha256(trusted_file(EXECUTOR)).hexdigest() == EXECUTOR_SHA256,
                'installed executor does not match the reviewed binary')
    require(json.loads(trusted_file(POLICY)) == EXPECTED_POLICY, 'installed policy differs')
    owner, account = pwd.getpwnam('zacc'), pwd.getpwnam('milo-ci')
    require(owner.pw_uid == 501 and owner.pw_dir == '/Users/zacc', 'owner identity differs')
    require((account.pw_uid, account.pw_gid, account.pw_dir) ==
            (502, 20, '/Users/milo-ci'), 'milo-ci identity differs')
    require(not {0, 80}.intersection(os.getgrouplist('milo-ci', 20)), 'privileged CI account')
    root = Path(EXPECTED_POLICY['root'])
    require(root.resolve(strict=True) == root and root.is_dir(), 'CI root is absent or aliased')
    original = CONFIG.read_bytes()
    validate_coding(original)
    environments = tomllib.loads(ENVIRONMENTS.read_text())
    entry = [e for e in environments.get('environments', []) if e.get('id') == 'milo-ci']
    require(len(entry) == 1, 'expected exactly one registered milo-ci environment')
    entry = entry[0]
    require(entry.get('program') in (str(PREVIOUS_EXECUTOR), str(EXECUTOR)) and entry.get('args') ==
            ['--listen', 'stdio', '--execution-account-policy', str(POLICY)]
            and entry.get('cwd') == str(root), 'host entry does not match the pinned executor')
    return original


def install_package():
    """Publish an immutable version without touching the prior runtime or policy."""
    payload = candidate_bytes()
    trusted_directory(EXECUTOR.parents[2])
    version = EXECUTOR.parents[1]
    if version.exists() or version.is_symlink():
        require(hashlib.sha256(trusted_file(EXECUTOR)).hexdigest() == EXECUTOR_SHA256,
                'existing version differs or is incomplete; preserved')
        return False
    version.mkdir(mode=0o755)  # Exclusive: another install is never overwritten.
    version.chmod(0o755)
    try:
        EXECUTOR.parent.mkdir(mode=0o755)
        EXECUTOR.parent.chmod(0o755)
        fd = os.open(EXECUTOR, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o755)
        with os.fdopen(fd, 'wb') as stream:
            os.fchmod(stream.fileno(), 0o755)
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        require(hashlib.sha256(trusted_file(EXECUTOR)).hexdigest() == EXECUTOR_SHA256,
                'installed package verification failed')
        return True
    except BaseException:
        # No sudo rule or host registration exists yet. Keep partial artifacts
        # for inspection rather than delete something another administrator changed.
        raise


def registration_bytes(original, executor):
    """Change only this environment's program, preserving every other byte."""
    text = original.decode()
    blocks = re.split(r'(?m)(?=^\[\[environments\]\]\s*$)', text)
    matches = 0
    for index, block in enumerate(blocks):
        parsed = tomllib.loads(block)
        entries = parsed.get('environments', [])
        if len(entries) != 1 or entries[0].get('id') != 'milo-ci':
            continue
        require(entries[0].get('program') in (str(PREVIOUS_EXECUTOR), str(EXECUTOR)),
                'environment program changed; preserved')
        block, count = re.subn(r'(?m)^program\s*=\s*[\"\'][^\n]*[\"\']\s*$',
                              'program = ' + json.dumps(str(executor)), block)
        require(count == 1, 'ambiguous environment program')
        blocks[index] = block
        matches += 1
    require(matches == 1, 'expected exactly one milo-ci environment')
    updated = ''.join(blocks).encode()
    old, new = tomllib.loads(original.decode()), tomllib.loads(updated.decode())
    for entry in old['environments']:
        if entry.get('id') == 'milo-ci':
            entry['program'] = str(executor)
    require(old == new, 'registration would change unrelated settings')
    return updated


def activate():
    require(os.geteuid() == 501, 'activation must run as zacc, not root')
    original_config = preflight()
    original = ENVIRONMENTS.read_bytes()
    updated = registration_bytes(original, EXECUTOR)
    if not installed_receipt_matches():
        # The only privileged operation is this reviewed package/rule installer.
        command = ['/usr/bin/sudo']
        if not sys.stdin.isatty():
            command.append('-n')
        command += ['--', sys.executable, '-I', str(Path(__file__).resolve()), 'install']
        result = subprocess.run(command)
        require(result.returncode == 0, 'administrator package installation did not complete; registration unchanged')
    require(CONFIG.read_bytes() == original_config, 'coding configuration changed; registration preserved')
    require(hashlib.sha256(trusted_file(EXECUTOR)).hexdigest() == EXECUTOR_SHA256,
            'installed candidate differs')
    require(installed_receipt_matches(), 'administrator installation receipt differs')
    boot = subprocess.run([str(EXECUTOR), '--listen', 'stdio', '--execution-account-policy', str(POLICY)],
                          input='', capture_output=True, text=True, timeout=30)
    require(boot.returncode == 0, 'account bootstrap failed; registration unchanged: ' + boot.stderr.strip())
    write_registration(original, updated)
    require(CONFIG.read_bytes() == original_config, 'coding configuration changed during activation')
    print('Verified package, exact bootstrap grant and account startup; milo-ci registration updated.')
    print('Coding profile unchanged. Live host connection and CI proof must still be verified through the host.')


def write_registration(original, updated):
    require(not ENVIRONMENTS.is_symlink() and ENVIRONMENTS.stat().st_uid == 501,
            'untrusted owner registration')
    fd, name = tempfile.mkstemp(prefix='.environments-', dir=ENVIRONMENTS.parent)
    staging = Path(name)
    try:
        with os.fdopen(fd, 'wb') as stream:
            os.fchmod(stream.fileno(), stat.S_IMODE(ENVIRONMENTS.stat().st_mode))
            stream.write(updated)
            stream.flush()
            os.fsync(stream.fileno())
        require(ENVIRONMENTS.read_bytes() == original, 'registration changed concurrently; preserved')
        os.replace(staging, ENVIRONMENTS)
    finally:
        staging.unlink(missing_ok=True)


def receipt_bytes():
    return (json.dumps(dict(schema=1, executor_sha256=EXECUTOR_SHA256,
                           rule_sha256=hashlib.sha256(rule_bytes()).hexdigest()), sort_keys=True) + '\n').encode()


def installed_receipt_matches():
    try:
        return trusted_file(RECEIPT) == receipt_bytes()
    except OSError:
        return False


def install_receipt():
    # Readable attestation; no sudoers content or credentials are exported.
    if RECEIPT.exists() or RECEIPT.is_symlink():
        require(trusted_file(RECEIPT) == receipt_bytes(), 'installation receipt differs; preserved')
        return
    fd = os.open(RECEIPT, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o644)
    with os.fdopen(fd, 'wb') as stream:
        os.fchmod(stream.fileno(), 0o644)
        stream.write(receipt_bytes())
        stream.flush()
        os.fsync(stream.fileno())


def visudo(path=None):
    command = ['/usr/sbin/visudo', '-c']
    if path is not None:
        command.extend(['-f', str(path)])
    result = subprocess.run(command, capture_output=True, text=True, timeout=30)
    require(result.returncode == 0, 'sudoers validation failed: ' + result.stderr.strip())


def trusted_directory(directory):
    for part in (directory, *directory.parents):
        info = part.lstat()
        require(stat.S_ISDIR(info.st_mode) and info.st_uid == 0
                and not info.st_mode & 0o022, f'untrusted directory: {part}')


def install_rule(destination, content, check):
    """Exclusive install. Roll back only this exact file if global validation fails."""
    if destination.exists() or destination.is_symlink():
        require(trusted_file(destination) == content, 'existing sudoers rule differs; preserved')
        check(None)
        return False
    directory = destination.parent
    trusted_directory(directory)
    fd, name = tempfile.mkstemp(prefix='.codex-account-', dir=directory)
    staging = Path(name)
    installed = False
    try:
        with os.fdopen(fd, 'wb') as stream:
            os.fchmod(stream.fileno(), 0o440)
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        check(staging)
        # link is atomic and refuses an existing destination, unlike replace.
        os.link(staging, destination, follow_symlinks=False)
        installed = True
        check(None)
        return True
    except BaseException:
        if installed and destination.exists():
            if (os.path.samestat(staging.lstat(), destination.lstat())
                    and destination.read_bytes() == content):
                destination.unlink()
        raise
    finally:
        staging.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['check', 'install', 'activate', 'deactivate', 'rollback'])
    args = parser.parse_args()
    content = rule_bytes()
    if args.action == 'activate':
        activate()
        return
    if args.action == 'deactivate':
        require(os.geteuid() == 501, 'deactivation must run as zacc, not root')
        original_config = preflight()
        original = ENVIRONMENTS.read_bytes()
        write_registration(original, registration_bytes(original, PREVIOUS_EXECUTOR))
        require(CONFIG.read_bytes() == original_config, 'coding configuration changed')
        print('Previous host registration restored. Packages and exact bootstrap grant preserved.')
        return
    if args.action == 'rollback':
        require(os.geteuid() == 0, 'rollback requires administrator execution')
        entries = tomllib.loads(ENVIRONMENTS.read_text()).get('environments', [])
        require(not any(e.get('id') == 'milo-ci' and e.get('program') == str(EXECUTOR) for e in entries),
                'run deactivate as the owner before removing the active bootstrap grant')
        require(trusted_file(RULE) == content, 'rule changed; refusing removal')
        # Removal grants no new privilege; no unrelated rule is touched.
        RULE.unlink()
        visudo()
        if RECEIPT.exists():
            require(trusted_file(RECEIPT) == receipt_bytes(), 'receipt changed; preserved')
            RECEIPT.unlink()
        print('Removed only the matching account-bootstrap rule; runtime and coding profile preserved.')
        return
    original = preflight()
    if args.action == 'check':
        with tempfile.NamedTemporaryFile() as file:
            file.write(content)
            file.flush()
            visudo(Path(file.name))
        print('Package, account, host entry, saved coding protections and sudoers syntax verified.')
        print('No installation or live activation performed.')
        return
    require(os.geteuid() == 0, 'install requires administrator execution')
    visudo()
    install_package()
    created = install_rule(RULE, content, visudo)
    try:
        require(CONFIG.read_bytes() == original, 'coding configuration changed during installation')
        install_receipt()
    except BaseException:
        if created and trusted_file(RULE) == content:
            RULE.unlink()
        raise
    print('Exact account-bootstrap permission installed. Coding configuration unchanged.')
    print('Host connection, protected workload admission and CI checks still require live verification.')


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, KeyError, ValueError, subprocess.TimeoutExpired) as error:
        raise SystemExit('Refused: ' + str(error))
