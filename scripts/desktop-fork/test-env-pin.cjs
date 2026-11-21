'use strict';
// The selection must survive the desktop's later login-shell environment import.
const test = require('node:test');
const assert = require('node:assert/strict');
const { pinEnvironment, PINNED } = require('./independent-bootstrap.cjs');

function fixture() {
  const target = { env: { CODEX_CLI_PATH: '/drako/bin/codex', CODEX_HOME: '/drako/home', PATH: '/usr/bin' } };
  pinEnvironment(target, PINNED);
  return target;
}

test('assignment, bulk import and deletion cannot replace pinned names', () => {
  const target = fixture();
  target.env.CODEX_CLI_PATH = '/shell/profile/codex';
  Object.assign(target.env, { CODEX_CLI_PATH: '/shell/profile/codex', CODEX_HOME: '/other', EDITOR: 'vi' });
  delete target.env.CODEX_CLI_PATH;
  Object.defineProperty(target.env, 'CODEX_HOME', { value: '/defined', writable: true, enumerable: true, configurable: true });
  assert.equal(target.env.CODEX_CLI_PATH, '/drako/bin/codex');
  assert.equal(target.env.CODEX_HOME, '/drako/home');
  assert.equal(target.env.EDITOR, 'vi');
});

test('replacing the whole environment keeps pinned names and takes the rest', () => {
  const target = fixture();
  target.env = { CODEX_CLI_PATH: '/shell/profile/codex', PATH: '/opt/bin', NEW: '1' };
  assert.deepEqual({ ...target.env }, { CODEX_CLI_PATH: '/drako/bin/codex', CODEX_HOME: '/drako/home', PATH: '/opt/bin', NEW: '1' });
  assert.throws(() => Object.defineProperty(target, 'env', { value: {} }), TypeError);
});

test('unpinned names stay fully writable and spawn-style copies see the pinned values', () => {
  const target = fixture();
  target.env.PATH = '/x';
  delete target.env.PATH;
  assert.equal('PATH' in target.env, false);
  assert.equal({ ...target.env, EXTRA: '1' }.CODEX_CLI_PATH, '/drako/bin/codex');
  assert.deepEqual(Object.keys(target.env).sort(), ['CODEX_CLI_PATH', 'CODEX_HOME']);
});
