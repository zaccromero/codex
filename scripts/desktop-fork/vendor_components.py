"""Preserve unmodified vendor runtimes; never reuse their identity for Drako."""
from pathlib import Path
import hashlib

PRESERVED_ROOTS = (
    Path('Contents/Resources/cua_node'),
    Path('Contents/Resources/codex-cli'),
)


def is_preserved(app, target):
    relative = target.relative_to(app)
    return any(relative == root or root in relative.parents for root in PRESERVED_ROOTS)


def component_snapshot(app):
    """Include symlink targets and all resource bytes, including signature seals."""
    result = {}
    for relative in PRESERVED_ROOTS:
        root = app / relative
        if root.is_symlink() or not root.is_dir():
            raise ValueError(f'Missing regular vendor component directory: {relative}')
        for item in sorted(root.rglob('*')):
            key = str(item.relative_to(app))
            if item.is_symlink():
                result[key] = {'link': str(item.readlink())}
            elif item.is_file():
                result[key] = {'sha256': hashlib.sha256(item.read_bytes()).hexdigest(),
                               'mode': item.stat().st_mode & 0o777}
    return result


def verify_preserved(app, expected):
    if not expected or component_snapshot(app) != expected:
        raise ValueError('Vendor runtime components changed; rebuild from the signed source app')
