'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { selectRuntime, activateSelection } = require('./runtime-selector.cjs');

function fixture(t) {
  const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'fork-selection-')));
  t.after(() => fs.rmSync(dir, { recursive: true }));
  fs.mkdirSync(path.join(dir, 'bin'));
  const manifest = { schemaVersion: 1, packageRoot: dir, sha256: {} };
  for (const name of ['codex', 'codex-code-mode-host', 'codex-exec-server']) {
    const data = Buffer.from(`#!/bin/sh\nprintf '${name} fixture\\n'\n`);
    fs.writeFileSync(path.join(dir, 'bin', name), data, { mode: 0o700 });
    manifest.sha256[`bin/${name}`] = crypto.createHash('sha256').update(data).digest('hex');
  }
  const file = path.join(dir, 'runtime.json');
  fs.writeFileSync(file, JSON.stringify(manifest), { mode: 0o600 });
  return { dir, file, manifest };
}

for (const inherited of [undefined, '/stale/bundled/codex']) {
  test(`persistent selection wins with inherited override ${inherited}`, t => {
    const { dir, file } = fixture(t);
    const old = process.env.CODEX_CLI_PATH;
    t.after(() => { if (old === undefined) delete process.env.CODEX_CLI_PATH; else process.env.CODEX_CLI_PATH = old; });
    if (inherited === undefined) delete process.env.CODEX_CLI_PATH; else process.env.CODEX_CLI_PATH = inherited;
    assert.equal(activateSelection(file).executable, path.join(dir, 'bin/codex'));
    assert.equal(process.env.CODEX_CLI_PATH, path.join(dir, 'bin/codex'));
  });
}
test('tampering fails closed', t => {
  const { dir, file } = fixture(t);
  fs.appendFileSync(path.join(dir, 'bin/codex'), '# changed');
  assert.throws(() => selectRuntime(file), /digest mismatch/);
});
test('missing companion fails closed', t => {
  const { dir, file } = fixture(t);
  fs.unlinkSync(path.join(dir, 'bin/codex-exec-server'));
  assert.throws(() => selectRuntime(file), /ENOENT/);
});
test('writable manifest rejected', t => {
  const { file } = fixture(t); fs.chmodSync(file, 0o666);
  assert.throws(() => selectRuntime(file), /owner-controlled/);
});
test('missing selection never falls back to bundled runtime', () => {
  assert.throws(() => selectRuntime('/nonexistent/drako-runtime.json'), /ENOENT/);
});
test('desktop version changes require revalidation', t => {
  const { file } = fixture(t);
  assert.throws(() => activateSelection(file, 'new-desktop-version'), /revalidated/);
});
test('embedded runtime resolves relative to a relocated app manifest', t => {
  const { dir, file, manifest } = fixture(t);
  fs.mkdirSync(path.join(dir, 'drako-runtime'));
  fs.renameSync(path.join(dir, 'bin'), path.join(dir, 'drako-runtime/bin'));
  delete manifest.packageRoot;
  manifest.packageRootRelative = 'drako-runtime';
  fs.writeFileSync(file, JSON.stringify(manifest));
  assert.equal(selectRuntime(file).executable, path.join(dir, 'drako-runtime/bin/codex'));
});
test('relative manifest cannot escape its resources directory', t => {
  const { file, manifest } = fixture(t);
  delete manifest.packageRoot;
  manifest.packageRootRelative = '../other-runtime';
  fs.writeFileSync(file, JSON.stringify(manifest));
  assert.throws(() => selectRuntime(file), /Unsupported/);
});
