// Deterministic controls for the buggy baseline, reference and semantic mutants.
import assert from 'node:assert/strict';
import { cp, mkdtemp, readFile, rm, utimes, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { captureProtected, verifyCandidate } from './oracle.mjs';

const template = fileURLToPath(new URL('./fixture/', import.meta.url));
const neighborTemplate = fileURLToPath(new URL('./neighbor/', import.meta.url));
const reference = await readFile(
  new URL('./reference/log-summary.mjs', import.meta.url),
  'utf8',
);
const literalPrimary =
  '{"services":[{"service":"api","count":3,"errors":1,"durationMs":210},{"service":"worker","count":2,"errors":1,"durationMs":15}]}\n';

function replaceExactly(text, before, after) {
  assert.equal(
    text.split(before).length,
    2,
    'one explicit mutant anchor must exist',
  );
  return text.replace(before, after);
}
async function withCandidate(body, useReference = false) {
  const privateRoot = await mkdtemp(
    path.join(tmpdir(), 'anima-m52-proposal-test-'),
  );
  const candidate = path.join(privateRoot, 'candidate');
  const neighbor = path.join(privateRoot, 'neighbor');
  try {
    await cp(template, candidate, { recursive: true });
    await cp(neighborTemplate, neighbor, { recursive: true });
    if (useReference)
      await writeFile(
        path.join(candidate, 'src/log-summary.mjs'),
        reference,
        'utf8',
      );
    await body(candidate, neighbor);
  } finally {
    // This exact fresh test root is owned by this test, never a computed ancestor.
    await rm(privateRoot, { recursive: true, force: true });
  }
}
function semanticFailure(verdict, caseId, boundary, reason) {
  assert.equal(verdict.passed, false);
  assert.ok(
    verdict.failures.some(
      (entry) =>
        entry.caseId === caseId &&
        entry.boundary === boundary &&
        entry.reason === reason,
    ),
    `literal ${caseId}/${boundary}/${reason} must reject the mutant`,
  );
}

test('buggy baseline fails the literal primary module and CLI outputs', async () => {
  await withCandidate(async (candidate) => {
    const result = await verifyCandidate(candidate);
    semanticFailure(result, 'primary', 'module', 'output-bytes');
    semanticFailure(result, 'primary', 'cli', 'output-bytes');
  });
});

test('independent reference satisfies the frozen literal module/CLI cases and protected witnesses', async () => {
  await withCandidate(async (candidate, neighbor) => {
    const frozen = await captureProtected(candidate, neighbor);
    const result = await verifyCandidate(candidate, frozen);
    assert.deepEqual(result, {
      passed: true,
      protectionScope: 'supplied_witnesses',
      failures: [],
    });
  }, true);
});

const sourceMutants = [
  {
    id: 'skip-errors',
    caseId: 'primary',
    source: replaceExactly(
      reference,
      '// MUTANT: skip-errors insertion point.',
      "if (event.status === 'error') continue;",
    ),
  },
  {
    id: 'overwrite-total',
    caseId: 'primary',
    source: replaceExactly(
      reference,
      'group.durationMs += event.durationMs;',
      'group.durationMs = event.durationMs;',
    ),
  },
  {
    id: 'zero-errors',
    caseId: 'primary',
    source: replaceExactly(
      reference,
      "if (event.status === 'error') group.errors += 1;",
      'if (false) group.errors += 1;',
    ),
  },
  {
    id: 'unsorted-rows',
    caseId: 'reversed',
    source: replaceExactly(
      reference,
      'const names = Object.keys(groups).sort();',
      'const names = Object.keys(groups);',
    ),
  },
  {
    id: 'hardcoded-primary',
    caseId: 'empty',
    source: `export function summarizeLog() { return ${JSON.stringify(literalPrimary)}; }\n`,
  },
];

for (const mutant of sourceMutants) {
  test(`semantic mutant ${mutant.id} fails a predeclared literal case`, async () => {
    await withCandidate(async (candidate) => {
      await writeFile(
        path.join(candidate, 'src/log-summary.mjs'),
        mutant.source,
        'utf8',
      );
      const result = await verifyCandidate(candidate);
      semanticFailure(result, mutant.caseId, 'module', 'output-bytes');
      semanticFailure(result, mutant.caseId, 'cli', 'output-bytes');
    }, true);
  });
}

test('semantic mutant prevalidation-output-truncation fails invalid output preservation', async () => {
  await withCandidate(async (candidate) => {
    const cli = path.join(candidate, 'src/cli.mjs');
    const source = await readFile(cli, 'utf8');
    await writeFile(
      cli,
      replaceExactly(
        source,
        'const result = summarizeLog(text);',
        "writeFileSync(output, '', 'utf8');\n  const result = summarizeLog(text);",
      ),
      'utf8',
    );
    const result = await verifyCandidate(candidate);
    semanticFailure(result, 'invalid-json', 'cli', 'changed-existing-output');
    semanticFailure(
      result,
      'invalid-json',
      'cli',
      'changed-existing-output-time',
    );
  }, true);
});

test('supplied witnesses reject unchanged bytes with a changed protected modification time', async () => {
  await withCandidate(async (candidate, neighbor) => {
    const frozen = await captureProtected(candidate, neighbor);
    await utimes(
      path.join(neighbor, 'keep.txt'),
      new Date('2002-01-01'),
      new Date('2002-01-01'),
    );
    const result = await verifyCandidate(candidate, frozen);
    semanticFailure(
      result,
      'neighbor:keep.txt',
      'files',
      'changed-protected-file',
    );
  }, true);
});
