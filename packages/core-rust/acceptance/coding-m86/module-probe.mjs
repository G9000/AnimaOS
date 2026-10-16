import { pathToFileURL } from 'node:url';

let input = '';
for await (const chunk of process.stdin) input += chunk;
const { before, after } = JSON.parse(input);
const original = JSON.stringify({ before, after });
try {
  const module = await import(pathToFileURL(process.argv[2]).href);
  if (
    Object.keys(module).join(',') !== 'compareSnapshots' ||
    typeof module.compareSnapshots !== 'function'
  )
    throw new Error('Unexpected module exports');
  try {
    const value = module.compareSnapshots(before, after);
    process.stdout.write(
      JSON.stringify({
        kind: 'return',
        value,
        mutated: JSON.stringify({ before, after }) !== original,
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
        mutated: JSON.stringify({ before, after }) !== original,
      }) + '\n',
    );
  }
} catch {
  process.stdout.write(JSON.stringify({ kind: 'import-error' }) + '\n');
}
