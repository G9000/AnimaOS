// HELD root-owned proposal. Imports no candidate code into this controller process.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  lstat,
  mkdtemp,
  readFile,
  rm,
  stat,
  utimes,
  writeFile,
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const manifest = JSON.parse(
  await readFile(new URL('./expected-cases.json', import.meta.url), 'utf8'),
);
const probe = fileURLToPath(new URL('./module-probe.mjs', import.meta.url));
const protectedCandidate = [
  'README.md',
  'sentinel.txt',
  'src/cli.mjs',
  'test/log-summary.test.mjs',
  'data/events.jsonl',
];
const protectedNeighbor = ['README.md', 'keep.txt'];
const preservedOutput = Buffer.from(
  'M52 preexisting output must remain exact.\r\n',
);
const invalidDiagnostic = Buffer.from('Invalid log input or arguments.\n');

async function witness(file) {
  const link = await lstat(file);
  if (!link.isFile() || link.isSymbolicLink())
    throw new Error('protected file shape');
  const bytes = await readFile(file);
  const metadata = await stat(file, { bigint: true });
  return {
    sha256: createHash('sha256').update(bytes).digest('hex'),
    bytes: bytes.length,
    mtimeNs: metadata.mtimeNs.toString(),
  };
}

export async function captureProtected(candidateDirectory, neighborDirectory) {
  const roots = {
    candidate: path.resolve(candidateDirectory),
    neighbor:
      neighborDirectory === undefined ? null : path.resolve(neighborDirectory),
  };
  const entries = [];
  for (const [root, relativePaths] of [
    ['candidate', protectedCandidate],
    ['neighbor', roots.neighbor === null ? [] : protectedNeighbor],
  ]) {
    for (const relative of relativePaths)
      entries.push({
        root,
        relative,
        ...(await witness(path.join(roots[root], relative))),
      });
  }
  return { version: 1, roots, entries };
}

async function checkProtected(candidate, frozen, failures) {
  const reject = (reason) =>
    failures.push({ caseId: 'protected', boundary: 'files', reason });
  if (
    frozen?.version !== 1 ||
    frozen.roots?.candidate !== candidate ||
    !Array.isArray(frozen.entries)
  ) {
    reject('invalid-witness-manifest');
    return;
  }
  const required = protectedCandidate.map(
    (relative) => `candidate:${relative}`,
  );
  if (frozen.roots.neighbor !== null)
    required.push(
      ...protectedNeighbor.map((relative) => `neighbor:${relative}`),
    );
  const seen = new Set();
  for (const entry of frozen.entries) {
    const key = `${entry.root}:${entry.relative}`;
    if (
      !required.includes(key) ||
      seen.has(key) ||
      typeof frozen.roots[entry.root] !== 'string'
    ) {
      reject('unexpected-witness-path');
      continue;
    }
    seen.add(key);
    try {
      const current = await witness(
        path.join(frozen.roots[entry.root], entry.relative),
      );
      if (
        current.sha256 !== entry.sha256 ||
        current.bytes !== entry.bytes ||
        current.mtimeNs !== entry.mtimeNs
      )
        failures.push({
          caseId: key,
          boundary: 'files',
          reason: 'changed-protected-file',
        });
    } catch {
      failures.push({
        caseId: key,
        boundary: 'files',
        reason: 'missing-or-invalid-protected-file',
      });
    }
  }
  if (required.some((key) => !seen.has(key))) reject('missing-witness-path');
}

export async function verifyCandidate(candidateDirectory, suppliedWitnesses) {
  const candidate = path.resolve(candidateDirectory);
  const failures = [];
  const reject = (caseId, boundary, reason) =>
    failures.push({ caseId, boundary, reason });
  const frozen = suppliedWitnesses ?? (await captureProtected(candidate));
  const work = await mkdtemp(path.join(tmpdir(), 'anima-m52-proposal-oracle-'));
  const input = path.join(work, 'input.jsonl');
  const output = path.join(work, 'output.json');
  const environment = {};
  for (const name of ['SystemRoot', 'WINDIR', 'TEMP', 'TMP'])
    if (process.env[name] !== undefined) environment[name] = process.env[name];
  const invoke = (args) =>
    spawnSync(process.execPath, args, {
      cwd: candidate,
      env: environment,
      timeout: 2000,
      maxBuffer: 16_384,
      windowsHide: true,
    });
  const seedOutput = async () => {
    await writeFile(output, preservedOutput);
    const fixed = new Date('2001-01-01T00:00:00.000Z');
    await utimes(output, fixed, fixed);
    return witness(output);
  };
  const checkModule = (entry, valid) => {
    const result = invoke([
      probe,
      path.join(candidate, 'src/log-summary.mjs'),
      input,
    ]);
    if (
      result.error ||
      result.signal ||
      result.status !== 0 ||
      result.stderr?.length !== 0
    ) {
      reject(entry.id, 'module', 'probe-exit');
      return;
    }
    let value;
    try {
      value = JSON.parse(result.stdout.toString('utf8'));
    } catch {
      reject(entry.id, 'module', 'probe-protocol');
      return;
    }
    if (value.shape !== true)
      reject(entry.id, 'module', 'unexpected-exports-or-import');
    else if (valid && (value.threw !== false || value.value !== entry.expected))
      reject(entry.id, 'module', 'output-bytes');
    else if (!valid && value.threw !== true)
      reject(entry.id, 'module', 'accepted-invalid');
  };
  const checkCli = async (entry, valid, args, before) => {
    const result = invoke([path.join(candidate, 'src/cli.mjs'), ...args]);
    if (result.error || result.signal || result.status !== (valid ? 0 : 1))
      reject(entry.id, 'cli', 'exit-status');
    if (
      !Buffer.isBuffer(result.stdout) ||
      result.stdout.length !== 0 ||
      !Buffer.isBuffer(result.stderr) ||
      !result.stderr.equals(valid ? Buffer.alloc(0) : invalidDiagnostic)
    )
      reject(entry.id, 'cli', 'unexpected-diagnostic');
    try {
      const bytes = await readFile(output);
      if (
        !bytes.equals(
          valid ? Buffer.from(entry.expected, 'utf8') : preservedOutput,
        )
      )
        reject(
          entry.id,
          'cli',
          valid ? 'output-bytes' : 'changed-existing-output',
        );
      if (!valid && (await witness(output)).mtimeNs !== before.mtimeNs)
        reject(entry.id, 'cli', 'changed-existing-output-time');
    } catch {
      reject(entry.id, 'cli', 'missing-output');
    }
  };
  try {
    for (const [valid, cases] of [
      [true, manifest.valid],
      [false, manifest.invalid],
    ]) {
      for (const entry of cases) {
        await writeFile(input, entry.input, 'utf8');
        const before = await seedOutput();
        checkModule(entry, valid);
        await checkCli(
          entry,
          valid,
          ['--input', input, '--output', output],
          before,
        );
      }
    }
    for (const entry of manifest.invalidBytes) {
      await writeFile(input, Buffer.from(entry.hex, 'hex'));
      const before = await seedOutput();
      await checkCli(
        entry,
        false,
        ['--input', input, '--output', output],
        before,
      );
    }
    for (const entry of manifest.invalidArguments) {
      await writeFile(input, manifest.valid[0].input, 'utf8');
      const before = await seedOutput();
      const args = entry.args.map((arg) =>
        arg === 'INPUT' ? input : arg === 'OUTPUT' ? output : arg,
      );
      await checkCli(entry, false, args, before);
    }
  } finally {
    // Only the newly created exact oracle scratch directory is removed.
    await rm(work, { recursive: true, force: true });
    await checkProtected(candidate, frozen, failures);
  }
  return {
    passed: failures.length === 0,
    protectionScope:
      suppliedWitnesses === undefined
        ? 'oracle_execution_only'
        : 'supplied_witnesses',
    failures,
  };
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  if (process.argv.length < 3 || process.argv.length > 4) {
    process.stderr.write(
      'Usage: node oracle.mjs <absolute-candidate-root> [<frozen-witnesses.json>]\n',
    );
    process.exitCode = 2;
  } else {
    try {
      const frozen =
        process.argv[3] === undefined
          ? undefined
          : JSON.parse(await readFile(process.argv[3], 'utf8'));
      const verdict = await verifyCandidate(process.argv[2], frozen);
      process.stdout.write(JSON.stringify(verdict) + '\n');
      process.exitCode = verdict.passed ? 0 : 1;
    } catch {
      process.stderr.write('M52 oracle runner failed.\n');
      process.exitCode = 2;
    }
  }
}
