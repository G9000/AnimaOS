import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('.', import.meta.url));
const childEnv = {};
for (const name of ['SystemRoot', 'WINDIR', 'TEMP', 'TMP'])
  if (process.env[name] !== undefined) childEnv[name] = process.env[name];
const run = (args) =>
  spawnSync(process.execPath, args, {
    cwd: root,
    env: childEnv,
    encoding: 'utf8',
    timeout: 8000,
    maxBuffer: 16384,
    windowsHide: true,
  });
const tests = run([
  '--test',
  '--test-reporter=tap',
  'test/compare-snapshots.test.mjs',
]);
const count = (label) =>
  Number(new RegExp(`^# ${label} (\\d+)$`, 'm').exec(tests.stdout ?? '')?.[1]);
const summary = {
  testExitCode: tests.status,
  visibleTests: count('tests'),
  passed: count('pass'),
  failed: count('fail'),
  artifactWritten: false,
};
if (
  tests.error ||
  tests.signal ||
  tests.status !== 0 ||
  summary.visibleTests !== 6 ||
  summary.passed !== 6 ||
  summary.failed !== 0
) {
  process.stdout.write(`${JSON.stringify(summary)}\n`);
  process.exitCode = 1;
} else {
  const cli = run(['src/cli.mjs']);
  if (cli.error || cli.signal || cli.status !== 0) {
    process.stdout.write(
      `${JSON.stringify({ ...summary, cliSucceeded: false })}\n`,
    );
    process.exitCode = 1;
  } else {
    const bytes = await readFile(new URL('./changes.json', import.meta.url));
    const changes = JSON.parse(bytes.toString('utf8'));
    process.stdout.write(
      `${JSON.stringify({
        ...summary,
        artifactWritten: true,
        counts: {
          added: changes.added.length,
          removed: changes.removed.length,
          changed: changes.changed.length,
        },
        artifactBytes: bytes.length,
        artifactSha256: createHash('sha256').update(bytes).digest('hex'),
      })}\n`,
    );
  }
}
