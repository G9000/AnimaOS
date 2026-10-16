import { pathToFileURL } from 'node:url';

let input = '';
for await (const chunk of process.stdin) input += chunk;
const { records } = JSON.parse(input);
const original = JSON.stringify(records);
try {
  const module = await import(pathToFileURL(process.argv[2]).href);
  if (
    Object.keys(module).join(',') !== 'findDuplicates' ||
    typeof module.findDuplicates !== 'function'
  )
    throw new Error('Unexpected module exports');
  try {
    const value = module.findDuplicates(records);
    process.stdout.write(
      JSON.stringify({
        kind: 'return',
        value,
        mutated: JSON.stringify(records) !== original,
      }) + '\n',
    );
  } catch (error) {
    const message = String(error?.message ?? '');
    process.stdout.write(
      JSON.stringify({
        kind: 'throw',
        name: error?.name,
        message: message.slice(0, 200),
        messageTruncated: message.length > 200,
        mutated: JSON.stringify(records) !== original,
      }) + '\n',
    );
  }
} catch {
  process.stdout.write(JSON.stringify({ kind: 'import-error' }) + '\n');
}
