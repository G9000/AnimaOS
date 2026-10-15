import { readFileSync, writeFileSync } from 'node:fs';
import { summarizeLog } from './log-summary.mjs';

try {
  const args = process.argv.slice(2);
  if (args.length !== 4) throw new Error('arguments');
  const values = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    if (
      !['--input', '--output'].includes(key) ||
      values.has(key) ||
      !args[index + 1]
    )
      throw new Error('arguments');
    values.set(key, args[index + 1]);
  }
  if (values.size !== 2) throw new Error('arguments');
  const input = values.get('--input');
  const output = values.get('--output');
  const bytes = readFileSync(input);
  if (bytes.length > 16_384) throw new Error('input size');
  const text = new TextDecoder('utf-8', {
    fatal: true,
    ignoreBOM: true,
  }).decode(bytes);
  const result = summarizeLog(text);
  writeFileSync(output, result, 'utf8');
} catch {
  process.stderr.write('Invalid log input or arguments.\n');
  process.exitCode = 1;
}
