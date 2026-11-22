'use strict';

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');

const {
  HttpStatusError,
  REQUEST_TIMEOUT_MS,
  annotationsFromJsonReport,
  annotationsFromSarifDocument,
  archiveName,
  assetTarget,
  buildCommands,
  download,
  input,
  install,
  normalizeFormat,
  normalizeVersion,
  parseAgainst,
  parseChecksum,
  readInputs,
  request,
  resolveVersion,
  runCommands,
  splitLines,
  supportsAttestation,
  validateInputs,
  verifyAttestation,
  workspaceVersion,
} = require('./index');

const WORKSPACE_VERSION = workspaceVersion();

test('platform targets and archive names are stable', () => {
  assert.equal(assetTarget('linux', 'x64'), 'x86_64-unknown-linux-gnu');
  assert.equal(assetTarget('linux', 'arm64'), 'aarch64-unknown-linux-gnu');
  assert.equal(assetTarget('darwin', 'x64'), 'x86_64-apple-darwin');
  assert.equal(assetTarget('darwin', 'arm64'), 'aarch64-apple-darwin');
  assert.equal(assetTarget('win32', 'x64'), 'x86_64-pc-windows-msvc');
  assert.equal(
    archiveName('0.3.0', 'x86_64-unknown-linux-gnu'),
    'witforge-0.3.0-x86_64-unknown-linux-gnu.tar.gz',
  );
  assert.throws(() => assetTarget('linux', 'ia32'), /no prebuilt.*linux\/ia32/);
});

test('version resolution uses release tags, workspace version for SHAs/branches/local, and latest only when requested', () => {
  assert.equal(normalizeVersion('v0.3.0'), '0.3.0');
  assert.equal(normalizeVersion('latest'), 'latest');
  assert.equal(normalizeVersion(''), '');

  // Release tags select their exact version.
  assert.equal(resolveVersion('', 'v0.3.0'), '0.3.0');
  assert.equal(resolveVersion('', 'refs/tags/v0.5.0'), '0.5.0');

  // Commit SHAs, branches, floating tags, and empty local `./` refs resolve to the Action tree's
  // own Cargo.toml workspace version — never silently falling back to `latest`.
  const sha = 'a1b2c3d4e5f60718293a4b5c6d7e8f9012345678';
  assert.equal(resolveVersion('', sha), WORKSPACE_VERSION);
  assert.equal(resolveVersion('', 'main'), WORKSPACE_VERSION);
  assert.equal(resolveVersion('', 'v0'), WORKSPACE_VERSION);
  assert.equal(resolveVersion('', ''), WORKSPACE_VERSION);

  // Explicit `version` input overrides ref, including explicit `latest`.
  assert.equal(resolveVersion('v0.4.0', 'v0.3.0'), '0.4.0');
  assert.equal(resolveVersion('latest', 'v0.3.0'), 'latest');
  assert.equal(resolveVersion('latest', sha), 'latest');
  assert.throws(() => normalizeVersion('v0.3'), /invalid/);
});

test('checksum parsing and list splitting', () => {
  assert.equal(
    parseChecksum('ab'.repeat(32) + '  witforge-0.3.0.tar.gz\n'),
    'ab'.repeat(32),
  );
  assert.deepEqual(splitLines(' wit/v0.1\r\n\r\nwit/v0.2 '), ['wit/v0.1', 'wit/v0.2']);
});

test('the command plan runs check, against, diff, freeze, format, and configured generators', () => {
  const commands = buildCommands('witforge', {
    check: 'wit/v0.2\nstore/v0.2',
    against: 'wit/v0.2=wit/v0.1/wire.lock\nstore/v0.2=store/v0.1/wire.lock',
    diffOld: 'wit/v0.1',
    diffNew: 'wit/v0.2',
    diffCheck: 'true',
    config: 'witforge.toml',
    project: 'demo',
    generators: 'rust,proto',
    freeze: 'true',
    format: 'sarif',
    sarifFile: 'out/report.sarif',
  });
  assert.deepEqual(commands, [
    ['witforge', 'check', 'wit/v0.2', '--against', 'wit/v0.1/wire.lock', '--format', 'sarif'],
    ['witforge', 'check', 'store/v0.2', '--against', 'store/v0.1/wire.lock', '--format', 'sarif'],
    ['witforge', 'diff', 'wit/v0.1', 'wit/v0.2', '--check', '--format', 'sarif'],
    ['witforge', 'freeze', '--config', 'witforge.toml', '--project', 'demo', '--format', 'sarif'],
    [
      'witforge',
      'rust',
      '--config',
      'witforge.toml',
      '--check',
      '--project',
      'demo',
      '--format',
      'sarif',
    ],
    [
      'witforge',
      'proto',
      '--config',
      'witforge.toml',
      '--check',
      '--project',
      'demo',
      '--format',
      'sarif',
    ],
  ]);

  // Single-directory `against` shorthand applies directly to the single `check` entry.
  assert.deepEqual(
    buildCommands('witforge', {
      check: 'wit/v0.2',
      against: 'wit/v0.1/wire.lock',
    }),
    [['witforge', 'check', 'wit/v0.2', '--against', 'wit/v0.1/wire.lock']],
  );
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
  const end = metadata.indexOf('\noutputs:\n');
  assert.ok(start >= 0 && end > start, 'action.yml has an inputs block before outputs');
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
  const { install: installOpts, commands } = readInputs(
    runnerEnvironment({
      version: '0.5.0',
      'github-token': 'secret',
      check: 'wit/v0.2',
      against: 'wit/v0.1/wire.lock',
      'diff-old': 'wit/v0.1',
      'diff-new': 'wit/v0.2',
      'diff-check': 'false',
      format: 'json',
    }),
  );
  assert.deepEqual(installOpts, { version: '0.5.0', binary: '', token: 'secret' });
  assert.deepEqual(buildCommands('witforge', commands), [
    ['witforge', 'check', 'wit/v0.2', '--against', 'wit/v0.1/wire.lock', '--format', 'json'],
    ['witforge', 'diff', 'wit/v0.1', 'wit/v0.2', '--format', 'json'],
  ]);
});

test('every input action.yml declares is read', () => {
  const names = declaredInputs();
  assert.deepEqual(names, [
    'version',
    'binary',
    'check',
    'against',
    'diff-old',
    'diff-new',
    'diff-check',
    'config',
    'project',
    'generators',
    'freeze',
    'format',
    'sarif-file',
    'github-token',
  ]);
  const { install: installOpts, commands } = readInputs(
    runnerEnvironment(Object.fromEntries(names.map((name) => [name, `value of ${name}`]))),
  );
  const read = [...Object.values(installOpts), ...Object.values(commands)].sort();
  assert.deepEqual(read, names.map((name) => `value of ${name}`).sort());

  const ciGuide = fs.readFileSync(path.join(__dirname, '..', 'docs', 'guide', 'ci.md'), 'utf8');
  for (const name of names) {
    assert.ok(
      ciGuide.includes(`\`${name}\``),
      `public docs/guide/ci.md must document Action input \`${name}\``,
    );
  }
});

test('incomplete or unknown inputs are refused before any download', () => {
  assert.throws(() => buildCommands('witforge', { diffOld: 'v0.1' }), /together/);
  assert.throws(
    () => buildCommands('witforge', { config: 'witforge.toml', generators: 'rust,unknown' }),
    /unknown generator/,
  );
  assert.throws(
    () => buildCommands('witforge', { generators: 'rust,typo' }),
    /unknown generator/,
  );
  assert.throws(() => normalizeFormat('xml'), /invalid format/);
  assert.throws(
    () => buildCommands('witforge', { format: 'human', sarifFile: 'a.sarif' }),
    /sarif-file requires format: sarif/,
  );
  assert.throws(() => parseAgainst('v0.1/wire.lock', []), /against requires check/);
  assert.throws(
    () => parseAgainst('v0.1/wire.lock', ['v0.2', 'other/v0.2']),
    /pair each baseline/,
  );
  assert.throws(
    () => parseAgainst('missing=v0.1/wire.lock', ['v0.2']),
    /does not match any directory/,
  );
  assert.throws(
    () => parseAgainst('v0.2=a\nv0.2=b', ['v0.2']),
    /duplicate against entry/,
  );
  assert.throws(
    () =>
      validateInputs(
        readInputs(runnerEnvironment({ version: '0.6.0', binary: '/bin/sh' })),
        { platform: 'linux', arch: 'x64' },
      ),
    /binary and version cannot be set together/,
  );
  assert.throws(
    () =>
      validateInputs(
        readInputs(runnerEnvironment({ binary: '/nonexistent/witforge-missing' })),
        { platform: 'linux', arch: 'x64' },
      ),
    /binary does not exist/,
  );
  assert.throws(
    () =>
      validateInputs(
        readInputs(runnerEnvironment({ version: 'bad', 'diff-old': 'v0.1' })),
        { platform: 'linux', arch: 'x64' },
      ),
    /invalid witforge version/,
  );
  assert.throws(
    () =>
      validateInputs(
        readInputs(runnerEnvironment({ version: '0.6.0', 'diff-old': 'v0.1' })),
        { platform: 'linux', arch: 'x64' },
      ),
    /together/,
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
  assert.throws(
    () => buildCommands('witforge', { freeze: 'true' }),
    /freeze: true requires config/,
  );
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

test('requests retry transient 5xx and network errors but never retry 4xx', async () => {
  let attempts = 0;
  const response = { ok: true, status: 200 };
  const returned = await request('https://example.test/release', 'secret', {
    timeoutMs: 50,
    maxAttempts: 3,
    retryDelayMs: 1,
    sleep: async () => {},
    fetch: async (_url, init) => {
      attempts += 1;
      assert.equal(init.headers.Authorization, 'Bearer secret');
      assert.ok(init.signal instanceof AbortSignal);
      if (attempts < 3) return { ok: false, status: 503 };
      return response;
    },
  });
  assert.equal(returned, response);
  assert.equal(attempts, 3);
  assert.equal(REQUEST_TIMEOUT_MS, 30_000);

  let notFoundAttempts = 0;
  await assert.rejects(
    request('https://example.test/missing', '', {
      maxAttempts: 3,
      retryDelayMs: 1,
      sleep: async () => {},
      fetch: async () => {
        notFoundAttempts += 1;
        return { ok: false, status: 404 };
      },
    }),
    (err) => err instanceof HttpStatusError && err.status === 404,
  );
  assert.equal(notFoundAttempts, 1, 'HTTP 404 must not be retried');
});

test('release assets are downloaded without credentials', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'witforge-action-test-'));
  const destination = path.join(directory, 'asset');
  let requestInit;
  try {
    await download('https://example.test/asset', destination, {
      fetch: async (_url, init) => {
        requestInit = init;
        return {
          ok: true,
          status: 200,
          arrayBuffer: async () => new TextEncoder().encode('bytes').buffer,
        };
      },
    });
    assert.equal(requestInit.headers.Authorization, undefined);
    assert.equal(fs.readFileSync(destination, 'utf8'), 'bytes');
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test('install names `version` on 404, never retries checksum mismatch, and cleans scratch files', async () => {
  const runnerTemp = fs.mkdtempSync(path.join(os.tmpdir(), 'witforge-action-install-'));
  try {
    // 1. Missing release assets fail naming the `version` input and clean up scratch dir.
    await assert.rejects(
      install(
        { version: '', binary: '', token: '' },
        {
          env: { RUNNER_TEMP: runnerTemp, GITHUB_ACTION_REF: 'main' },
          platform: 'linux',
          arch: 'x64',
          download: async (url) => {
            throw new HttpStatusError(url, 404);
          },
        },
      ),
      /pass the `version` input/,
    );
    assert.deepEqual(fs.readdirSync(runnerTemp), [], 'failed install must remove scratch dir');

    // 2. Checksum mismatch is never retried and removes the scratch dir.
    let downloadCalls = 0;
    await assert.rejects(
      install(
        { version: '0.6.0', binary: '', token: '' },
        {
          env: { RUNNER_TEMP: runnerTemp },
          platform: 'linux',
          arch: 'x64',
          download: async (url, dest) => {
            downloadCalls += 1;
            if (url.endsWith('.sha256')) {
              fs.writeFileSync(dest, `${'0'.repeat(64)}  asset.tar.gz\n`);
            } else {
              fs.writeFileSync(dest, 'tampered');
            }
          },
        },
      ),
      /checksum mismatch/,
    );
    assert.equal(downloadCalls, 2, 'checksum mismatch must never retry downloads');
    assert.deepEqual(fs.readdirSync(runnerTemp), [], 'checksum mismatch must clean scratch dir');

    // 3. Successful install removes archive + checksum files while keeping binary on PATH.
    const payload = Buffer.from('fake-archive');
    const digest = crypto.createHash('sha256').update(payload).digest('hex');
    const ghPath = path.join(runnerTemp, 'gh_path');
    const ghEnv = path.join(runnerTemp, 'gh_env');
    fs.writeFileSync(ghPath, '');
    fs.writeFileSync(ghEnv, '');
    const binary = await install(
      { version: '0.6.0', binary: '', token: '' },
      {
        env: { RUNNER_TEMP: runnerTemp, GITHUB_PATH: ghPath, GITHUB_ENV: ghEnv },
        platform: 'linux',
        arch: 'x64',
        notice: () => {},
        download: async (url, dest) => {
          if (url.endsWith('.sha256')) {
            fs.writeFileSync(dest, `${digest}  witforge-0.6.0-x86_64-unknown-linux-gnu.tar.gz\n`);
          } else {
            fs.writeFileSync(dest, payload);
          }
        },
        execFileSync: (_cmd, args) => {
          const destDir = args[args.indexOf('-C') + 1];
          fs.writeFileSync(path.join(destDir, 'witforge'), '#!/bin/sh\n');
        },
      },
    );
    const installDir = path.dirname(binary);
    assert.deepEqual(
      fs.readdirSync(installDir),
      ['witforge'],
      'archive and checksum scratch files must be removed after extraction',
    );
  } finally {
    fs.rmSync(runnerTemp, { recursive: true, force: true });
  }
});

test('requests abort when the timeout expires', async () => {
  const keepAlive = setTimeout(() => {}, 10_000);
  try {
    await assert.rejects(
      request('https://example.test/slow', '', {
        timeoutMs: 10,
        maxAttempts: 1,
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

test('attestation verification runs for >=0.7.0, warns when gh is absent, and fails on invalid provenance', () => {
  assert.equal(supportsAttestation('0.6.0'), false);
  assert.equal(supportsAttestation('0.7.0'), true);
  assert.equal(supportsAttestation('1.0.0'), true);

  const notices = [];
  assert.equal(
    verifyAttestation('/tmp/witforge-0.6.0.tar.gz', '0.6.0', {
      notice: (msg) => notices.push(msg),
      execFileSync: () => {
        throw new Error('must not run gh for pre-0.7.0');
      },
    }),
    'unsupported-version',
  );
  assert.equal(notices.length, 1);

  let captured;
  assert.equal(
    verifyAttestation('/tmp/witforge-0.7.0.tar.gz', '0.7.0', {
      token: 'tok',
      execFileSync: (cmd, args, opts) => {
        captured = { cmd, args, ghToken: opts.env.GH_TOKEN };
      },
    }),
    'verified',
  );
  assert.equal(captured.cmd, 'gh');
  assert.deepEqual(captured.args, [
    'attestation',
    'verify',
    '/tmp/witforge-0.7.0.tar.gz',
    '--repo',
    'asamarts/witforge',
    '--signer-workflow',
    'asamarts/witforge/.github/workflows/release.yml',
  ]);
  assert.equal(captured.ghToken, 'tok');

  const warnings = [];
  assert.equal(
    verifyAttestation('/tmp/witforge-0.7.0.tar.gz', '0.7.0', {
      warn: (msg) => warnings.push(msg),
      execFileSync: () => {
        const err = new Error('spawnSync gh ENOENT');
        err.code = 'ENOENT';
        throw err;
      },
    }),
    'skipped-no-gh',
  );
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /falling back to the verified SHA-256 checksum/);

  assert.throws(
    () =>
      verifyAttestation('/tmp/witforge-0.7.0.tar.gz', '0.7.0', {
        execFileSync: () => {
          const err = new Error('verification failed');
          err.stderr = 'no matching attestation';
          throw err;
        },
      }),
    /attestation verification failed.*no matching attestation/,
  );
});

test('workflow annotations and failed-command/sarif outputs are emitted from diagnostics', () => {
  const fromJson = annotationsFromJsonReport({
    diagnostics: [
      {
        code: 'WF1010',
        severity: 'error',
        file: 'wit/v0.2/contract.wit',
        line: 4,
        column: 5,
        end_line: 4,
        end_column: 18,
        message: 'resources are not supported',
      },
      {
        code: 'WF3004',
        severity: 'note',
        message: 'informational note',
      },
    ],
  });
  assert.deepEqual(fromJson, [
    '::error file=wit/v0.2/contract.wit,line=4,col=5,endLine=4,endColumn=18,title=WF1010::resources are not supported',
  ]);

  const fromSarif = annotationsFromSarifDocument({
    version: '2.1.0',
    runs: [
      {
        results: [
          {
            ruleId: 'WF3001',
            level: 'warning',
            message: { text: 'added field breaks source' },
            locations: [
              {
                physicalLocation: {
                  artifactLocation: { uri: 'wit/v0.2/contract.wit' },
                  region: { startLine: 7, startColumn: 3, endLine: 7, endColumn: 15 },
                },
              },
            ],
          },
        ],
      },
    ],
  });
  assert.deepEqual(fromSarif, [
    '::warning file=wit/v0.2/contract.wit,line=7,col=3,endLine=7,endColumn=15,title=WF3001::added field breaks source',
  ]);

  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'witforge-action-run-'));
  const ghOutput = path.join(tmp, 'gh_output');
  const sarifOut = path.join(tmp, 'combined.sarif');
  fs.writeFileSync(ghOutput, '');
  const logs = [];
  try {
    assert.throws(
      () =>
        runCommands([['witforge', 'diff', 'v0.1', 'v0.2', '--check', '--format', 'sarif']], {
          format: 'sarif',
          sarifFile: sarifOut,
          env: { GITHUB_OUTPUT: ghOutput },
          log: (line) => logs.push(line),
          spawnSync: () => ({
            status: 1,
            stdout: JSON.stringify({
              version: '2.1.0',
              runs: [
                {
                  tool: { driver: { name: 'witforge' } },
                  results: [
                    {
                      ruleId: 'WF3001',
                      level: 'error',
                      message: { text: 'breaking change' },
                      locations: [
                        {
                          physicalLocation: {
                            artifactLocation: { uri: 'wit/v0.2/contract.wit' },
                            region: { startLine: 2, startColumn: 1 },
                          },
                        },
                      ],
                    },
                  ],
                },
              ],
            }),
            stderr: '',
          }),
        }),
      /witforge diff exited with status 1/,
    );
    const outputs = fs.readFileSync(ghOutput, 'utf8');
    assert.match(outputs, /failed-command=witforge diff/);
    assert.match(outputs, new RegExp(`sarif-file=${sarifOut.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`));
    const writtenSarif = JSON.parse(fs.readFileSync(sarifOut, 'utf8'));
    assert.equal(writtenSarif.version, '2.1.0');
    assert.equal(writtenSarif.runs.length, 1);
    assert.ok(
      logs.some((line) => line.startsWith('::error file=wit/v0.2/contract.wit,line=2,col=1,title=WF3001::')),
      `expected annotation in logs: ${logs.join('\n')}`,
    );
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
