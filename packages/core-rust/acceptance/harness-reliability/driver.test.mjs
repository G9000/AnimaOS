import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createPlan, executePlan, summarize, runProcess, validateArtifact } from './live-driver.mjs';

const fixtures = ['ambiguous-edit', 'stale-read', 'large-output', 'repeated-failure', 'unrelated-green-check', 'fresh-skill-reuse'];
const build = (hash) => ({ binary: process.execPath, binarySha256: hash.repeat(64), sourceSnapshotSha256: 'c'.repeat(64), sourceCommit: 'd'.repeat(40) });
const options = { fixtures, builds: { baseline: build('a'), candidate: build('b') }, model: 'explicit-test-model' };
const complete = (row) => ({ fixtureId: row.fixtureId, repetition: row.repetition, variant: row.variant, oracleFailures: [], traceComplete: true });

test('the default plan retains every one of the 24 matched trials', () => {
  const plan = createPlan(options);
  assert.equal(plan.rows.length, 24);
  assert.equal(new Set(plan.rows.map((r) => r.id)).size, 24);
  assert.ok(plan.rows.every((r) => r.status === 'planned'));
  assert.equal(summarize(plan).complete, false);
});

test('a timeout leaves all subsequent rows planned and makes the comparison incomplete', async () => {
  const root = await mkdtemp(join(tmpdir(), 'anima-trace-driver-'));
  try {
    let calls = 0;
    const plan = createPlan(options);
    await executePlan(plan, root, async (row) => ++calls === 2 ? { status: 'timed_out' } : { status: 'completed', evidence: complete(row) });
    const saved = JSON.parse(await readFile(join(root, 'plan.json'), 'utf8'));
    assert.equal(calls, 2, 'an uncertain trial must not be retried or followed by another trial');
    assert.equal(saved.rows[0].status, 'completed');
    assert.equal(saved.rows[1].status, 'timed_out');
    assert.ok(saved.rows.slice(2).every((row) => row.status === 'planned'));
    assert.equal(summarize(saved).complete, false);
    assert.equal(summarize(saved).comparisonAccepted, false);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('mismatched or incomplete observed identities cannot complete a planned row', async () => {
  for (const mutate of [
    (e) => { e.fixtureId = 'other'; },
    (e) => { e.repetition = 99; },
    (e) => { e.variant = 'other'; },
    (e) => { e.traceComplete = false; },
  ]) {
    const root = await mkdtemp(join(tmpdir(), 'anima-trace-driver-'));
    try {
      const plan = createPlan(options);
      await executePlan(plan, root, async (row) => { const evidence = complete(row); mutate(evidence); return { status: 'completed', evidence }; });
      assert.equal(plan.rows[0].status, 'invalid_evidence');
      assert.ok(plan.rows.slice(1).every((r) => r.status === 'planned'));
      assert.equal(summarize(plan).comparisonAccepted, false);
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});

test('identical binary hashes are repeatability evidence and cannot claim improvement', async () => {
  const root = await mkdtemp(join(tmpdir(), 'anima-trace-driver-'));
  try {
    const plan = createPlan({ ...options, builds: { baseline: build('a'), candidate: build('a') } });
    await executePlan(plan, root, async (row) => ({ status: 'completed', evidence: complete(row) }));
    const result = summarize(plan);
    assert.equal(result.complete, true);
    assert.equal(result.comparisonKind, 'repeatability-control');
    assert.equal(result.improvementClaimAllowed, false);
    assert.equal(result.comparisonAccepted, false, 'the independent Rust matrix validator must also accept the traces');
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('plan bounds reject empty fixtures, excess repetitions and duplicate fixture IDs', () => {
  assert.throws(() => createPlan({ ...options, fixtures: [] }));
  assert.throws(() => createPlan({ ...options, repetitions: 100 }));
  assert.throws(() => createPlan({ ...options, fixtures: ['same', 'same'] }));
});

test('a real owned subprocess is killed and reaped at the bounded timeout', async () => {
  const result = await runProcess(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { timeoutMs: 100, env: {} });
  assert.equal(result.timedOut, true);
  assert.ok(result.elapsedMs < 5000);
});

test('the compiled worker fixture digest must match the current archived fixture set', () => {
  const plan = createPlan(options);
  plan.fixtureFileSha256 = 'e'.repeat(64);
  const row = plan.rows[0];
  const artifact = { traceComplete: true, fixtureFileSha256: 'f'.repeat(64), hostPolicy: plan.hostPolicy, graded: { trace: { fixtureId: row.fixtureId, repetition: row.repetition, variant: row.variant, settings: { provider: plan.provider, model: plan.model } }, oracleFailures: [] } };
  assert.equal(validateArtifact(plan, row, artifact), false, 'a stale compiled-in fixture suite was accepted under the current suite hash');
  artifact.fixtureFileSha256 = plan.fixtureFileSha256;
  assert.equal(validateArtifact(plan, row, artifact), true);
});
