//! Minimal dependency-free GitHub Action for witforge.
'use strict';

const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');

const REPOSITORY = 'asamarts/witforge';
const GENERATORS = new Set(['rust', 'proto', 'schema', 'glue']);

function input(name) {
  const key = `INPUT_${name.replaceAll('-', '_').toUpperCase()}`;
  return (process.env[key] || '').trim();
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
    if (String(options.diffCheck || 'true').toLowerCase() !== 'false') args.push('--check');
    commands.push(args);
  }
  const config = String(options.config || '').trim();
  if (config) {
    const generators = splitList(options.generators);
    if (generators.length === 0) throw new Error('config is set but generators is empty');
    for (const generator of generators) {
      if (!GENERATORS.has(generator)) {
        throw new Error(`unknown generator \`${generator}\`; expected rust, proto, schema, or glue`);
      }
      const args = [binary, generator, '--config', config, '--check'];
      const project = String(options.project || '').trim();
      if (project) args.push('--project', project);
      commands.push(args);
    }
  }
  return commands;
}

async function request(url, token) {
  const headers = { 'User-Agent': 'witforge-action' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const response = await fetch(url, { headers });
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

async function download(url, destination, token) {
  const response = await request(url, token);
  fs.writeFileSync(destination, Buffer.from(await response.arrayBuffer()));
}

async function downloadText(url, token) {
  return (await request(url, token)).text();
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
  await download(`${base}/${asset}`, archive, options.token);
  await download(`${base}/${asset}.sha256`, checksumFile, options.token);
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
  const binary = await install({
    version: input('version'),
    token: input('github-token'),
  });
  const commands = buildCommands(binary, {
    check: input('check'),
    diffOld: input('diff-old'),
    diffNew: input('diff-new'),
    diffCheck: input('diff-check'),
    config: input('config'),
    project: input('project'),
    generators: input('generators'),
  });
  for (const command of commands) {
    console.log(`> ${command.join(' ')}`);
    execFileSync(command[0], command.slice(1), { stdio: 'inherit' });
  }
}

module.exports = {
  archiveName,
  assetTarget,
  buildCommands,
  normalizeVersion,
  parseChecksum,
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
