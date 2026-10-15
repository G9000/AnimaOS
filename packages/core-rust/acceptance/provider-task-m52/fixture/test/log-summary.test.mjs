import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { summarizeLog } from '../src/log-summary.mjs';

test('summarizes all events including errors and accumulates durations', () => {
  const text = readFileSync(
    new URL('../data/events.jsonl', import.meta.url),
    'utf8',
  );
  assert.equal(
    summarizeLog(text),
    '{"services":[{"service":"api","count":3,"errors":1,"durationMs":210},{"service":"worker","count":2,"errors":1,"durationMs":15}]}\n',
  );
});
test('blank input has no services', () => {
  assert.equal(summarizeLog(' \r\n\n'), '{"services":[]}\n');
});
test('invalid duration is rejected', () => {
  assert.throws(() =>
    summarizeLog('{"service":"api","status":"ok","durationMs":"4"}\n'),
  );
});
