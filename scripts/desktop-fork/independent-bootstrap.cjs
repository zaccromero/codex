'use strict';
const path = require('node:path');
const os = require('node:os');
const fs = require('node:fs');
const { app } = require('electron');

// Set before any vendor module is evaluated. Never reuse the vendor profile.
const root = path.join(os.homedir(), 'Library/Application Support/DrakoCodex');
process.env.CODEX_HOME = path.join(root, 'codex-home');
process.env.CODEX_ELECTRON_USER_DATA_PATH = path.join(root, 'desktop-profile');
for (const directory of [process.env.CODEX_HOME, process.env.CODEX_ELECTRON_USER_DATA_PATH]) {
  fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
}
// The vendor updater must not replace this locally maintained distribution.
process.env.CODEX_SPARKLE_ENABLED = 'false';
app.setPath('userData', process.env.CODEX_ELECTRON_USER_DATA_PATH);
require(path.join(process.resourcesPath, 'drako-runtime-selector.cjs'))
  .activateSelection(path.join(process.resourcesPath, 'drako-runtime.json'), app.getVersion());
