// Root-owned finite subprocess probe. Never copy into candidate authority.
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

let result = { shape: false };
try {
  if (process.argv.length !== 4) throw new Error('usage');
  const loaded = await import(pathToFileURL(process.argv[2]).href);
  if (
    Object.keys(loaded).join(',') !== 'summarizeLog' ||
    typeof loaded.summarizeLog !== 'function'
  )
    throw new Error('exports');
  const text = new TextDecoder('utf-8', {
    fatal: true,
    ignoreBOM: true,
  }).decode(readFileSync(process.argv[3]));
  try {
    result = { shape: true, threw: false, value: loaded.summarizeLog(text) };
  } catch {
    result = { shape: true, threw: true };
  }
} catch {
  // No candidate error text escapes into the controller protocol.
}
process.stdout.write(JSON.stringify(result) + '\n');
