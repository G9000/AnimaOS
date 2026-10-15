// Independent positive control; expected answers live only in the literal manifest.
export function summarizeLog(jsonlText) {
  if (typeof jsonlText !== 'string' || Buffer.byteLength(jsonlText) > 16_384)
    throw new Error('invalid');
  const groups = Object.create(null);
  let count = 0;
  for (const raw of jsonlText.split('\n')) {
    const record = raw.trim();
    if (record === '') continue;
    count += 1;
    if (count > 128) throw new Error('invalid');
    const event = JSON.parse(record);
    if (event === null || typeof event !== 'object' || Array.isArray(event))
      throw new Error('invalid');
    const service = event.service;
    if (
      typeof service !== 'string' ||
      service.length < 1 ||
      service.length > 40
    )
      throw new Error('invalid');
    for (let index = 0; index < service.length; index += 1) {
      const character = service[index];
      const alphanumeric =
        (character >= 'A' && character <= 'Z') ||
        (character >= 'a' && character <= 'z') ||
        (character >= '0' && character <= '9');
      if (!alphanumeric && (index === 0 || !'._-'.includes(character)))
        throw new Error('invalid');
    }
    if (event.status !== 'ok' && event.status !== 'error')
      throw new Error('invalid');
    if (
      !Number.isInteger(event.durationMs) ||
      event.durationMs < 0 ||
      event.durationMs > 1_000_000
    )
      throw new Error('invalid');
    // MUTANT: skip-errors insertion point.
    if (!Object.hasOwn(groups, service))
      groups[service] = { service, count: 0, errors: 0, durationMs: 0 };
    const group = groups[service];
    group.count += 1;
    if (event.status === 'error') group.errors += 1;
    group.durationMs += event.durationMs;
  }
  const names = Object.keys(groups).sort();
  return JSON.stringify({ services: names.map((name) => groups[name]) }) + '\n';
}
