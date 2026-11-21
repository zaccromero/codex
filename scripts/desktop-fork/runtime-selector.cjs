'use strict';

// Loaded before the vendor desktop bootstrap. No app-server or app restart here.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const os = require('node:os');

function selectRuntime(manifestPath) {
  const stat = fs.lstatSync(manifestPath);
  if (!stat.isFile() || stat.isSymbolicLink() || (stat.mode & 0o022) ||
      (process.getuid && stat.uid !== process.getuid())) {
    throw new Error('Fork runtime manifest must be an owner-controlled regular file');
  }
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  const relative = manifest.packageRootRelative;
  if (manifest.schemaVersion !== 1 ||
      (relative !== undefined ? relative !== 'drako-runtime' : !path.isAbsolute(manifest.packageRoot || ''))) {
    throw new Error('Unsupported fork runtime manifest');
  }
  const root = fs.realpathSync(relative ? path.join(path.dirname(manifestPath), relative) : manifest.packageRoot);
  for (const name of ['codex', 'codex-code-mode-host', 'codex-exec-server']) {
    const relative = `bin/${name}`;
    const expected = manifest.sha256?.[relative];
    if (!/^[a-f0-9]{64}$/.test(expected || '')) throw new Error(`Missing digest: ${relative}`);
    const target = path.join(root, relative);
    if (fs.realpathSync(target) !== target) throw new Error(`Symlinked runtime component: ${relative}`);
    const component = fs.statSync(target);
    if (!component.isFile() || (component.mode & 0o022) ||
        (process.getuid && component.uid !== process.getuid())) {
      throw new Error(`Runtime component is not owner-controlled: ${relative}`);
    }
    fs.accessSync(target, fs.constants.X_OK);
    const actual = crypto.createHash('sha256').update(fs.readFileSync(target)).digest('hex');
    if (actual !== expected) throw new Error(`Fork runtime digest mismatch: ${relative}`);
  }
  return { executable: path.join(root, 'bin/codex'), manifest };
}

function activateSelection(manifestPath, desktopVersion) {
  const selected = selectRuntime(manifestPath);
  if (desktopVersion !== undefined && selected.manifest.desktopVersion !== desktopVersion) {
    throw new Error('Fork runtime selection must be revalidated for this desktop version');
  }
  // The app's selector reads this before creating any local app-server.
  // A stale inherited override cannot supersede the persisted selection.
  process.env.CODEX_CLI_PATH = selected.executable;
  // The local executor reads a separate override; leave no second selector.
  process.env.CODEX_TPP_LOCAL_EXECUTOR_CLI_PATH = selected.executable;
  delete process.env.CODEX_APP_RUNTIME_CLI_PATH;
  return selected;
}

module.exports = { selectRuntime, activateSelection };
if (require.main === module) {
  const manifest = process.argv[2] || path.join(os.homedir(), 'Library/Application Support/DrakoCodex/runtime.json');
  console.log(JSON.stringify(selectRuntime(manifest)));
}
