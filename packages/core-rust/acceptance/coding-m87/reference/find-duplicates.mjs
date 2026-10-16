const pathPattern =
  /^[A-Za-z0-9][A-Za-z0-9._-]*(?:\/[A-Za-z0-9][A-Za-z0-9._-]*)*$/;

function validated(records) {
  if (!Array.isArray(records) || records.length > 128)
    throw new Error('Invalid inventory');
  const paths = new Set();
  for (const record of records) {
    if (
      record === null ||
      typeof record !== 'object' ||
      Array.isArray(record) ||
      Object.keys(record).sort().join(',') !== 'bytes,path,sha256' ||
      typeof record.path !== 'string' ||
      record.path.length > 128 ||
      !pathPattern.test(record.path) ||
      paths.has(record.path) ||
      !Number.isSafeInteger(record.bytes) ||
      record.bytes < 0 ||
      record.bytes > 1048576 ||
      typeof record.sha256 !== 'string' ||
      !/^[0-9a-f]{64}$/.test(record.sha256)
    )
      throw new Error('Invalid inventory record');
    paths.add(record.path);
  }
}

export function findDuplicates(records) {
  validated(records);
  const byIdentity = new Map();
  for (const record of records) {
    const key = `${record.bytes}:${record.sha256}`;
    const group = byIdentity.get(key) ?? [];
    group.push(record);
    byIdentity.set(key, group);
  }
  const groups = [];
  let redundantBytes = 0;
  for (const records of byIdentity.values()) {
    if (records.length < 2) continue;
    const paths = records.map((record) => record.path).sort();
    const bytes = records[0].bytes;
    groups.push({ paths, bytes, sha256: records[0].sha256 });
    redundantBytes += (records.length - 1) * bytes;
  }
  groups.sort((left, right) =>
    left.paths[0] < right.paths[0]
      ? -1
      : left.paths[0] > right.paths[0]
        ? 1
        : 0,
  );
  return { groups, redundantBytes };
}
