'use strict';
// Exercise the exact prefix extracted from the staged app, without its UI code.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const cp = require('node:child_process');
const output = path.resolve(process.argv[2]);
const receipt = JSON.parse(fs.readFileSync(path.join(output, 'receipt.json')));
const prefix = fs.readFileSync(path.join(output, 'verified-bootstrap-prefix.cjs'), 'utf8');
const home = fs.mkdtempSync(path.join(os.tmpdir(), 'fork-bootstrap-'));
try {
  const selection = path.join(home, 'Library/Application Support/DrakoCodex/runtime.json');
  fs.mkdirSync(path.dirname(selection), { recursive: true });
  fs.copyFileSync(receipt.manifest, selection);
  fs.chmodSync(selection, 0o600);
  for (const inherited of [undefined, '/stale/bundled/codex']) {
    if (inherited === undefined) delete process.env.CODEX_CLI_PATH;
    else process.env.CODEX_CLI_PATH = inherited;
    const context = { process: { resourcesPath: path.join(receipt.candidateApp, 'Contents/Resources') },
      require: id => id === 'node:os' ? { homedir: () => home } :
        id === 'electron' ? { app: { getVersion: () => receipt.sourceVersion } } : require(id) };
    vm.runInNewContext(prefix, context);
    assert.equal(process.env.CODEX_CLI_PATH, path.join(receipt.runtime.packageRoot, 'bin/codex'));
    const version = cp.execFileSync(process.env.CODEX_CLI_PATH, ['--version'], { encoding: 'utf8' }).trim();
    assert.match(version, /^codex-cli /);
    console.log(JSON.stringify({ inherited: inherited ?? null, selected: process.env.CODEX_CLI_PATH, version }));
  }
} finally {
  fs.rmSync(home, { recursive: true });
}
