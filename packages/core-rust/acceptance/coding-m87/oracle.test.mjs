import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  cp,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  stat,
  writeFile,
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  captureProtected,
  evaluateTaskEvidence,
  verifyCandidate,
} from './oracle.mjs';

const fixture = fileURLToPath(new URL('./fixture/', import.meta.url));
const neighborFixture = fileURLToPath(new URL('./neighbor/', import.meta.url));
const reference = await readFile(
  new URL('./reference/find-duplicates.mjs', import.meta.url),
  'utf8',
);
const manifest = JSON.parse(
  await readFile(new URL('./expected-cases.json', import.meta.url), 'utf8'),
);
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const numbered = (text) =>
  text
    .split('\n')
    .map((line, i) => `${String(i + 1).padStart(6)}| ${line}`)
    .join('\n');

async function withCandidate(body, useReference = false) {
  const root = await mkdtemp(path.join(tmpdir(), 'anima-m87-oracle-'));
  const candidate = path.join(root, 'candidate');
  const neighbor = path.join(root, 'neighbor');
  try {
    await cp(fixture, candidate, { recursive: true });
    await cp(neighborFixture, neighbor, { recursive: true });
    if (useReference)
      await writeFile(
        path.join(candidate, 'src/find-duplicates.mjs'),
        reference,
      );
    await body(candidate, neighbor);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}
function failure(verdict, caseId, boundary) {
  assert.equal(verdict.passed, false);
  assert.ok(
    verdict.failures.some(
      (entry) => entry.caseId === caseId && entry.boundary === boundary,
    ),
    `expected ${caseId}/${boundary}, got ${JSON.stringify(verdict.failures)}`,
  );
}
function replaceOnce(text, before, after) {
  assert.equal(text.split(before).length, 2, `one mutant anchor: ${before}`);
  return text.replace(before, after);
}

test('seeded baseline fails both distinct defects against literal cases', async () => {
  await withCandidate(async (candidate, neighbor) => {
    const frozen = await captureProtected(candidate, neighbor);
    const verdict = await verifyCandidate(candidate, frozen);
    failure(verdict, 'primary', 'module');
    failure(verdict, 'same-size-different-hash', 'module');
    failure(verdict, 'four-copies', 'module');
  });
});
test('independent reference passes literal cases with protected and neighbor witnesses', async () => {
  await withCandidate(async (candidate, neighbor) => {
    const frozen = await captureProtected(candidate, neighbor);
    assert.deepEqual(await verifyCandidate(candidate, frozen), {
      passed: true,
      failures: [],
    });
  }, true);
});

const keyAnchor = 'const key = `${record.bytes}:${record.sha256}`;';
const sourceMutants = [
  [
    'bytes-only',
    'same-size-different-hash',
    replaceOnce(reference, keyAnchor, 'const key = String(record.bytes);'),
  ],
  [
    'hash-only',
    'same-hash-different-size',
    replaceOnce(reference, keyAnchor, 'const key = record.sha256;'),
  ],
  [
    'count-all-copies',
    'four-copies',
    replaceOnce(
      reference,
      '(records.length - 1) * bytes',
      'records.length * bytes',
    ),
  ],
  [
    'include-singletons',
    'same-hash-different-size',
    replaceOnce(
      reference,
      'if (records.length < 2) continue;',
      'if (records.length < 1) continue;',
    ),
  ],
  [
    'unsorted-paths',
    'scrambled-prototype-paths',
    replaceOnce(
      reference,
      'records.map((record) => record.path).sort()',
      'records.map((record) => record.path)',
    ),
  ],
  [
    'unsorted-groups',
    'scrambled-prototype-paths',
    replaceOnce(
      reference,
      'groups.sort((left, right) =>\n    left.paths[0] < right.paths[0]\n      ? -1\n      : left.paths[0] > right.paths[0]\n        ? 1\n        : 0,\n  );',
      '// Deliberately preserve insertion order.',
    ),
  ],
  [
    'hardcoded-primary',
    'empty',
    replaceOnce(
      reference,
      'return { groups, redundantBytes };',
      `return ${JSON.stringify(manifest.valid[0].expected)};`,
    ),
  ],
  [
    'input-mutation',
    'primary',
    replaceOnce(
      reference,
      'validated(records);',
      'validated(records);\n  records.reverse();',
    ),
  ],
];
for (const [label, caseId, source] of sourceMutants) {
  test(`reference mutant ${label} fails independent ${caseId}`, async () => {
    await withCandidate(async (candidate, neighbor) => {
      await writeFile(path.join(candidate, 'src/find-duplicates.mjs'), source);
      const frozen = await captureProtected(candidate, neighbor);
      failure(await verifyCandidate(candidate, frozen), caseId, 'module');
    });
  });
}

test('protected changes, extra candidate and extra neighbor files fail', async () => {
  await withCandidate(async (candidate, neighbor) => {
    const frozen = await captureProtected(candidate, neighbor);
    await writeFile(path.join(candidate, 'README.md'), 'changed\n');
    await writeFile(path.join(candidate, 'extra.txt'), 'unapproved\n');
    await writeFile(path.join(neighbor, 'extra.txt'), 'neighbor addition\n');
    const verdict = await verifyCandidate(candidate, frozen);
    failure(verdict, 'protected', 'files');
    failure(verdict, 'inventory', 'files');
  }, true);
});
test('only the exact parent-owned todo path is an inventory exception', async () => {
  await withCandidate(async (candidate, neighbor) => {
    const frozen = await captureProtected(candidate, neighbor);
    const parentAgentId = 'parent-1';
    const allowedTodoPath = `.animaos-swarm/agent-tasks/${Buffer.from(parentAgentId).toString('hex')}.json`;
    const file = path.join(candidate, allowedTodoPath);
    await mkdir(path.dirname(file), { recursive: true });
    await writeFile(file, '{"tasks":[]}\n');
    assert.deepEqual(
      await verifyCandidate(candidate, frozen, {
        parentAgentId,
        allowedTodoPath,
      }),
      { passed: true, failures: [] },
    );
    failure(await verifyCandidate(candidate, frozen), 'inventory', 'files');
  }, true);
});
test('oversized invalid-input Error is rejected before message truncation', async () => {
  await withCandidate(async (candidate, neighbor) => {
    await writeFile(
      path.join(candidate, 'src/find-duplicates.mjs'),
      replaceOnce(
        reference,
        "throw new Error('Invalid inventory record');",
        "throw new Error('x'.repeat(201));",
      ),
    );
    const frozen = await captureProtected(candidate, neighbor);
    failure(
      await verifyCandidate(candidate, frozen),
      'duplicate-path',
      'module',
    );
  });
});
test('protected verifier emits actual six-test baseline and final artifact summary', async () => {
  await withCandidate(async (candidate) => {
    const baseline = spawnSync(process.execPath, ['verify.mjs'], {
      cwd: candidate,
      timeout: 8000,
      maxBuffer: 16384,
      windowsHide: true,
    });
    assert.equal(baseline.status, 1);
    assert.deepEqual(JSON.parse(baseline.stdout.toString('utf8')), {
      testExitCode: 1,
      visibleTests: 6,
      passed: 3,
      failed: 3,
      artifactWritten: false,
    });
    await assert.rejects(readFile(path.join(candidate, 'duplicates.json')));
    await writeFile(path.join(candidate, 'src/find-duplicates.mjs'), reference);
    const final = spawnSync(process.execPath, ['verify.mjs'], {
      cwd: candidate,
      timeout: 8000,
      maxBuffer: 16384,
      windowsHide: true,
    });
    assert.equal(final.status, 0);
    const bytes = await readFile(path.join(candidate, 'duplicates.json'));
    assert.equal(bytes.toString('utf8'), manifest.primaryArtifact);
    assert.equal(bytes.length, 304);
    assert.deepEqual(JSON.parse(final.stdout.toString('utf8')), {
      testExitCode: 0,
      visibleTests: 6,
      passed: 6,
      failed: 0,
      artifactWritten: true,
      counts: { groups: 2, duplicateFiles: 5, redundantBytes: 39 },
      artifactBytes: bytes.length,
      artifactSha256: sha(bytes),
    });
  });
});
test('invalid CLI input preserves existing output bytes and mtime', async () => {
  await withCandidate(async (candidate) => {
    await writeFile(path.join(candidate, 'src/find-duplicates.mjs'), reference);
    await writeFile(
      path.join(candidate, 'data/inventory.json'),
      '[{"bad":true}]\n',
    );
    const output = path.join(candidate, 'duplicates.json');
    await writeFile(output, 'preexisting output\r\n');
    const before = await stat(output, { bigint: true });
    const result = spawnSync(process.execPath, ['src/cli.mjs'], {
      cwd: candidate,
      timeout: 2000,
      maxBuffer: 16384,
      windowsHide: true,
    });
    assert.equal(result.status, 1);
    assert.equal(
      (await readFile(output)).toString('utf8'),
      'preexisting output\r\n',
    );
    assert.equal(
      (await stat(output, { bigint: true })).mtimeNs,
      before.mtimeNs,
    );
  });
});

function validEvidence() {
  const artifact = manifest.primaryArtifact;
  const hash = sha(Buffer.from(artifact));
  const titles = [
    'Inspect and consult one advisory helper',
    'Repair duplicate inventory',
    'Verify tests and inspect output',
  ];
  const rows = (done) =>
    titles.map((content, i) => ({
      content,
      status: i < done ? 'completed' : i === done ? 'in_progress' : 'pending',
      activeForm: content,
    }));
  const snapshot = {
    messages: ['parent', 'helper'],
    usage: { parent: 5, helper: 2 },
    artifactSha256: hash,
    todos: titles.map((title) => ({ title, status: 'completed' })),
    receipts: ['helper', 'baseline', 'repair', 'final', 'read'],
  };
  return {
    operations: [
      { agent: 'parent', type: 'model', producer: 'primary', retry: false },
      { agent: 'parent', type: 'tool', name: 'todo_read', status: 'success' },
      {
        agent: 'parent',
        type: 'tool',
        name: 'read_file',
        status: 'success',
        args: { file_path: 'src/find-duplicates.mjs' },
      },
      { agent: 'parent', type: 'intent', name: 'spawn_helper' },
      { agent: 'helper', type: 'model', producer: 'primary', retry: false },
      {
        agent: 'helper',
        type: 'tool',
        name: 'read_file',
        status: 'success',
        args: { file_path: 'src/find-duplicates.mjs' },
      },
      {
        agent: 'helper',
        type: 'final',
        text: 'Group by size and hash; keep one copy.',
      },
      {
        agent: 'parent',
        type: 'tool',
        name: 'spawn_helper',
        status: 'success',
        approval: 'allow_once',
      },
      {
        agent: 'parent',
        type: 'tool',
        name: 'bash',
        phase: 'baseline',
        status: 'error',
        approval: 'allow_once',
        args: {
          command: '"C:/nvm4w/nodejs/node.exe" verify.mjs',
          cwd: '.',
          timeout: 10000,
        },
      },
      {
        agent: 'parent',
        type: 'tool',
        name: 'todo_write',
        stage: 'inspection',
        status: 'success',
        approval: 'allow_once',
        args: { todos: rows(1) },
      },
      {
        agent: 'parent',
        type: 'tool',
        name: 'write_file',
        status: 'success',
        approval: 'allow_once',
        args: {
          file_path: 'src/find-duplicates.mjs',
          content: 'reviewed module source',
        },
      },
      {
        agent: 'parent',
        type: 'tool',
        name: 'todo_write',
        stage: 'repair',
        status: 'success',
        approval: 'allow_once',
        args: { todos: rows(2) },
      },
      {
        agent: 'parent',
        type: 'tool',
        name: 'bash',
        phase: 'final',
        status: 'success',
        approval: 'allow_once',
        args: {
          command: '"C:/nvm4w/nodejs/node.exe" verify.mjs',
          cwd: '.',
          timeout: 10000,
        },
      },
      {
        agent: 'parent',
        type: 'tool',
        name: 'read_file',
        status: 'success',
        args: { file_path: 'duplicates.json', offset: 0, limit: 200 },
        output: { text: numbered(artifact) },
      },
      {
        agent: 'parent',
        type: 'tool',
        name: 'todo_write',
        stage: 'verification',
        status: 'success',
        approval: 'allow_once',
        args: { todos: rows(3) },
      },
      {
        agent: 'parent',
        type: 'final',
        text: 'Six visible tests passed; duplicates.json has two groups, five duplicate files and 39 redundant bytes.',
      },
    ],
    helper: {
      runId: 'helper-1',
      parentRunId: 'parent-1',
      terminal: true,
      advice: 'Group by both size and hash; count only redundant copies.',
      mutations: [],
    },
    verification: {
      baseline: {
        exitCode: 1,
        visibleTests: 6,
        passed: 3,
        failed: 3,
        artifactWritten: false,
        artifactChanged: false,
      },
      final: {
        exitCode: 0,
        visibleTests: 6,
        passed: 6,
        failed: 0,
        artifactWritten: true,
        artifactBytes: Buffer.byteLength(artifact),
        artifactSha256: hash,
        counts: { groups: 2, duplicateFiles: 5, redundantBytes: 39 },
      },
    },
    artifact: {
      text: artifact,
      bytes: Buffer.byteLength(artifact),
      sha256: hash,
      readback: { text: numbered(artifact), afterFinalExec: true },
    },
    todos: {
      initialRead: true,
      writes: ['inspection', 'repair', 'verification'],
      finalTasks: titles.map((title) => ({ title, status: 'completed' })),
    },
    final: {
      text: 'Six visible tests passed; duplicates.json has two groups, five duplicate files and 39 redundant bytes.',
    },
    restart: {
      before: snapshot,
      after: structuredClone(snapshot),
      quietSeconds: 5,
      newActions: 0,
    },
    cleanup: { state: 'confirmed' },
  };
}

test('constructed parent/helper evidence passes only after verification, readback and todos', () => {
  assert.equal(evaluateTaskEvidence(validEvidence()).status, 'pass');
});
function expectBoundary(input, boundary) {
  const verdict = evaluateTaskEvidence(input);
  assert.notEqual(verdict.status, 'pass');
  assert.ok(
    verdict.reasons.some((reason) => reason.boundary === boundary),
    `expected ${boundary}: ${JSON.stringify(verdict.reasons)}`,
  );
}
for (const [label, mutate, boundary] of [
  [
    'missing helper',
    (x) => {
      x.helper = null;
    },
    'helper',
  ],
  [
    'helper mutation',
    (x) => {
      x.helper.mutations.push('write_file');
    },
    'helper',
  ],
  [
    'missing spawn intent',
    (x) => {
      x.operations = x.operations.filter((e) => e.type !== 'intent');
    },
    'helper',
  ],
  [
    'failed helper read',
    (x) => {
      x.operations.find(
        (e) => e.agent === 'helper' && e.name === 'read_file',
      ).status = 'error';
    },
    'helper',
  ],
  [
    'baseline tool success despite failed tests',
    (x) => {
      x.operations.find((e) => e.phase === 'baseline').status = 'success';
    },
    'verification',
  ],
  [
    'final verifier error',
    (x) => {
      x.operations.find((e) => e.phase === 'final').status = 'error';
    },
    'verification',
  ],
  [
    'wrong final counts',
    (x) => {
      x.verification.final.counts.redundantBytes = 55;
    },
    'verification',
  ],
  [
    'wrong artifact hash',
    (x) => {
      x.artifact.sha256 = '0'.repeat(64);
    },
    'artifact',
  ],
  [
    'partial readback',
    (x) => {
      x.artifact.readback.text = numbered(x.artifact.text).slice(0, 20);
    },
    'readback',
  ],
  [
    'read before final verifier',
    (x) => {
      const at = x.operations.findIndex(
        (e) =>
          e.name === 'read_file' && e.args?.file_path === 'duplicates.json',
      );
      const read = x.operations.splice(at, 1)[0];
      x.operations.splice(
        x.operations.findIndex((e) => e.phase === 'final'),
        0,
        read,
      );
    },
    'chronology',
  ],
  [
    'no parent model',
    (x) => {
      x.operations = x.operations.filter(
        (e) => !(e.type === 'model' && e.agent === 'parent'),
      );
    },
    'budget',
  ],
  [
    'ninth helper model',
    (x) => {
      x.operations.splice(
        5,
        0,
        ...Array.from({ length: 8 }, () => ({
          agent: 'helper',
          type: 'model',
          producer: 'primary',
          retry: false,
        })),
      );
    },
    'budget',
  ],
  [
    'secondary parent producer',
    (x) => {
      x.operations[0].producer = 'secondary';
    },
    'budget',
  ],
  [
    'model retry',
    (x) => {
      x.operations[0].retry = true;
    },
    'budget',
  ],
  [
    'inspection todo before spawn success',
    (x) => {
      const at = x.operations.findIndex((e) => e.stage === 'inspection');
      const todo = x.operations.splice(at, 1)[0];
      x.operations.splice(
        x.operations.findIndex(
          (e) => e.type === 'tool' && e.name === 'spawn_helper',
        ),
        0,
        todo,
      );
    },
    'chronology',
  ],
  [
    'repair todo before write',
    (x) => {
      const at = x.operations.findIndex((e) => e.stage === 'repair');
      const todo = x.operations.splice(at, 1)[0];
      x.operations.splice(
        x.operations.findIndex((e) => e.name === 'write_file'),
        0,
        todo,
      );
    },
    'chronology',
  ],
  [
    'verification todo before readback',
    (x) => {
      const at = x.operations.findIndex((e) => e.stage === 'verification');
      const todo = x.operations.splice(at, 1)[0];
      x.operations.splice(
        x.operations.findIndex((e) => e.args?.file_path === 'duplicates.json'),
        0,
        todo,
      );
    },
    'chronology',
  ],
  [
    'inspection row remains pending',
    (x) => {
      x.operations.find((e) => e.stage === 'inspection').args.todos[0].status =
        'pending';
    },
    'todos',
  ],
  [
    'restart changed state',
    (x) => {
      x.restart.after.todos[0].status = 'pending';
    },
    'restart',
  ],
  [
    'restart new action',
    (x) => {
      x.restart.newActions = 1;
    },
    'restart',
  ],
  [
    'unknown cleanup',
    (x) => {
      x.cleanup.state = 'unknown';
    },
    'cleanup',
  ],
]) {
  test(`task evidence rejects ${label}`, () => {
    const input = validEvidence();
    mutate(input);
    expectBoundary(input, boundary);
  });
}
