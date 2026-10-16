import assert from 'node:assert/strict';
import test from 'node:test';
import { compareSnapshots } from '../src/compare-snapshots.mjs';

const hash = (letter) => letter.repeat(64);
const row = (path, bytes, letter) => ({ path, bytes, sha256: hash(letter) });

test('reports paths removed from the after snapshot', () => {
  assert.deepEqual(compareSnapshots([row('old.txt', 10, 'a')], []), {
    added: [],
    removed: ['old.txt'],
    changed: [],
  });
});
test('reports same-size content hash changes', () => {
  assert.deepEqual(
    compareSnapshots([row('same.txt', 20, 'b')], [row('same.txt', 20, 'e')]),
    { added: [], removed: [], changed: ['same.txt'] },
  );
});
test('reports additions and size changes', () => {
  assert.deepEqual(
    compareSnapshots(
      [row('size.txt', 30, 'c')],
      [row('new.txt', 5, 'f'), row('size.txt', 31, 'c')],
    ),
    { added: ['new.txt'], removed: [], changed: ['size.txt'] },
  );
});
test('omits unchanged entries and accepts empty snapshots', () => {
  assert.deepEqual(compareSnapshots([], []), {
    added: [],
    removed: [],
    changed: [],
  });
  assert.deepEqual(
    compareSnapshots(
      [row('stable.txt', 40, 'd')],
      [row('stable.txt', 40, 'd')],
    ),
    { added: [], removed: [], changed: [] },
  );
});
test('sorts ASCII paths without changing either input', () => {
  const before = [row('z.txt', 1, 'a'), row('A.txt', 2, 'b')];
  const after = [row('m.txt', 3, 'c'), row('b.txt', 4, 'd')];
  const frozenBefore = structuredClone(before);
  const frozenAfter = structuredClone(after);
  assert.deepEqual(compareSnapshots(before, after), {
    added: ['b.txt', 'm.txt'],
    removed: ['A.txt', 'z.txt'],
    changed: [],
  });
  assert.deepEqual(before, frozenBefore);
  assert.deepEqual(after, frozenAfter);
});
test('rejects malformed records, unsafe paths and duplicates', () => {
  for (const bad of [
    [row('../escape', 1, 'a')],
    [row('x\\y', 1, 'a')],
    [row('same.txt', -1, 'a')],
    [row('same.txt', 1.5, 'a')],
    [{ ...row('same.txt', 1, 'a'), sha256: 'A'.repeat(64) }],
    [row('dup.txt', 1, 'a'), row('dup.txt', 2, 'b')],
  ])
    assert.throws(() => compareSnapshots(bad, []), Error);
});
