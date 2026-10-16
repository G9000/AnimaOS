import { readFile, writeFile } from 'node:fs/promises';
import { findDuplicates } from './find-duplicates.mjs';

try {
  if (process.argv.length !== 2) throw new Error('Unexpected arguments');
  const records = JSON.parse(
    await readFile(new URL('../data/inventory.json', import.meta.url), 'utf8'),
  );
  const duplicates = findDuplicates(records);
  await writeFile(
    new URL('../duplicates.json', import.meta.url),
    `${JSON.stringify(duplicates)}\n`,
    'utf8',
  );
} catch {
  process.stderr.write('Duplicate inventory failed.\n');
  process.exitCode = 1;
}
