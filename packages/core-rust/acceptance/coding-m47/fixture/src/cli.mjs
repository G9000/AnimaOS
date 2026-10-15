import { readFile, writeFile } from 'node:fs/promises';
import { summarizeInvoices } from './invoice-summary.mjs';

try {
  const args = process.argv.slice(2);
  if (
    args.length !== 4 ||
    args[0] !== '--input' ||
    args[2] !== '--output' ||
    !args[1] ||
    !args[3]
  )
    throw new Error('Invalid arguments');
  const [input, output] = [args[1], args[3]];
  const bytes = await readFile(input);
  if (bytes.length > 16384) throw new Error('Input too large');
  const csvText = new TextDecoder('utf-8', {
    fatal: true,
    ignoreBOM: true,
  }).decode(bytes);
  const result = summarizeInvoices(csvText);
  // Fully validate before opening/replacing any existing output.
  await writeFile(output, result, 'utf8');
} catch {
  process.stderr.write('Invalid invoice input or arguments.\n');
  process.exitCode = 1;
}
