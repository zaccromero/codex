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
    const resources = path.join(receipt.candidateApp, 'Contents/Resources');
    const mockProcess = { resourcesPath: resources, env: process.env };
    function load(id) {
      if (id === 'node:os') return { homedir: () => home };
      if (id === path.join(resources, 'drako-independent-bootstrap.cjs')) {
        const module = { exports: {} };
        vm.runInNewContext(fs.readFileSync(id, 'utf8'), { process: mockProcess, require: load, module });
        return module.exports;
      }
      return require(id);
    }
    const context = { process: mockProcess, require: load };
    vm.runInNewContext(prefix, context);
    const runtimeRoot = receipt.independent ? path.join(resources, 'drako-runtime') : receipt.runtime.packageRoot;
    assert.equal(process.env.CODEX_CLI_PATH, path.join(runtimeRoot, 'bin/codex'));
    if (receipt.independent) {
      assert.equal(process.env.CODEX_HOME, path.join(home, 'Library/Application Support/DrakoCodex/codex-home'));
      assert.equal(process.env.CODEX_ELECTRON_USER_DATA_PATH,
        path.join(home, 'Library/Application Support/DrakoCodex/desktop-profile'));
      assert.equal(process.env.CODEX_SPARKLE_ENABLED, 'false');
    }
    const version = cp.execFileSync(process.env.CODEX_CLI_PATH, ['--version'], { encoding: 'utf8' }).trim();
    assert.match(version, /^codex-cli /);
    console.log(JSON.stringify({ inherited: inherited ?? null, selected: process.env.CODEX_CLI_PATH, version }));
  }
} finally {
  fs.rmSync(home, { recursive: true });
}
