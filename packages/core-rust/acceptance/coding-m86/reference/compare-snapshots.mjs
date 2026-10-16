const pathPattern =
  /^[A-Za-z0-9][A-Za-z0-9._-]*(?:\/[A-Za-z0-9][A-Za-z0-9._-]*)*$/;

function records(snapshot) {
  if (!Array.isArray(snapshot) || snapshot.length > 128)
    throw new Error('Invalid snapshot');
  const byPath = new Map();
  for (const record of snapshot) {
    if (
      record === null ||
      typeof record !== 'object' ||
      Array.isArray(record) ||
      Object.keys(record).sort().join(',') !== 'bytes,path,sha256' ||
      typeof record.path !== 'string' ||
      record.path.length > 128 ||
      !pathPattern.test(record.path) ||
      byPath.has(record.path) ||
      !Number.isSafeInteger(record.bytes) ||
      record.bytes < 0 ||
      record.bytes > 1048576 ||
      typeof record.sha256 !== 'string' ||
      !/^[0-9a-f]{64}$/.test(record.sha256)
    )
      throw new Error('Invalid snapshot record');
    byPath.set(record.path, record);
  }
  return byPath;
}

export function compareSnapshots(before, after) {
  const oldFiles = records(before);
  const newFiles = records(after);
  const added = [];
  const removed = [];
  const changed = [];
  for (const [path, current] of newFiles) {
    const previous = oldFiles.get(path);
    if (!previous) added.push(path);
    else if (
      previous.bytes !== current.bytes ||
      previous.sha256 !== current.sha256
    )
      changed.push(path);
  }
  for (const path of oldFiles.keys())
    if (!newFiles.has(path)) removed.push(path);
  return {
    added: added.sort(),
    removed: removed.sort(),
    changed: changed.sort(),
  };
}
