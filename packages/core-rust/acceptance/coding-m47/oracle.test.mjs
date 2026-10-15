import assert from 'node:assert/strict';
import { cp, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { verifyCandidate } from './oracle.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const reference = await readFile(
  new URL('./reference/invoice-summary.mjs', import.meta.url),
  'utf8',
);
const originalCli = await readFile(
  new URL('./fixture/src/cli.mjs', import.meta.url),
  'utf8',
);
const expected = JSON.parse(
  await readFile(new URL('./expected-manifest.json', import.meta.url), 'utf8'),
);

async function candidate(t, source) {
  const work = await mkdtemp(path.join(tmpdir(), 'anima-m47-selftest-'));
  t.after(() => rm(work, { recursive: true, force: true }));
  const directory = path.join(work, 'candidate');
  await cp(path.join(here, 'fixture'), directory, { recursive: true });
  if (source !== undefined)
    await writeFile(path.join(directory, 'src/invoice-summary.mjs'), source);
  return directory;
}

test('m47_coding_oracle_has_literal_primary_changed_unicode_and_large_expected_bytes', () => {
  assert.equal(
    expected.valid.find((entry) => entry.id === 'primary').expected,
    'customer,open_count,total\nCedar,3,20.00\nOrchid,2,0.30\n',
  );
  assert.equal(
    expected.valid.find((entry) => entry.id === 'changed-amount').expected,
    'customer,open_count,total\nCedar,3,21.00\nOrchid,2,0.30\n',
  );
  assert.equal(
    expected.valid.find((entry) => entry.id === 'large-exact-cents').expected,
    'customer,open_count,total\nLarge,2,123456789012345678901234567891.00\n',
  );
  assert.ok(
    expected.valid
      .find((entry) => entry.id === 'unicode-codepoint-order')
      .expected.includes('\n\ue000,1,0.06\n𐀀,1,0.07\n'),
  );
  assert.equal(
    new Set([...expected.valid, ...expected.invalid].map((entry) => entry.id))
      .size,
    expected.valid.length + expected.invalid.length,
  );
});

test(
  'm47_coding_oracle_rejects_buggy_baseline_at_module_and_actual_cli_boundaries',
  { timeout: 20000 },
  async (t) => {
    const directory = await candidate(t);
    const verdict = await verifyCandidate(directory);
    assert.equal(verdict.passed, false);
    for (const boundary of ['module', 'cli'])
      assert.ok(
        verdict.failures.some(
          (entry) =>
            entry.caseId === 'primary' &&
            entry.boundary === boundary &&
            entry.reason === 'output-bytes',
        ),
      );
  },
);

test(
  'm47_coding_oracle_accepts_known_correct_reference_without_changing_protected_candidate_files',
  { timeout: 20000 },
  async (t) => {
    const directory = await candidate(t, reference);
    const protectedFiles = [
      'src/cli.mjs',
      'test/invoice-summary.test.mjs',
      'data/invoices.csv',
      'README.md',
      'sentinel.txt',
    ];
    const before = await Promise.all(
      protectedFiles.map((name) => readFile(path.join(directory, name))),
    );
    assert.deepEqual(await verifyCandidate(directory), {
      passed: true,
      failures: [],
    });
    for (let index = 0; index < protectedFiles.length; index++)
      assert.deepEqual(
        await readFile(path.join(directory, protectedFiles[index])),
        before[index],
      );
  },
);

for (const mutant of [
  {
    id: 'decimal-truncation',
    before: 'const cents = BigInt(whole) * 100n + BigInt(fraction);',
    after: 'const cents = BigInt(whole) * 100n;',
    caseId: 'primary',
  },
  {
    id: 'void-inclusion',
    before: "if (status === 'void') continue;",
    after: 'if (false) continue;',
    caseId: 'all-void',
  },
  {
    id: 'utf16-sort',
    before: 'return compareCodePoints(left, right);',
    after: 'return left < right ? -1 : left > right ? 1 : 0;',
    caseId: 'unicode-codepoint-order',
  },
]) {
  test(
    `m47_coding_oracle_rejects_${mutant.id}_mutant_for_its_semantic_reason`,
    { timeout: 20000 },
    async (t) => {
      assert.equal(
        reference.split(mutant.before).length,
        2,
        'mutation must match exactly once',
      );
      const directory = await candidate(
        t,
        reference.replace(mutant.before, mutant.after),
      );
      const verdict = await verifyCandidate(directory);
      assert.equal(verdict.passed, false);
      assert.ok(
        verdict.failures.some(
          (entry) =>
            entry.caseId === mutant.caseId &&
            entry.boundary === 'module' &&
            entry.reason === 'output-bytes',
        ),
      );
      assert.equal(
        verdict.failures.some(
          (entry) =>
            entry.reason === 'import-failed' || entry.reason === 'exit-status',
        ),
        false,
      );
    },
  );
}

test(
  'm47_coding_oracle_rejects_cli_prevalidation_truncation_while_correct_module_still_passes',
  { timeout: 20000 },
  async (t) => {
    const directory = await candidate(t, reference);
    const marker = 'const result = summarizeInvoices(csvText);';
    assert.equal(
      originalCli.split(marker).length,
      2,
      'mutation must match exactly once',
    );
    await writeFile(
      path.join(directory, 'src/cli.mjs'),
      originalCli.replace(
        marker,
        "await writeFile(output, '', 'utf8');\n  " + marker,
      ),
    );
    const verdict = await verifyCandidate(directory);
    assert.equal(verdict.passed, false);
    assert.equal(
      verdict.failures.some((entry) => entry.boundary === 'module'),
      false,
    );
    assert.ok(
      verdict.failures.some(
        (entry) =>
          entry.caseId === 'invalid-row-after-valid' &&
          entry.boundary === 'cli' &&
          entry.reason === 'changed-existing-output',
      ),
    );
  },
);
