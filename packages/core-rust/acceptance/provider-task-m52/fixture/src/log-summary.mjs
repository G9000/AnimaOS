export function summarizeLog(jsonlText) {
  if (
    typeof jsonlText !== 'string' ||
    Buffer.byteLength(jsonlText, 'utf8') > 16_384
  )
    throw new Error('Invalid log input.');
  const rows = jsonlText
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  if (rows.length > 128) throw new Error('Invalid log input.');
  const services = new Map();
  for (const line of rows) {
    const event = JSON.parse(line);
    if (
      !event ||
      Array.isArray(event) ||
      typeof event !== 'object' ||
      typeof event.service !== 'string' ||
      !/^[A-Za-z0-9][A-Za-z0-9._-]{0,39}(?![\s\S])/.test(event.service) ||
      !['ok', 'error'].includes(event.status) ||
      !Number.isInteger(event.durationMs) ||
      event.durationMs < 0 ||
      event.durationMs > 1_000_000
    )
      throw new Error('Invalid log input.');
    // Deliberate repair task: both defects below must be found independently.
    if (event.status === 'error') continue;
    const group = services.get(event.service) ?? {
      service: event.service,
      count: 0,
      errors: 0,
      durationMs: 0,
    };
    group.count += 1;
    if (event.status === 'error') group.errors += 1;
    group.durationMs = event.durationMs;
    services.set(event.service, group);
  }
  return (
    JSON.stringify({
      services: [...services.keys()].sort().map((id) => services.get(id)),
    }) + '\n'
  );
}
