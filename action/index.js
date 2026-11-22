//! Minimal dependency-free GitHub Action for witforge.
'use strict';

const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');

const REPOSITORY = 'asamarts/witforge';
const GENERATORS = new Set(['rust', 'proto', 'schema', 'glue']);
const REQUEST_TIMEOUT_MS = 30_000;

// The runner exports each input as `INPUT_<NAME>`: the name upper-cased, with spaces replaced
// by underscores. A hyphen stays a hyphen, so `diff-old` arrives as `INPUT_DIFF-OLD`. This is
// the rule `getInput` in @actions/core applies.
function input(name, env = process.env) {
  const key = `INPUT_${name.replace(/ /g, '_').toUpperCase()}`;
  return (env[key] || '').trim();
}

// Every input `action.yml` declares, read from the runner's environment.
function readInputs(env = process.env) {
  return {
    install: {
      version: input('version', env),
      token: input('github-token', env),
    },
    commands: {
      check: input('check', env),
      diffOld: input('diff-old', env),
      diffNew: input('diff-new', env),
      diffCheck: input('diff-check', env),
      config: input('config', env),
      project: input('project', env),
      generators: input('generators', env),
      freeze: input('freeze', env),
    },
  };
}

function normalizeVersion(value) {
  const text = String(value || '').trim();
  if (!text || text === 'latest') return 'latest';
  const version = text.startsWith('v') ? text.slice(1) : text;
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`invalid witforge version \`${value}\`; expected X.Y.Z, vX.Y.Z, or latest`);
  }
  return version;
}

function versionFromRef(ref) {
  const match = /^v?(\d+\.\d+\.\d+)$/.exec(String(ref || '').trim());
  return match ? match[1] : null;
}

function resolveVersion(value, ref) {
  const requested = normalizeVersion(value);
  if (requested !== 'latest') return requested;
  return versionFromRef(ref) || 'latest';
}

function assetTarget(platform = process.platform, arch = process.arch) {
  const targets = {
    'linux/x64': 'x86_64-unknown-linux-gnu',
    'darwin/x64': 'x86_64-apple-darwin',
    'darwin/arm64': 'aarch64-apple-darwin',
    'win32/x64': 'x86_64-pc-windows-msvc',
  };
  const target = targets[`${platform}/${arch}`];
  if (!target) {
    throw new Error(
      `no prebuilt witforge release for ${platform}/${arch}; ` +
        `supported: ${Object.keys(targets).join(', ')}`,
    );
  }
  return target;
}

function archiveName(version, target) {
  return `witforge-${version}-${target}.tar.gz`;
}

function parseChecksum(text) {
  const match = /^([0-9a-fA-F]{64})\s+/m.exec(String(text));
  if (!match) throw new Error('release checksum file has no SHA-256 digest');
  return match[1].toLowerCase();
}

function splitLines(value) {
  return String(value || '')
    .split(/\r?\n/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function splitList(value) {
  return String(value || '')
    .split(/[\s,]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function buildCommands(binary, options) {
  const commands = [];
  for (const directory of splitLines(options.check)) {
    commands.push([binary, 'check', directory]);
  }
  const old = String(options.diffOld || '').trim();
  const next = String(options.diffNew || '').trim();
  if (old || next) {
    if (!old || !next) throw new Error('diff-old and diff-new must be set together');
    const args = [binary, 'diff', old, next];
    const diffCheckRaw = String(options.diffCheck ?? '').trim();
    const diffCheck = diffCheckRaw === '' ? 'true' : diffCheckRaw;
    const diffCheckLower = diffCheck.toLowerCase();
    if (diffCheckLower === 'true') {
      args.push('--check');
    } else if (diffCheckLower !== 'false') {
      const views = splitList(diffCheck).join(',');
      if (!views) throw new Error('diff-check view list must not be empty');
      args.push(`--check=${views}`);
    }
    commands.push(args);
  }
  const config = String(options.config || '').trim();
  const project = String(options.project || '').trim();
  const freeze = String(options.freeze || '').trim();
  const freezeEnabled = freeze && freeze.toLowerCase() !== 'false';
  const freezeUsesConfig = freeze.toLowerCase() === 'true';
  if (freezeEnabled) {
    if (freezeUsesConfig) {
      if (!config) {
        throw new Error('freeze: true requires config, or pass WIT root paths');
      }
      const args = [binary, 'freeze', '--config', config];
      if (project) args.push('--project', project);
      commands.push(args);
    } else {
      for (const root of splitLines(freeze)) {
        commands.push([binary, 'freeze', root]);
      }
    }
  }
  if (config) {
    const generators = splitList(options.generators);
    if (generators.length === 0 && !freezeUsesConfig) {
      throw new Error('config is set but generators is empty');
    }
    for (const generator of generators) {
      if (!GENERATORS.has(generator)) {
        throw new Error(`unknown generator \`${generator}\`; expected rust, proto, schema, or glue`);
      }
      const args = [binary, generator, '--config', config, '--check'];
      if (project) args.push('--project', project);
      commands.push(args);
    }
  }
  return commands;
}

async function request(url, token, options = {}) {
  const headers = { 'User-Agent': 'witforge-action' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const timeoutMs = options.timeoutMs ?? REQUEST_TIMEOUT_MS;
  const fetchImpl = options.fetch ?? fetch;
  const signal = AbortSignal.timeout(timeoutMs);
  let response;
  try {
    response = await fetchImpl(url, { headers, signal });
  } catch (error) {
    if (signal.aborted) {
      throw new Error(`GET ${url} timed out after ${timeoutMs}ms`);
    }
    throw error;
  }
  if (!response.ok) {
    throw new Error(`GET ${url} failed with HTTP ${response.status}`);
  }
  return response;
}

async function latestVersion(token) {
  const response = await request(`https://api.github.com/repos/${REPOSITORY}/releases/latest`, token);
  const release = await response.json();
  const tag = String(release.tag_name || '');
  return tag.startsWith('v') ? tag.slice(1) : tag;
}

// Release assets are public and are fetched without credentials: `github-token` is for the
// rate limit of the latest-release API query and is not sent to the download host.
async function download(url, destination, options = {}) {
  const response = await request(url, '', options);
  fs.writeFileSync(destination, Buffer.from(await response.arrayBuffer()));
}

async function install(options) {
  const requested = resolveVersion(options.version, process.env.GITHUB_ACTION_REF);
  const version = requested === 'latest' ? await latestVersion(options.token) : requested;
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`latest release advertised no usable version tag (${version || 'empty'})`);
  }
  const target = assetTarget();
  const asset = archiveName(version, target);
  const base = `https://github.com/${REPOSITORY}/releases/download/v${version}`;
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'witforge-action-'));
  const archive = path.join(temporary, asset);
  const checksumFile = `${archive}.sha256`;
  await download(`${base}/${asset}`, archive);
  await download(`${base}/${asset}.sha256`, checksumFile);
  const expected = parseChecksum(fs.readFileSync(checksumFile, 'utf8'));
  const actual = crypto.createHash('sha256').update(fs.readFileSync(archive)).digest('hex');
  if (actual !== expected) {
    throw new Error(`checksum mismatch for ${asset}: expected ${expected}, got ${actual}`);
  }
  execFileSync('tar', process.platform === 'win32' ? ['-xf', archive, '-C', temporary] : ['-xzf', archive, '-C', temporary], { stdio: 'inherit' });
  const executable = process.platform === 'win32' ? 'witforge.exe' : 'witforge';
  const binary = path.join(temporary, executable);
  if (!fs.existsSync(binary)) throw new Error(`release asset ${asset} did not contain ${executable}`);
  if (process.platform !== 'win32') fs.chmodSync(binary, 0o755);
  if (process.env.GITHUB_PATH) fs.appendFileSync(process.env.GITHUB_PATH, `${temporary}\n`);
  if (process.env.GITHUB_ENV) {
    fs.appendFileSync(process.env.GITHUB_ENV, `WITFORGE_BIN=${binary}\n`);
  }
  console.log(`witforge ${version} (${target}) installed at ${binary}`);
  return binary;
}

async function main() {
  const inputs = readInputs();
  const binary = await install(inputs.install);
  const commands = buildCommands(binary, inputs.commands);
  for (const command of commands) {
    console.log(`> ${command.join(' ')}`);
    execFileSync(command[0], command.slice(1), { stdio: 'inherit' });
  }
}

module.exports = {
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
  splitList,
  versionFromRef,
};

if (require.main === module) {
  main().catch((error) => {
    console.error(`::error::${error.message || error}`);
    process.exitCode = 1;
  });
}
