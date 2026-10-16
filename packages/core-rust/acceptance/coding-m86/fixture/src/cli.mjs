import { readFile, writeFile } from 'node:fs/promises';
import { compareSnapshots } from './compare-snapshots.mjs';

try {
  if (process.argv.length !== 2) throw new Error('Unexpected arguments');
  const before = JSON.parse(
    await readFile(new URL('../data/before.json', import.meta.url), 'utf8'),
  );
  const after = JSON.parse(
    await readFile(new URL('../data/after.json', import.meta.url), 'utf8'),
  );
  const changes = compareSnapshots(before, after);
  await writeFile(
    new URL('../changes.json', import.meta.url),
    `${JSON.stringify(changes)}\n`,
    'utf8',
  );
} catch {
  process.stderr.write('Snapshot comparison failed.\n');
  process.exitCode = 1;
}
