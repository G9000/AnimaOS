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
  new URL('./reference/compare-snapshots.mjs', import.meta.url),
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
  const root = await mkdtemp(path.join(tmpdir(), 'anima-m86-oracle-'));
  const candidate = path.join(root, 'candidate');
  const neighbor = path.join(root, 'neighbor');
  try {
    await cp(fixture, candidate, { recursive: true });
    await cp(neighborFixture, neighbor, { recursive: true });
    if (useReference)
      await writeFile(
        path.join(candidate, 'src/compare-snapshots.mjs'),
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

test('seeded baseline fails both independent primary defects', async () => {
  await withCandidate(async (candidate, neighbor) => {
    const protectedBefore = await captureProtected(candidate, neighbor);
    const verdict = await verifyCandidate(candidate, protectedBefore);
    failure(verdict, 'primary', 'module');
    failure(verdict, 'all-removed', 'module');
    failure(verdict, 'same-size-hash', 'module');
  });
});
test('independent reference passes literal cases with protected and neighbor witnesses', async () => {
  await withCandidate(async (candidate, neighbor) => {
    const protectedBefore = await captureProtected(candidate, neighbor);
    const verdict = await verifyCandidate(candidate, protectedBefore);
    assert.deepEqual(verdict, { passed: true, failures: [] });
  }, true);
});

const sourceMutants = [
  [
    'missing-removals',
    'all-removed',
    replaceOnce(
      reference,
      'for (const path of oldFiles.keys())\n    if (!newFiles.has(path)) removed.push(path);',
      '// Deliberately omit removed paths.',
    ),
  ],
  [
    'bytes-only',
    'same-size-hash',
    replaceOnce(
      reference,
      'previous.bytes !== current.bytes ||\n      previous.sha256 !== current.sha256',
      'previous.bytes !== current.bytes',
    ),
  ],
  [
    'unsorted',
    'all-removed',
    replaceOnce(reference, 'removed: removed.sort()', 'removed'),
  ],
  [
    'unchanged-as-changed',
    'nested-and-unchanged',
    replaceOnce(
      reference,
      'previous.bytes !== current.bytes ||\n      previous.sha256 !== current.sha256',
      'previous',
    ),
  ],
  [
    'hardcoded-primary',
    'empty',
    replaceOnce(
      reference,
      'return {\n    added: added.sort(),\n    removed: removed.sort(),\n    changed: changed.sort(),\n  };',
      'return { added: ["new.txt"], removed: ["old.txt"], changed: ["same.txt", "size.txt"] };',
    ),
  ],
  [
    'mutated-input',
    'primary',
    replaceOnce(
      reference,
      'const oldFiles = records(before);',
      'before.reverse();\n  const oldFiles = records(before);',
    ),
  ],
];
for (const [label, caseId, source] of sourceMutants) {
  test(`reference mutant ${label} fails independent ${caseId}`, async () => {
    await withCandidate(async (candidate, neighbor) => {
      await writeFile(
        path.join(candidate, 'src/compare-snapshots.mjs'),
        source,
      );
      const frozen = await captureProtected(candidate, neighbor);
      const verdict = await verifyCandidate(candidate, frozen);
      failure(verdict, caseId, 'module');
    });
  });
}

test('protected byte/mtime changes and unapproved candidate files fail', async () => {
  await withCandidate(async (candidate, neighbor) => {
    const frozen = await captureProtected(candidate, neighbor);
    await writeFile(
      path.join(candidate, 'README.md'),
      'changed protected README\n',
    );
    await writeFile(path.join(candidate, 'extra.txt'), 'unapproved\n');
    await writeFile(path.join(neighbor, 'keep.txt'), 'changed neighbor\n');
    const verdict = await verifyCandidate(candidate, frozen);
    failure(verdict, 'protected', 'files');
    failure(verdict, 'inventory', 'files');
  }, true);
});
test('an added neighbor file fails the external inventory boundary', async () => {
  await withCandidate(async (candidate, neighbor) => {
    const frozen = await captureProtected(candidate, neighbor);
    await writeFile(
      path.join(neighbor, 'unrelated-extra.txt'),
      'must remain outside candidate authority\n',
    );
    failure(await verifyCandidate(candidate, frozen), 'inventory', 'files');
  }, true);
});
test('an oversized invalid-input diagnostic cannot become an accepted Error', async () => {
  await withCandidate(async (candidate, neighbor) => {
    const mutant = replaceOnce(
      reference,
      "throw new Error('Invalid snapshot record');",
      "throw new Error('x'.repeat(201));",
    );
    await writeFile(path.join(candidate, 'src/compare-snapshots.mjs'), mutant);
    const frozen = await captureProtected(candidate, neighbor);
    failure(
      await verifyCandidate(candidate, frozen),
      'duplicate-path',
      'module',
    );
  });
});
test('only the exact parent-owned todo path can be supplied as an inventory exception', async () => {
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
    failure(
      await verifyCandidate(candidate, frozen, {
        parentAgentId: 'neighbor',
        allowedTodoPath,
      }),
      'inventory',
      'files',
    );
  }, true);
});
test('protected verifier gates CLI output on six tests and reports actual artifact bytes', async () => {
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
    await assert.rejects(readFile(path.join(candidate, 'changes.json')));
    await writeFile(
      path.join(candidate, 'src/compare-snapshots.mjs'),
      reference,
    );
    const final = spawnSync(process.execPath, ['verify.mjs'], {
      cwd: candidate,
      timeout: 8000,
      maxBuffer: 16384,
      windowsHide: true,
    });
    assert.equal(final.status, 0);
    const bytes = await readFile(path.join(candidate, 'changes.json'));
    assert.equal(bytes.toString('utf8'), manifest.primaryArtifact);
    assert.equal(bytes.length, 78);
    const summary = JSON.parse(final.stdout.toString('utf8'));
    assert.deepEqual(summary, {
      testExitCode: 0,
      visibleTests: 6,
      passed: 6,
      failed: 0,
      artifactWritten: true,
      counts: { added: 1, removed: 1, changed: 2 },
      artifactBytes: bytes.length,
      artifactSha256: sha(bytes),
    });
  });
});
test('invalid CLI input preserves existing artifact bytes and mtime', async () => {
  await withCandidate(async (candidate) => {
    await writeFile(
      path.join(candidate, 'src/compare-snapshots.mjs'),
      reference,
    );
    await writeFile(
      path.join(candidate, 'data/before.json'),
      '[{"bad":true}]\n',
    );
    const output = path.join(candidate, 'changes.json');
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
  const tasks = [
    'Inspect and consult one advisory helper',
    'Repair snapshot comparison',
    'Verify tests and inspect output',
  ];
  const todoRows = (completed) =>
    tasks.map((content, i) => ({
      content,
      status:
        i < completed
          ? 'completed'
          : i === completed
            ? 'in_progress'
            : 'pending',
      activeForm: content,
    }));
  const snapshot = {
    messages: ['parent', 'helper'],
    usage: { parent: 5, helper: 2 },
    artifactSha256: hash,
    todos: tasks.map((title) => ({ title, status: 'completed' })),
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
        args: { file_path: 'src/compare-snapshots.mjs' },
      },
      { agent: 'parent', type: 'intent', name: 'spawn_helper' },
      { agent: 'helper', type: 'model', producer: 'primary', retry: false },
      {
        agent: 'helper',
        type: 'tool',
        name: 'read_file',
        status: 'success',
        args: { file_path: 'src/compare-snapshots.mjs' },
      },
      {
        agent: 'helper',
        type: 'final',
        text: 'Two defects and one edge case.',
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
        args: { todos: todoRows(1) },
      },
      {
        agent: 'parent',
        type: 'tool',
        name: 'write_file',
        status: 'success',
        approval: 'allow_once',
        args: {
          file_path: 'src/compare-snapshots.mjs',
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
        args: { todos: todoRows(2) },
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
        args: { file_path: 'changes.json', offset: 0, limit: 200 },
        output: { text: numbered(artifact) },
      },
      {
        agent: 'parent',
        type: 'tool',
        name: 'todo_write',
        stage: 'verification',
        status: 'success',
        approval: 'allow_once',
        args: { todos: todoRows(3) },
      },
      {
        agent: 'parent',
        type: 'final',
        text: 'Six visible tests passed; changes.json has one added, one removed and two changed.',
      },
    ],
    helper: {
      runId: 'helper-1',
      parentRunId: 'parent-1',
      terminal: true,
      advice:
        'Missing removals and same-size hash changes; check prototype-like paths.',
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
        counts: { added: 1, removed: 1, changed: 2 },
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
      finalTasks: tasks.map((title) => ({ title, status: 'completed' })),
    },
    final: {
      text: 'Six visible tests passed; changes.json has one added, one removed and two changed.',
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

test('constructed parent/helper evidence passes only after artifact read and todo completion', () => {
  assert.equal(evaluateTaskEvidence(validEvidence()).status, 'pass');
});
test('unfinished next tasks may remain pending without claiming completion', () => {
  const input = validEvidence();
  for (const operation of input.operations) {
    for (const row of operation.args?.todos ?? []) {
      if (row.status === 'in_progress') row.status = 'pending';
    }
  }
  assert.equal(evaluateTaskEvidence(input).status, 'pass');
});
for (const [label, mutate] of [
  [
    'missing helper',
    (x) => {
      x.helper = null;
    },
  ],
  [
    'helper mutation',
    (x) => {
      x.helper.mutations.push('todo_write');
    },
  ],
  [
    'baseline artifact changed',
    (x) => {
      x.verification.baseline.artifactChanged = true;
    },
  ],
  [
    'failed final verifier',
    (x) => {
      x.verification.final.exitCode = 1;
    },
  ],
  [
    'wrong artifact hash',
    (x) => {
      x.artifact.sha256 = '0'.repeat(64);
    },
  ],
  [
    'partial read-back',
    (x) => {
      x.artifact.readback.text = numbered(x.artifact.text).slice(0, 20);
    },
  ],
  [
    'no initial todo read',
    (x) => {
      x.todos.initialRead = false;
    },
  ],
  [
    'missing verification todo',
    (x) => {
      x.todos.writes.pop();
    },
  ],
  [
    'extra helper spawn',
    (x) => {
      const spawn = x.operations.find(
        (e) => e.name === 'spawn_helper' && e.type === 'tool',
      );
      x.operations.splice(8, 0, structuredClone(spawn));
    },
  ],
  [
    'read before final verifier',
    (x) => {
      const at = x.operations.findIndex(
        (e) => e.name === 'read_file' && e.args?.file_path === 'changes.json',
      );
      const read = x.operations.splice(at, 1)[0];
      x.operations.splice(
        x.operations.findIndex((e) => e.phase === 'final'),
        0,
        read,
      );
    },
  ],
  [
    'todo completion before read',
    (x) => {
      const at = x.operations.findIndex((e) => e.stage === 'verification');
      const write = x.operations.splice(at, 1)[0];
      x.operations.splice(
        x.operations.findIndex(
          (e) => e.name === 'read_file' && e.args?.file_path === 'changes.json',
        ),
        0,
        write,
      );
    },
  ],
  [
    'restart changed todos',
    (x) => {
      x.restart.after.todos[0].status = 'pending';
    },
  ],
  [
    'restart new work',
    (x) => {
      x.restart.newActions = 1;
    },
  ],
  [
    'unknown cleanup',
    (x) => {
      x.cleanup.state = 'unknown';
    },
  ],
]) {
  test(`task evidence rejects ${label}`, () => {
    const input = validEvidence();
    mutate(input);
    assert.notEqual(evaluateTaskEvidence(input).status, 'pass');
  });
}

function expectEvidenceBoundary(input, boundary) {
  const verdict = evaluateTaskEvidence(input);
  assert.notEqual(verdict.status, 'pass');
  assert.ok(
    verdict.reasons.some((reason) => reason.boundary === boundary),
    `expected ${boundary}, got ${JSON.stringify(verdict.reasons)}`,
  );
}
for (const [label, mutate, boundary] of [
  [
    'baseline tool success despite failed tests',
    (x) => {
      x.operations.find((e) => e.phase === 'baseline').status = 'success';
    },
    'verification',
  ],
  [
    'final tool error despite claimed pass',
    (x) => {
      x.operations.find((e) => e.phase === 'final').status = 'error';
    },
    'verification',
  ],
  [
    'failed parent module read',
    (x) => {
      x.operations.find(
        (e) => e.agent === 'parent' && e.name === 'read_file',
      ).status = 'error';
    },
    'authority',
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
    'no parent model observation',
    (x) => {
      x.operations = x.operations.filter(
        (e) => !(e.type === 'model' && e.agent === 'parent'),
      );
    },
    'budget',
  ],
  [
    'no helper model observation',
    (x) => {
      x.operations = x.operations.filter(
        (e) => !(e.type === 'model' && e.agent === 'helper'),
      );
    },
    'budget',
  ],
  [
    'retry parent model',
    (x) => {
      x.operations.find(
        (e) => e.type === 'model' && e.agent === 'parent',
      ).retry = true;
    },
    'budget',
  ],
  [
    'secondary helper producer',
    (x) => {
      x.operations.find(
        (e) => e.type === 'model' && e.agent === 'helper',
      ).producer = 'secondary';
    },
    'budget',
  ],
  [
    'seventh helper model',
    (x) => {
      x.operations.splice(
        5,
        0,
        ...Array.from({ length: 6 }, () => ({
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
    'missing spawn intent',
    (x) => {
      x.operations = x.operations.filter(
        (e) => !(e.type === 'intent' && e.name === 'spawn_helper'),
      );
    },
    'helper',
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
    'repair todo before module write',
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
    'inspection todo rows not completed',
    (x) => {
      x.operations.find((e) => e.stage === 'inspection').args.todos[0].status =
        'pending';
    },
    'todos',
  ],
]) {
  test(`actual-shaped evidence rejects ${label}`, () => {
    const input = validEvidence();
    mutate(input);
    expectEvidenceBoundary(input, boundary);
  });
}
