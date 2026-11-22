'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');

const {
  REQUEST_TIMEOUT_MS,
  archiveName,
  assetTarget,
  buildCommands,
  download,
  input,
  normalizeVersion,
  parseChecksum,
  readInputs,
  request,
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

// The names GitHub's runner sets: upper-cased, spaces replaced, hyphens kept.
function runnerEnvironment(inputs) {
  const env = {};
  for (const [name, value] of Object.entries(inputs)) {
    env[`INPUT_${name.replace(/ /g, '_').toUpperCase()}`] = value;
  }
  return env;
}

function declaredInputs() {
  const metadata = fs.readFileSync(path.join(__dirname, '..', 'action.yml'), 'utf8');
  const start = metadata.indexOf('\ninputs:\n');
  const end = metadata.indexOf('\nruns:\n');
  assert.ok(start >= 0 && end > start, 'action.yml has an inputs block before runs');
  return [...metadata.slice(start, end).matchAll(/^ {2}([a-z][a-z0-9-]*):$/gm)].map(
    (match) => match[1],
  );
}

test('inputs are read under the environment names the runner sets', () => {
  const env = runnerEnvironment({ 'diff-old': ' wit/v0.1 ', check: 'wit/v0.2' });
  assert.deepEqual(Object.keys(env), ['INPUT_DIFF-OLD', 'INPUT_CHECK']);
  assert.equal(input('diff-old', env), 'wit/v0.1');
  assert.equal(input('check', env), 'wit/v0.2');
  assert.equal(input('diff-new', env), '');
  // An underscore spelling is a different variable; the runner never sets it.
  assert.equal(input('diff-old', { INPUT_DIFF_OLD: 'wit/v0.1' }), '');
});

test('hyphenated inputs reach the install options and the command plan', () => {
  const { install, commands } = readInputs(
    runnerEnvironment({
      version: '0.5.0',
      'github-token': 'secret',
      check: 'wit/v0.2',
      'diff-old': 'wit/v0.1',
      'diff-new': 'wit/v0.2',
      'diff-check': 'false',
    }),
  );
  assert.deepEqual(install, { version: '0.5.0', token: 'secret' });
  assert.deepEqual(buildCommands('witforge', commands), [
    ['witforge', 'check', 'wit/v0.2'],
    ['witforge', 'diff', 'wit/v0.1', 'wit/v0.2'],
  ]);
});

test('every input action.yml declares is read', () => {
  const names = declaredInputs();
  assert.deepEqual(names, [
    'version',
    'check',
    'diff-old',
    'diff-new',
    'diff-check',
    'config',
    'project',
    'generators',
    'freeze',
    'github-token',
  ]);
  const { install, commands } = readInputs(
    runnerEnvironment(Object.fromEntries(names.map((name) => [name, `value of ${name}`]))),
  );
  const read = [...Object.values(install), ...Object.values(commands)].sort();
  assert.deepEqual(read, names.map((name) => `value of ${name}`).sort());
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
  assert.deepEqual(
    buildCommands('witforge', {
      diffOld: 'v0.1',
      diffNew: 'v0.2',
      diffCheck: 'wire',
    }),
    [['witforge', 'diff', 'v0.1', 'v0.2', '--check=wire']],
  );
  assert.deepEqual(
    buildCommands('witforge', {
      diffOld: 'v0.1',
      diffNew: 'v0.2',
      diffCheck: 'wire, api',
    }),
    [['witforge', 'diff', 'v0.1', 'v0.2', '--check=wire,api']],
  );
  assert.throws(
    () => buildCommands('witforge', { diffOld: 'v0.1', diffNew: 'v0.2', diffCheck: ',,,' }),
    /diff-check view list must not be empty/,
  );
  assert.throws(() => buildCommands('witforge', { freeze: 'true' }), /freeze: true requires config/);
});

test('freeze input builds witforge freeze commands from roots or config', () => {
  assert.deepEqual(
    buildCommands('witforge', {
      freeze: 'wit\nother/wit',
    }),
    [
      ['witforge', 'freeze', 'wit'],
      ['witforge', 'freeze', 'other/wit'],
    ],
  );
  assert.deepEqual(
    buildCommands('witforge', {
      freeze: 'true',
      config: 'witforge.toml',
      project: 'demo',
      generators: 'rust',
    }),
    [
      ['witforge', 'freeze', '--config', 'witforge.toml', '--project', 'demo'],
      ['witforge', 'rust', '--config', 'witforge.toml', '--check', '--project', 'demo'],
    ],
  );
  assert.deepEqual(
    buildCommands('witforge', {
      freeze: 'true',
      config: 'witforge.toml',
      generators: '',
    }),
    [['witforge', 'freeze', '--config', 'witforge.toml']],
  );
  assert.deepEqual(buildCommands('witforge', { freeze: 'false' }), []);
});

test('requests carry a bounded abort signal and authorization', async () => {
  let requestInit;
  const response = { ok: true, status: 200 };
  const returned = await request('https://example.test/release', 'secret', {
    timeoutMs: 50,
    fetch: async (_url, init) => {
      requestInit = init;
      return response;
    },
  });
  assert.equal(returned, response);
  assert.equal(requestInit.headers.Authorization, 'Bearer secret');
  assert.ok(requestInit.signal instanceof AbortSignal);
  assert.equal(requestInit.signal.aborted, false);
  assert.equal(REQUEST_TIMEOUT_MS, 30_000);
});

test('release assets are downloaded without credentials', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'witforge-action-test-'));
  const destination = path.join(directory, 'asset');
  let requestInit;
  try {
    await download('https://example.test/asset', destination, {
      fetch: async (_url, init) => {
        requestInit = init;
        return { ok: true, status: 200, arrayBuffer: async () => new TextEncoder().encode('bytes').buffer };
      },
    });
    assert.equal(requestInit.headers.Authorization, undefined);
    assert.equal(fs.readFileSync(destination, 'utf8'), 'bytes');
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test('requests abort when the timeout expires', async () => {
  // A real request holds a socket open until the signal aborts it. This fetch holds nothing, and
  // before Node 24 the timer behind AbortSignal.timeout() does not keep the event loop alive on
  // its own, so the runner would see the loop drain and cancel the test.
  const keepAlive = setTimeout(() => {}, 10_000);
  try {
    await assert.rejects(
      request('https://example.test/slow', '', {
        timeoutMs: 10,
        fetch: (_url, init) =>
          new Promise((_resolve, reject) => {
            init.signal.addEventListener('abort', () => reject(new Error('request aborted')));
          }),
      }),
      /timed out after 10ms/,
    );
  } finally {
    clearTimeout(keepAlive);
  }
});
