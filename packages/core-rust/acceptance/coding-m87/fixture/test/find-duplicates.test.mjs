import assert from 'node:assert/strict';
import test from 'node:test';
import { findDuplicates } from '../src/find-duplicates.mjs';

const hash = (letter) => letter.repeat(64);
const row = (path, bytes, letter) => ({ path, bytes, sha256: hash(letter) });

test('same size with distinct hashes is not a duplicate group', () => {
  assert.deepEqual(
    findDuplicates([row('a.txt', 16, 'a'), row('b.txt', 16, 'b')]),
    { groups: [], redundantBytes: 0 },
  );
});
test('a same-size different-hash singleton stays outside the duplicate group', () => {
  assert.deepEqual(
    findDuplicates([
      row('a.txt', 16, 'a'),
      row('b.txt', 16, 'a'),
      row('lonely.txt', 16, 'c'),
    ]),
    {
      groups: [{ paths: ['a.txt', 'b.txt'], bytes: 16, sha256: hash('a') }],
      redundantBytes: 16,
    },
  );
});
test('a three-file group counts two redundant nonzero copies', () => {
  assert.deepEqual(
    findDuplicates([
      row('a.txt', 7, 'a'),
      row('b.txt', 7, 'a'),
      row('c.txt', 7, 'a'),
    ]),
    {
      groups: [
        { paths: ['a.txt', 'b.txt', 'c.txt'], bytes: 7, sha256: hash('a') },
      ],
      redundantBytes: 14,
    },
  );
});
test('zero-byte groups remain visible and sort without mutating input', () => {
  const input = [
    row('z.txt', 0, 'b'),
    row('b.txt', 0, 'b'),
    row('unique.txt', 1, 'a'),
  ];
  const frozen = structuredClone(input);
  assert.deepEqual(findDuplicates(input), {
    groups: [{ paths: ['b.txt', 'z.txt'], bytes: 0, sha256: hash('b') }],
    redundantBytes: 0,
  });
  assert.deepEqual(input, frozen);
});
test('empty, singleton and distinct-size inputs have no duplicate groups', () => {
  assert.deepEqual(findDuplicates([]), { groups: [], redundantBytes: 0 });
  assert.deepEqual(findDuplicates([row('only.txt', 3, 'a')]), {
    groups: [],
    redundantBytes: 0,
  });
  assert.deepEqual(
    findDuplicates([row('a.txt', 3, 'a'), row('b.txt', 4, 'a')]),
    { groups: [], redundantBytes: 0 },
  );
});
test('duplicate or unsafe paths and malformed records refuse', () => {
  for (const records of [
    [row('same.txt', 1, 'a'), row('same.txt', 1, 'a')],
    [row('../escape', 1, 'a')],
    [row('x\\y', 1, 'a')],
    [row('bad.txt', -1, 'a')],
    [{ ...row('bad.txt', 1, 'a'), sha256: 'A'.repeat(64) }],
  ])
    assert.throws(() => findDuplicates(records), Error);
});
