'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');

const {
  archiveName,
  assetTarget,
  buildCommands,
  normalizeVersion,
  parseChecksum,
  resolveVersion,
  splitLines,
} = require('./index');

test('platform targets and archive names are stable', () => {
  assert.equal(assetTarget('linux', 'x64'), 'x86_64-unknown-linux-gnu');
  assert.equal(assetTarget('darwin', 'arm64'), 'aarch64-apple-darwin');
  assert.equal(assetTarget('win32', 'x64'), 'x86_64-pc-windows-msvc');
  assert.equal(
    archiveName('0.3.0', 'x86_64-unknown-linux-gnu'),
    'witforge-0.3.0-x86_64-unknown-linux-gnu.tar.gz',
  );
  assert.throws(() => assetTarget('linux', 'ia32'), /no prebuilt/);
});

test('pinned action refs select the same release and floating refs use latest', () => {
  assert.equal(normalizeVersion('v0.3.0'), '0.3.0');
  assert.equal(normalizeVersion('latest'), 'latest');
  assert.equal(resolveVersion('', 'v0.3.0'), '0.3.0');
  assert.equal(resolveVersion('', 'v0'), 'latest');
  assert.equal(resolveVersion('', 'main'), 'latest');
  assert.equal(resolveVersion('v0.4.0', 'v0.3.0'), '0.4.0');
  assert.throws(() => normalizeVersion('v0.3'), /invalid/);
});

test('checksum parsing and list splitting', () => {
  assert.equal(
    parseChecksum('ab'.repeat(32) + '  witforge-0.3.0.tar.gz\n'),
    'ab'.repeat(32),
  );
  assert.deepEqual(splitLines(' wit/v0.1\r\n\r\nwit/v0.2 '), ['wit/v0.1', 'wit/v0.2']);
});

test('the command plan runs check, diff, and configured generators', () => {
  const commands = buildCommands('witforge', {
    check: 'wit/v0.2\nstore/v0.1',
    diffOld: 'wit/v0.1',
    diffNew: 'wit/v0.2',
    diffCheck: 'true',
    config: 'witforge.toml',
    project: 'demo',
    generators: 'rust,proto',
  });
  assert.deepEqual(commands, [
    ['witforge', 'check', 'wit/v0.2'],
    ['witforge', 'check', 'store/v0.1'],
    ['witforge', 'diff', 'wit/v0.1', 'wit/v0.2', '--check'],
    ['witforge', 'rust', '--config', 'witforge.toml', '--check', '--project', 'demo'],
    ['witforge', 'proto', '--config', 'witforge.toml', '--check', '--project', 'demo'],
  ]);
});

test('incomplete or unknown inputs are refused', () => {
  assert.throws(() => buildCommands('witforge', { diffOld: 'v0.1' }), /together/);
  assert.throws(
    () => buildCommands('witforge', { config: 'witforge.toml', generators: 'rust,unknown' }),
    /unknown generator/,
  );
  const noCheck = buildCommands('witforge', {
    diffOld: 'v0.1',
    diffNew: 'v0.2',
    diffCheck: 'false',
  });
  assert.deepEqual(noCheck, [['witforge', 'diff', 'v0.1', 'v0.2']]);
});
