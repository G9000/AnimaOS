// External controller tooling: never put this file or its manifest in a candidate copy.
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const manifest = JSON.parse(
  await readFile(new URL('./expected-manifest.json', import.meta.url), 'utf8'),
);
const preservedOutput = Buffer.from(
  'M47 preexisting output must remain exact.\r\n',
);
const invalidDiagnostic = Buffer.from('Invalid invoice input or arguments.\n');

export async function verifyCandidate(candidateDirectory) {
  const candidate = path.resolve(candidateDirectory);
  const failures = [];
  const reject = (caseId, boundary, reason) =>
    failures.push({ caseId, boundary, reason });
  let summarize;
  try {
    const module = await import(
      pathToFileURL(path.join(candidate, 'src/invoice-summary.mjs')).href
    );
    if (
      Object.keys(module).join(',') !== 'summarizeInvoices' ||
      typeof module.summarizeInvoices !== 'function'
    )
      reject('interface', 'module', 'unexpected-exports');
    else summarize = module.summarizeInvoices;
  } catch {
    reject('interface', 'module', 'import-failed');
  }

  const work = await mkdtemp(path.join(tmpdir(), 'anima-m47-oracle-'));
  const input = path.join(work, 'input.csv');
  const output = path.join(work, 'output.csv');
  const cli = path.join(candidate, 'src/cli.mjs');
  const environment = {};
  for (const name of ['SystemRoot', 'WINDIR', 'TEMP', 'TMP'])
    if (process.env[name] !== undefined) environment[name] = process.env[name];
  const invoke = (args) =>
    spawnSync(process.execPath, [cli, ...args], {
      cwd: candidate,
      env: environment,
      timeout: 2000,
      maxBuffer: 16384,
      windowsHide: true,
    });
  const check = (result, valid, caseId) => {
    if (result.error || result.signal || result.status !== (valid ? 0 : 1))
      reject(caseId, 'cli', 'exit-status');
    if (
      !Buffer.isBuffer(result.stdout) ||
      result.stdout.length !== 0 ||
      !Buffer.isBuffer(result.stderr) ||
      !result.stderr.equals(valid ? Buffer.alloc(0) : invalidDiagnostic)
    )
      reject(caseId, 'cli', 'unexpected-diagnostic');
  };
  try {
    for (const entry of manifest.valid) {
      if (summarize) {
        try {
          if (summarize(entry.input) !== entry.expected)
            reject(entry.id, 'module', 'output-bytes');
        } catch {
          reject(entry.id, 'module', 'unexpected-throw');
        }
      }
      await writeFile(input, entry.input, 'utf8');
      await writeFile(output, preservedOutput);
      check(invoke(['--input', input, '--output', output]), true, entry.id);
      if (!(await readFile(output)).equals(Buffer.from(entry.expected, 'utf8')))
        reject(entry.id, 'cli', 'output-bytes');
    }
    for (const entry of manifest.invalid) {
      if (summarize) {
        let threw = false;
        try {
          summarize(entry.input);
        } catch {
          threw = true;
        }
        if (!threw) reject(entry.id, 'module', 'accepted-invalid');
      }
      await writeFile(input, entry.input, 'utf8');
      await writeFile(output, preservedOutput);
      check(invoke(['--input', input, '--output', output]), false, entry.id);
      if (!(await readFile(output)).equals(preservedOutput))
        reject(entry.id, 'cli', 'changed-existing-output');
    }
    for (const [id, args] of [
      ['missing-arguments', []],
      ['unknown-option', ['--source', input, '--output', output]],
      ['duplicate-option', ['--input', input, '--input', output]],
      ['extra-argument', ['--input', input, '--output', output, 'extra']],
    ]) {
      await writeFile(output, preservedOutput);
      check(invoke(args), false, id);
      if (!(await readFile(output)).equals(preservedOutput))
        reject(id, 'cli', 'changed-existing-output');
    }
  } finally {
    await rm(work, { recursive: true, force: true });
  }
  return { passed: failures.length === 0, failures };
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  if (process.argv.length !== 3) {
    process.stderr.write(
      'Usage: node oracle.mjs <frozen-candidate-directory>\n',
    );
    process.exitCode = 2;
  } else {
    try {
      const verdict = await verifyCandidate(process.argv[2]);
      process.stdout.write(JSON.stringify(verdict) + '\n');
      process.exitCode = verdict.passed ? 0 : 1;
    } catch {
      process.stderr.write('M47 oracle could not complete.\n');
      process.exitCode = 2;
    }
  }
}
