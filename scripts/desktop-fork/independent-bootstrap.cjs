'use strict';
const path = require('node:path');
const os = require('node:os');
const fs = require('node:fs');

function activate(expectedDesktopVersion) {
  if (typeof expectedDesktopVersion !== 'string' || expectedDesktopVersion.length === 0) {
    throw new Error('Expected desktop version is required');
  }
  // This runs before Electron's module bootstrap is ready. Use Node primitives
  // only; requiring `electron` here recursively re-enters early-bootstrap.
  const root = path.join(os.homedir(), 'Library/Application Support/DrakoCodex');
  process.env.CODEX_HOME = path.join(root, 'codex-home');
  process.env.CODEX_ELECTRON_USER_DATA_PATH = path.join(root, 'desktop-profile');
  for (const directory of [process.env.CODEX_HOME, process.env.CODEX_ELECTRON_USER_DATA_PATH]) {
    fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
  }
  // The vendor updater must not replace this locally maintained distribution.
  process.env.CODEX_SPARKLE_ENABLED = 'false';
  const selected = require(path.join(process.resourcesPath, 'drako-runtime-selector.cjs'))
    .activateSelection(path.join(process.resourcesPath, 'drako-runtime.json'), expectedDesktopVersion);
  pinEnvironment(process, PINNED);
  return selected;
}

const PINNED = ['CODEX_CLI_PATH', 'CODEX_TPP_LOCAL_EXECUTOR_CLI_PATH', 'CODEX_HOME',
  'CODEX_ELECTRON_USER_DATA_PATH', 'CODEX_SPARKLE_ENABLED'];

// The desktop later copies the login shell's environment over its own. A shell
// profile that exports a runtime path would then replace this selection, so
// writes and deletes of the pinned names are ignored for the process lifetime.
function pinEnvironment(target, names) {
  const pinned = new Set(names);
  const proxy = new Proxy(target.env, {
    set(env, key, value) {
      if (!pinned.has(key)) env[key] = value;
      return true;
    },
    deleteProperty(env, key) {
      if (!pinned.has(key)) delete env[key];
      return true;
    },
    defineProperty(env, key, descriptor) {
      if (pinned.has(key)) return true;
      return Reflect.defineProperty(env, key, descriptor);
    },
  });
  Object.defineProperty(target, 'env', {
    configurable: false,
    enumerable: true,
    get: () => proxy,
    // Replacing the whole object keeps the pinned values and takes the rest.
    set(next) {
      for (const [key, value] of Object.entries(next ?? {})) proxy[key] = value;
    },
  });
  return proxy;
}

module.exports = { activate, pinEnvironment, PINNED };
