//! Minimal dependency-free GitHub Action for witforge.
'use strict';

const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync, spawnSync } = require('node:child_process');

const REPOSITORY = 'asamarts/witforge';
const RELEASE_SIGNER_WORKFLOW = `${REPOSITORY}/.github/workflows/release.yml`;
const FIRST_ATTESTED_VERSION = [0, 7, 0];
const FIRST_MUSL_VERSION = [0, 8, 0];
const GENERATORS = new Set(['rust', 'proto', 'schema', 'glue']);
const FORMATS = new Set(['human', 'json', 'sarif']);
const REQUEST_TIMEOUT_MS = 30_000;
const DEFAULT_MAX_ATTEMPTS = 3;
const DEFAULT_RETRY_DELAY_MS = 500;
const DEFAULT_SARIF_FILE = 'witforge.sarif';

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
      binary: input('binary', env),
      token: input('github-token', env),
    },
    commands: {
      check: input('check', env),
      against: input('against', env),
      diffOld: input('diff-old', env),
      diffNew: input('diff-new', env),
      diffCheck: input('diff-check', env),
      config: input('config', env),
      project: input('project', env),
      generators: input('generators', env),
      freeze: input('freeze', env),
      format: input('format', env),
      sarifFile: input('sarif-file', env),
    },
  };
}

function setOutput(name, value, env = process.env) {
  const outputPath = env.GITHUB_OUTPUT;
  if (!outputPath) return;
  const text = String(value ?? '');
  if (text.includes('\n') || text.includes('\r')) {
    const delimiter = `EOF_${crypto.randomBytes(8).toString('hex')}`;
    fs.appendFileSync(outputPath, `${name}<<${delimiter}\n${text}\n${delimiter}\n`);
  } else {
    fs.appendFileSync(outputPath, `${name}=${text}\n`);
  }
}

function workspaceVersion(repoRoot = path.join(__dirname, '..')) {
  const manifestPath = path.join(repoRoot, 'Cargo.toml');
  const text = fs.readFileSync(manifestPath, 'utf8');
  const section = /\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/.exec(text);
  const block = section ? section[1] : text;
  const match = /^\s*version\s*=\s*"(\d+\.\d+\.\d+)"/m.exec(block);
  if (!match) {
    throw new Error(`could not determine workspace version from ${manifestPath}`);
  }
  return match[1];
}

function normalizeVersion(value) {
  const text = String(value || '').trim();
  if (!text) return '';
  if (text === 'latest') return 'latest';
  const version = text.startsWith('v') ? text.slice(1) : text;
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`invalid witforge version \`${value}\`; expected X.Y.Z, vX.Y.Z, or latest`);
  }
  return version;
}

function versionFromRef(ref) {
  const text = String(ref || '').trim();
  const stripped = text.startsWith('refs/tags/') ? text.slice('refs/tags/'.length) : text;
  const match = /^v?(\d+\.\d+\.\d+)$/.exec(stripped);
  return match ? match[1] : null;
}

// A release tag ref installs that release; any other ref (commit SHA, branch, floating major tag,
// or local `./`) installs the workspace version recorded in the Action's own `Cargo.toml`. Never
// falls back to `latest` unless the caller explicitly passed `version: latest`.
function resolveVersion(value, ref, options = {}) {
  const requested = normalizeVersion(value);
  if (requested) return requested;
  const fromRef = versionFromRef(ref);
  if (fromRef) return fromRef;
  return workspaceVersion(options.repoRoot);
}

function normalizeFormat(value) {
  const text = String(value || '').trim().toLowerCase();
  if (!text) return 'human';
  if (!FORMATS.has(text)) {
    throw new Error(`invalid format \`${value}\`; expected human, json, or sarif`);
  }
  return text;
}

function versionAtLeast(version, minParts) {
  const parts = String(version || '')
    .split('.')
    .map((part) => Number.parseInt(part, 10));
  if (parts.length !== 3 || parts.some(Number.isNaN)) return false;
  for (let i = 0; i < 3; i += 1) {
    if (parts[i] > minParts[i]) return true;
    if (parts[i] < minParts[i]) return false;
  }
  return true;
}

function supportsAttestation(version) {
  return versionAtLeast(version, FIRST_ATTESTED_VERSION);
}

function supportsMusl(version) {
  return versionAtLeast(version, FIRST_MUSL_VERSION);
}

function verifyAttestation(archivePath, version, options = {}) {
  const exec = options.execFileSync ?? execFileSync;
  const warn = options.warn ?? ((msg) => console.log(`::warning::${msg}`));
  const notice = options.notice ?? ((msg) => console.log(`::notice::${msg}`));
  if (!supportsAttestation(version)) {
    notice(
      `witforge ${version} predates build provenance attestations (added in 0.7.0); ` +
        'verified by SHA-256 checksum only.',
    );
    return 'unsupported-version';
  }
  const env = { ...process.env };
  if (options.token) env.GH_TOKEN = options.token;
  try {
    exec(
      'gh',
      [
        'attestation',
        'verify',
        archivePath,
        '--repo',
        REPOSITORY,
        '--signer-workflow',
        RELEASE_SIGNER_WORKFLOW,
      ],
      { stdio: 'pipe', env },
    );
    return 'verified';
  } catch (error) {
    if (error && error.code === 'ENOENT') {
      warn(
        '`gh` CLI is not installed on this runner; skipping build provenance attestation ' +
          'verification and falling back to the verified SHA-256 checksum.',
      );
      return 'skipped-no-gh';
    }
    const stderr = error && error.stderr ? String(error.stderr).trim() : '';
    const detail = stderr || (error && error.message) || String(error);
    const authHint =
      !options.token && !env.GH_TOKEN && !env.GITHUB_TOKEN && /gh auth login|GH_TOKEN/i.test(detail)
        ? ' (pass `github-token` or leave it at its default workflow token)'
        : '';
    throw new Error(
      `build provenance attestation verification failed for ${path.basename(archivePath)}: ${detail}${authHint}`,
    );
  }
}

function assetTarget(platform = process.platform, arch = process.arch, version = '') {
  const linuxLibc = supportsMusl(version) ? 'musl' : 'gnu';
  const targets = {
    'linux/x64': `x86_64-unknown-linux-${linuxLibc}`,
    'linux/arm64': `aarch64-unknown-linux-${linuxLibc}`,
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

function parseAgainst(againstRaw, checkDirs) {
  const entries = splitLines(againstRaw);
  const mapping = new Map();
  if (entries.length === 0) return mapping;
  if (checkDirs.length === 0) {
    throw new Error('against requires check to name at least one WIT version directory');
  }
  if (entries.length === 1 && !entries[0].includes('=')) {
    if (checkDirs.length !== 1) {
      throw new Error(
        'against must pair each baseline as `<check-dir>=<baseline-lockfile>` when `check` lists multiple directories',
      );
    }
    mapping.set(checkDirs[0], entries[0]);
    return mapping;
  }
  const known = new Set(checkDirs);
  for (const entry of entries) {
    const eq = entry.indexOf('=');
    if (eq <= 0 || eq === entry.length - 1) {
      throw new Error(
        `invalid against entry \`${entry}\`; expected \`<check-dir>=<baseline-lockfile>\``,
      );
    }
    const dir = entry.slice(0, eq).trim();
    const baseline = entry.slice(eq + 1).trim();
    if (!dir || !baseline) {
      throw new Error(
        `invalid against entry \`${entry}\`; expected \`<check-dir>=<baseline-lockfile>\``,
      );
    }
    if (!known.has(dir)) {
      throw new Error(`against entry \`${dir}\` does not match any directory in \`check\``);
    }
    if (mapping.has(dir)) {
      throw new Error(`duplicate against entry for \`${dir}\``);
    }
    mapping.set(dir, baseline);
  }
  return mapping;
}

function buildCommands(binary, options) {
  const format = normalizeFormat(options.format);
  const sarifFile = String(options.sarifFile || '').trim();
  if (sarifFile && format !== 'sarif') {
    throw new Error('sarif-file requires format: sarif');
  }

  const commands = [];
  const config = String(options.config || '').trim();
  const project = String(options.project || '').trim();
  const checkRaw = String(options.check || '').trim();
  const checkLower = checkRaw.toLowerCase();
  const checkUsesConfig = checkLower === 'true';
  const checkEnabled = checkRaw && checkLower !== 'false';
  if (checkUsesConfig) {
    if (!config) {
      throw new Error('check: true requires config, or pass WIT version directory or root paths');
    }
    if (String(options.against || '').trim()) {
      throw new Error('against cannot be combined with check: true');
    }
    const args = [binary, 'check', '--config', config];
    if (project) args.push('--project', project);
    if (format !== 'human') args.push('--format', format);
    commands.push(args);
  } else {
    const checkDirs = checkEnabled ? splitLines(checkRaw) : [];
    const againstMap = parseAgainst(options.against, checkDirs);
    for (const directory of checkDirs) {
      const args = [binary, 'check', directory];
      const baseline = againstMap.get(directory);
      if (baseline) args.push('--against', baseline);
      if (format !== 'human') args.push('--format', format);
      commands.push(args);
    }
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
    if (format !== 'human') args.push('--format', format);
    commands.push(args);
  }

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
      if (format !== 'human') args.push('--format', format);
      commands.push(args);
    } else {
      for (const root of splitLines(freeze)) {
        const args = [binary, 'freeze', root];
        if (format !== 'human') args.push('--format', format);
        commands.push(args);
      }
    }
  }

  if (config) {
    const generators = splitList(options.generators);
    if (generators.length === 0 && !freezeUsesConfig && !checkUsesConfig) {
      throw new Error('config is set but generators is empty');
    }
    if (generators.includes('all')) {
      if (generators.length !== 1) {
        throw new Error('generators: all cannot be combined with individual generator names');
      }
      const args = [binary, 'generate', '--config', config, '--check'];
      if (project) args.push('--project', project);
      if (format !== 'human') args.push('--format', format);
      commands.push(args);
    } else {
      for (const generator of generators) {
        if (!GENERATORS.has(generator)) {
          throw new Error(
            `unknown generator \`${generator}\`; expected rust, proto, schema, glue, or all`,
          );
        }
        const args = [binary, generator, '--config', config, '--check'];
        if (project) args.push('--project', project);
        if (format !== 'human') args.push('--format', format);
        commands.push(args);
      }
    }
  } else if (
    String(options.generators || '').trim() &&
    String(options.generators || '').trim() !== 'rust'
  ) {
    // Validate explicitly listed generator names even when config is omitted, catching typos up front.
    const generators = splitList(options.generators);
    if (generators.includes('all')) {
      if (generators.length !== 1) {
        throw new Error('generators: all cannot be combined with individual generator names');
      }
    } else {
      for (const generator of generators) {
        if (!GENERATORS.has(generator)) {
          throw new Error(
            `unknown generator \`${generator}\`; expected rust, proto, schema, glue, or all`,
          );
        }
      }
    }
  }

  return commands;
}

// Validate every install and command input before performing any network request.
function validateInputs(inputs, options = {}) {
  const binary = String(inputs.install.binary || '').trim();
  const version = String(inputs.install.version || '').trim();
  if (binary && version) {
    throw new Error('binary and version cannot be set together');
  }
  if (binary) {
    const resolved = path.resolve(binary);
    if (!fs.existsSync(resolved) || !fs.statSync(resolved).isFile()) {
      throw new Error(`binary does not exist or is not a file: ${binary}`);
    }
  } else {
    resolveVersion(version, options.actionRef ?? process.env.GITHUB_ACTION_REF, {
      repoRoot: options.repoRoot,
    });
    assetTarget(options.platform, options.arch);
  }
  const format = normalizeFormat(inputs.commands.format);
  buildCommands(binary || 'witforge', inputs.commands);
  return { format };
}

class HttpStatusError extends Error {
  constructor(url, status) {
    super(`GET ${url} failed with HTTP ${status}`);
    this.status = status;
    this.url = url;
  }
}

function isRetryableError(error) {
  if (error instanceof HttpStatusError) {
    return error.status === 429 || (error.status >= 500 && error.status < 600);
  }
  return true;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function request(url, token, options = {}) {
  const headers = { 'User-Agent': 'witforge-action' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const timeoutMs = options.timeoutMs ?? REQUEST_TIMEOUT_MS;
  const fetchImpl = options.fetch ?? fetch;
  const maxAttempts = options.maxAttempts ?? DEFAULT_MAX_ATTEMPTS;
  const retryDelayMs = options.retryDelayMs ?? DEFAULT_RETRY_DELAY_MS;
  const sleepImpl = options.sleep ?? sleep;

  let lastError;
  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    const signal = AbortSignal.timeout(timeoutMs);
    try {
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
        throw new HttpStatusError(url, response.status);
      }
      return response;
    } catch (error) {
      lastError = error;
      if (attempt >= maxAttempts || !isRetryableError(error)) {
        throw error;
      }
      await sleepImpl(retryDelayMs * attempt);
    }
  }
  throw lastError;
}

async function latestVersion(token, options = {}) {
  const response = await request(
    `https://api.github.com/repos/${REPOSITORY}/releases/latest`,
    token,
    options,
  );
  const release = await response.json();
  const tag = String(release.tag_name || '');
  return tag.startsWith('v') ? tag.slice(1) : tag;
}

// Release assets are public and are fetched without credentials: `github-token` is for the
// rate limit of the latest-release API query and attestation verification, never asset downloads.
async function download(url, destination, options = {}) {
  const response = await request(url, '', options);
  fs.writeFileSync(destination, Buffer.from(await response.arrayBuffer()));
}

async function install(options, runtime = {}) {
  const env = runtime.env ?? process.env;
  const explicitBinary = String(options.binary || '').trim();
  if (explicitBinary) {
    const resolved = path.resolve(explicitBinary);
    if (!fs.existsSync(resolved) || !fs.statSync(resolved).isFile()) {
      throw new Error(`binary does not exist or is not a file: ${explicitBinary}`);
    }
    if (env.GITHUB_PATH) fs.appendFileSync(env.GITHUB_PATH, `${path.dirname(resolved)}\n`);
    if (env.GITHUB_ENV) fs.appendFileSync(env.GITHUB_ENV, `WITFORGE_BIN=${resolved}\n`);
    console.log(`using local witforge binary at ${resolved}`);
    return resolved;
  }

  const requested = resolveVersion(options.version, env.GITHUB_ACTION_REF, {
    repoRoot: runtime.repoRoot,
  });
  const version =
    requested === 'latest' ? await latestVersion(options.token, runtime) : requested;
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`latest release advertised no usable version tag (${version || 'empty'})`);
  }
  const target = assetTarget(runtime.platform, runtime.arch, version);
  const asset = archiveName(version, target);
  const base = `https://github.com/${REPOSITORY}/releases/download/v${version}`;
  const tempRoot = env.RUNNER_TEMP || os.tmpdir();
  fs.mkdirSync(tempRoot, { recursive: true });
  const temporary = fs.mkdtempSync(path.join(tempRoot, 'witforge-action-'));
  const archive = path.join(temporary, asset);
  const checksumFile = `${archive}.sha256`;
  const downloadFn = runtime.download ?? download;
  const execFn = runtime.execFileSync ?? execFileSync;

  try {
    try {
      await downloadFn(`${base}/${asset}`, archive, runtime);
      await downloadFn(`${base}/${asset}.sha256`, checksumFile, runtime);
    } catch (error) {
      if (error instanceof HttpStatusError && error.status === 404) {
        throw new Error(
          `no published witforge ${version} release assets found (${error.url} returned HTTP 404); ` +
            'pass the `version` input to select a published release (or `binary` for a local build)',
        );
      }
      throw error;
    }

    const expected = parseChecksum(fs.readFileSync(checksumFile, 'utf8'));
    const actual = crypto.createHash('sha256').update(fs.readFileSync(archive)).digest('hex');
    if (actual !== expected) {
      throw new Error(`checksum mismatch for ${asset}: expected ${expected}, got ${actual}`);
    }
    verifyAttestation(archive, version, {
      token: options.token,
      execFileSync: runtime.verifyExecFileSync ?? execFn,
      warn: runtime.warn,
      notice: runtime.notice,
    });

    const tarArgs =
      (runtime.platform ?? process.platform) === 'win32'
        ? ['-xf', archive, '-C', temporary]
        : ['-xzf', archive, '-C', temporary];
    execFn('tar', tarArgs, { stdio: 'inherit' });
    // Remove the archive and checksum scratch files while keeping the extracted executable under
    // RUNNER_TEMP on PATH for subsequent workflow steps.
    fs.rmSync(archive, { force: true });
    fs.rmSync(checksumFile, { force: true });

    const executable =
      (runtime.platform ?? process.platform) === 'win32' ? 'witforge.exe' : 'witforge';
    const binary = path.join(temporary, executable);
    if (!fs.existsSync(binary)) {
      throw new Error(`release asset ${asset} did not contain ${executable}`);
    }
    if ((runtime.platform ?? process.platform) !== 'win32') fs.chmodSync(binary, 0o755);
    if (env.GITHUB_PATH) fs.appendFileSync(env.GITHUB_PATH, `${temporary}\n`);
    if (env.GITHUB_ENV) fs.appendFileSync(env.GITHUB_ENV, `WITFORGE_BIN=${binary}\n`);
    console.log(`witforge ${version} (${target}) installed at ${binary}`);
    return binary;
  } catch (error) {
    fs.rmSync(temporary, { recursive: true, force: true });
    throw error;
  }
}

function escapeAnnotationProperty(value) {
  return String(value)
    .replace(/%/g, '%25')
    .replace(/\r/g, '%0D')
    .replace(/\n/g, '%0A')
    .replace(/:/g, '%3A')
    .replace(/,/g, '%2C');
}

function escapeAnnotationData(value) {
  return String(value)
    .replace(/%/g, '%25')
    .replace(/\r/g, '%0D')
    .replace(/\n/g, '%0A');
}

function formatAnnotationLine({ level, file, line, col, endLine, endColumn, title, message }) {
  const props = [];
  if (file) props.push(`file=${escapeAnnotationProperty(file)}`);
  if (line) props.push(`line=${line}`);
  if (col) props.push(`col=${col}`);
  if (endLine) props.push(`endLine=${endLine}`);
  if (endColumn) props.push(`endColumn=${endColumn}`);
  if (title) props.push(`title=${escapeAnnotationProperty(title)}`);
  const header = props.length > 0 ? `::${level} ${props.join(',')}::` : `::${level}::`;
  return `${header}${escapeAnnotationData(message)}`;
}

function annotationsFromJsonReport(report) {
  if (!report || !Array.isArray(report.diagnostics)) return [];
  const lines = [];
  for (const diag of report.diagnostics) {
    const severity = String(diag.severity || 'error').toLowerCase();
    if (severity === 'note') continue;
    const level = severity === 'warning' ? 'warning' : 'error';
    const pos = diag.position || diag;
    lines.push(
      formatAnnotationLine({
        level,
        file: diag.file || undefined,
        line: pos.line || undefined,
        col: pos.column || undefined,
        endLine: pos.end_line || undefined,
        endColumn: pos.end_column || undefined,
        title: diag.code || 'witforge',
        message: diag.message || '',
      }),
    );
  }
  return lines;
}

function annotationsFromSarifDocument(doc) {
  if (!doc || !Array.isArray(doc.runs)) return [];
  const lines = [];
  for (const run of doc.runs) {
    if (!Array.isArray(run.results)) continue;
    for (const result of run.results) {
      const sarifLevel = String(result.level || 'error').toLowerCase();
      if (sarifLevel === 'note' || sarifLevel === 'none') continue;
      const level = sarifLevel === 'warning' ? 'warning' : 'error';
      const loc =
        Array.isArray(result.locations) && result.locations[0]
          ? result.locations[0].physicalLocation
          : null;
      const file = loc && loc.artifactLocation ? loc.artifactLocation.uri : undefined;
      const region = loc ? loc.region : null;
      lines.push(
        formatAnnotationLine({
          level,
          file,
          line: region ? region.startLine : undefined,
          col: region ? region.startColumn : undefined,
          endLine: region ? region.endLine : undefined,
          endColumn: region ? region.endColumn : undefined,
          title: result.ruleId || 'witforge',
          message: (result.message && result.message.text) || '',
        }),
      );
    }
  }
  return lines;
}

function commandLabel(command) {
  const subcommand = command[1] || '';
  return subcommand ? `witforge ${subcommand}` : 'witforge';
}

function emitAnnotations(lines, write = (line) => console.log(line)) {
  for (const line of lines) write(line);
}

function runCommands(commands, options = {}) {
  const format = normalizeFormat(options.format);
  const sarifFile = String(options.sarifFile || '').trim() || DEFAULT_SARIF_FILE;
  const spawn = options.spawnSync ?? spawnSync;
  const env = options.env ?? process.env;
  const log = options.log ?? ((line) => console.log(line));
  const sarifRuns = [];
  let failed = null;

  for (const command of commands) {
    log(`> ${command.join(' ')}`);
    if (format === 'human') {
      const result = spawn(command[0], command.slice(1), { stdio: 'inherit' });
      if (result.error) throw result.error;
      if (result.status !== 0) {
        // Re-run the same read-only check/diff/freeze/--check command in structured JSON mode so
        // GitHub workflow annotations receive exact file/line/column spans.
        const probe = spawn(command[0], [...command.slice(1), '--format', 'json'], {
          encoding: 'utf8',
          stdio: ['ignore', 'pipe', 'pipe'],
        });
        if (!probe.error && probe.stdout) {
          try {
            const parsed = JSON.parse(probe.stdout);
            emitAnnotations(annotationsFromJsonReport(parsed), log);
          } catch {
            // Fall back cleanly if the installed binary did not emit JSON diagnostics.
          }
        }
        failed = { command, status: result.status };
        break;
      }
      continue;
    }

    const result = spawn(command[0], command.slice(1), {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    if (result.error) throw result.error;
    if (result.stderr) process.stderr.write(result.stderr);
    if (format === 'json') {
      if (result.stdout) process.stdout.write(result.stdout);
      try {
        const parsed = JSON.parse(result.stdout || '{}');
        emitAnnotations(annotationsFromJsonReport(parsed), log);
      } catch {
        // Ignore unparseable stdout when a command failed before emitting JSON.
      }
      if (result.status !== 0) {
        failed = { command, status: result.status };
        break;
      }
    } else if (format === 'sarif') {
      try {
        const parsed = JSON.parse(result.stdout || '{}');
        emitAnnotations(annotationsFromSarifDocument(parsed), log);
        if (Array.isArray(parsed.runs)) sarifRuns.push(...parsed.runs);
      } catch {
        // Keep stderr visible and still record command failure below.
      }
      if (result.status !== 0) {
        failed = { command, status: result.status };
        break;
      }
    }
  }

  if (format === 'sarif') {
    const sarifDoc = {
      $schema: 'https://json.schemastore.org/sarif-2.1.0.json',
      version: '2.1.0',
      runs: sarifRuns,
    };
    const resolvedSarif = path.resolve(sarifFile);
    fs.mkdirSync(path.dirname(resolvedSarif), { recursive: true });
    fs.writeFileSync(resolvedSarif, `${JSON.stringify(sarifDoc, null, 2)}\n`);
    setOutput('sarif-file', sarifFile, env);
  }

  if (failed) {
    const label = commandLabel(failed.command);
    setOutput('failed-command', label, env);
    throw new Error(`${label} exited with status ${failed.status}`);
  }
  setOutput('failed-command', '', env);
}

async function main() {
  const inputs = readInputs();
  const { format } = validateInputs(inputs);
  let binary;
  try {
    binary = await install(inputs.install);
  } catch (error) {
    setOutput('failed-command', 'install');
    throw error;
  }
  const commands = buildCommands(binary, inputs.commands);
  runCommands(commands, { format, sarifFile: inputs.commands.sarifFile });
}

module.exports = {
  DEFAULT_MAX_ATTEMPTS,
  DEFAULT_RETRY_DELAY_MS,
  DEFAULT_SARIF_FILE,
  HttpStatusError,
  RELEASE_SIGNER_WORKFLOW,
  REQUEST_TIMEOUT_MS,
  annotationsFromJsonReport,
  annotationsFromSarifDocument,
  archiveName,
  assetTarget,
  buildCommands,
  commandLabel,
  download,
  formatAnnotationLine,
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
  setOutput,
  splitLines,
  splitList,
  supportsAttestation,
  supportsMusl,
  validateInputs,
  verifyAttestation,
  versionFromRef,
  workspaceVersion,
};

if (require.main === module) {
  main().catch((error) => {
    console.error(`::error::${error.message || error}`);
    process.exitCode = 1;
  });
}
