import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import test from 'node:test';
import { evaluateResearch } from './oracle.mjs';

const index = 'https://doc.rust-lang.org/edition-guide/rust-2024/index.html';
const transition =
  'https://doc.rust-lang.org/edition-guide/editions/transitioning-an-existing-project-to-a-new-edition.html';
const query =
  'site:doc.rust-lang.org/edition-guide Rust 2024 first stable release edition Cargo.toml cargo fix migration';
const report = `# Rust 2024 research

| Question | Answer | Source |
| --- | --- | --- |
| Minimum stable Rust | Rust 1.85.0 is the first stable release for the 2024 edition. | [Edition guide](${index}) |
| Manifest entry | Set \`edition = "2024"\` in Cargo.toml. | [Transition guide](${transition}) |
| Automatic migration command | Run \`cargo fix --edition\` before changing the manifest. | [Transition guide](${transition}) |

Limits: cargo fix assists migration; review its edits and test the project. No project was changed here.
`;
const numbered = (text) =>
  text
    .split('\n')
    .map((line, i) => `${String(i + 1).padStart(6)}| ${line}`)
    .join('\n');
const sha = (text) => createHash('sha256').update(text).digest('hex');

function valid() {
  const snapshot = {
    reportSha256: sha(report),
    messages: ['task', 'final'],
    sources: [index, transition],
    receipts: ['search', 'fetch1', 'fetch2', 'write', 'read'],
    usage: { modelCalls: 5, toolCalls: 5 },
  };
  return {
    report: {
      text: report,
      fileText: report,
      fileSha256: sha(report),
      fileBytes: Buffer.byteLength(report),
    },
    expectations: {
      query,
      path: 'rust-2024-research.md',
      minimumStable: '1.85.0',
      manifestEntry: 'edition = "2024"',
      migrationCommand: 'cargo fix --edition',
    },
    operations: [
      { type: 'model', producer: 'primary', retry: false },
      {
        type: 'tool',
        name: 'web_search',
        args: { query, num_results: 3 },
        status: 'success',
        approval: 'allow_once',
        output: { urls: [index, transition] },
      },
      { type: 'model', producer: 'primary', retry: false },
      {
        type: 'tool',
        name: 'web_fetch',
        args: { url: index, max_length: 20000 },
        status: 'success',
        approval: 'allow_once',
        output: {
          text: 'The Rust 2024 Edition was stabilized in Rust 1.85.0.',
        },
      },
      {
        type: 'tool',
        name: 'web_fetch',
        args: { url: transition, max_length: 20000 },
        status: 'success',
        approval: 'allow_once',
        output: {
          text: 'Run cargo fix --edition, then change Cargo.toml to edition = "2024".',
        },
      },
      { type: 'model', producer: 'primary', retry: false },
      {
        type: 'tool',
        name: 'write_file',
        args: { file_path: 'rust-2024-research.md', content: report },
        status: 'success',
        approval: 'allow_once',
      },
      { type: 'model', producer: 'primary', retry: false },
      {
        type: 'tool',
        name: 'read_file',
        args: { file_path: 'rust-2024-research.md', offset: 0, limit: 200 },
        status: 'success',
        approval: 'not_required',
        output: { text: numbered(report) },
      },
      { type: 'model', producer: 'primary', retry: false },
      { type: 'final', text: 'Saved rust-2024-research.md and read it back.' },
    ],
    restart: {
      before: snapshot,
      after: structuredClone(snapshot),
      quietSeconds: 5,
      newActions: 0,
    },
    cleanup: { state: 'confirmed' },
  };
}

const op = (input, name) =>
  input.operations.find((entry) => entry.name === name);
const status = (input) => evaluateResearch(input).status;
test('constructed, sourced report and full numbered read-back pass', () => {
  assert.equal(status(valid()), 'pass');
});
test('equivalent returned official URL fragment and repeated citation pass', () => {
  const input = valid();
  const fragment = `${transition}#edition`;
  input.report.text = input.report.text.replaceAll(transition, fragment);
  input.report.fileText = input.report.text;
  input.report.fileSha256 = sha(input.report.text);
  input.report.fileBytes = Buffer.byteLength(input.report.text);
  op(input, 'write_file').args.content = input.report.text;
  op(input, 'read_file').output.text = numbered(input.report.text);
  input.restart.before.reportSha256 = input.report.fileSha256;
  input.restart.after.reportSha256 = input.report.fileSha256;
  assert.equal(status(input), 'pass');
});

const reportMutants = [
  [
    'wrong stable release',
    (x) => x.replace('Rust 1.85.0 is', 'Rust 1.84.0 is'),
  ],
  [
    'version decoy in title',
    (x) =>
      x
        .replace('# Rust 2024 research', '# Rust 2024 research 1.85.0')
        .replace('Rust 1.85.0 is', 'Rust 1.84.0 is'),
  ],
  [
    'negated stable release',
    (x) => x.replace('Rust 1.85.0 is', 'Rust 1.85.0 is not'),
  ],
  [
    'wrong manifest',
    (x) => x.replace('Set `edition = "2024"`', 'Set `edition = "2021"`'),
  ],
  [
    'manifest decoy in limits',
    (x) =>
      x
        .replace('Set `edition = "2024"`', 'Set `edition = "2021"`')
        .replace('Limits:', 'Limits: `edition = "2024"`.'),
  ],
  [
    'wrong migration command',
    (x) => x.replace('Run `cargo fix --edition`', 'Run `cargo fix`'),
  ],
  [
    'migration decoy in limits',
    (x) =>
      x
        .replace('Run `cargo fix --edition`', 'Run `cargo fix`')
        .replace('Limits:', 'Limits: `cargo fix --edition`.'),
  ],
  [
    'fabricated citation',
    (x) =>
      x.replace(
        transition,
        'https://doc.rust-lang.org/edition-guide/fabricated.html',
      ),
  ],
  [
    'nonofficial citation',
    (x) => x.replace(index, 'https://example.com/rust-2024'),
  ],
  [
    'malformed table',
    (x) => x.replace('| Question | Answer | Source |', '| Question | Answer |'),
  ],
  ['missing source link', (x) => x.replace(`[Edition guide](${index})`, index)],
  ['oversized report', (x) => `${x}\n${'x'.repeat(9000)}`],
  ['over 100 lines', (x) => `${x}${'\nextra'.repeat(102)}`],
];
for (const [label, mutate] of reportMutants) {
  test(`report rejects ${label}`, () => {
    const input = valid();
    input.report.text = mutate(input.report.text);
    input.report.fileText = input.report.text;
    input.report.fileSha256 = sha(input.report.text);
    input.report.fileBytes = Buffer.byteLength(input.report.text);
    op(input, 'write_file').args.content = input.report.text;
    op(input, 'read_file').output.text = numbered(input.report.text);
    input.restart.before.reportSha256 = input.report.fileSha256;
    input.restart.after.reportSha256 = input.report.fileSha256;
    assert.notEqual(status(input), 'pass');
  });
}

const evidenceMutants = [
  [
    'unreturned citation',
    (x) => {
      op(x, 'web_search').output.urls = [index];
    },
  ],
  [
    'unfetched citation',
    (x) => {
      x.operations = x.operations.filter(
        (e) => e.name !== 'web_fetch' || e.args.url !== transition,
      );
    },
  ],
  [
    'failed search',
    (x) => {
      op(x, 'web_search').status = 'error';
    },
  ],
  [
    'empty search',
    (x) => {
      op(x, 'web_search').output.urls = [];
    },
  ],
  [
    'failed fetch',
    (x) => {
      x.operations.find((e) => e.name === 'web_fetch').status = 'error';
    },
  ],
  [
    'insufficient fetched text',
    (x) => {
      x.operations.filter((e) => e.name === 'web_fetch')[1].output.text =
        'Result truncated before migration details';
    },
  ],
  [
    'changed source identity',
    (x) => {
      x.operations.filter((e) => e.name === 'web_fetch')[1].args.url =
        `${transition}/elsewhere`;
    },
  ],
  [
    'missing read-back',
    (x) => {
      x.operations = x.operations.filter((e) => e.name !== 'read_file');
    },
  ],
  [
    'pre-write read-back',
    (x) => {
      const read = x.operations.splice(
        x.operations.findIndex((e) => e.name === 'read_file'),
        1,
      )[0];
      x.operations.splice(
        x.operations.findIndex((e) => e.name === 'write_file'),
        0,
        read,
      );
    },
  ],
  [
    'partial read-back',
    (x) => {
      op(x, 'read_file').output.text = numbered(report)
        .split('\n')
        .slice(0, 5)
        .join('\n');
    },
  ],
  [
    'final before read-back',
    (x) => {
      const final = x.operations.pop();
      x.operations.splice(
        x.operations.findIndex((e) => e.name === 'read_file'),
        0,
        final,
      );
    },
  ],
  [
    'write after read-back',
    (x) => {
      x.operations.splice(
        x.operations.findIndex((e) => e.name === 'read_file') + 1,
        0,
        structuredClone(op(x, 'write_file')),
      );
    },
  ],
  [
    'search and fetch in same model batch',
    (x) => {
      x.operations.splice(
        x.operations.findIndex(
          (e) => e.type === 'model' && x.operations.indexOf(e) > 0,
        ),
        1,
      );
    },
  ],
  [
    'write and read in same model batch',
    (x) => {
      x.operations.splice(
        x.operations.findIndex(
          (e) =>
            e.type === 'model' &&
            x.operations.indexOf(e) >
              x.operations.findIndex((v) => v.name === 'write_file'),
        ),
        1,
      );
    },
  ],
  [
    'report changed after write',
    (x) => {
      x.report.fileText += 'changed';
      x.report.fileSha256 = sha(x.report.fileText);
      x.report.fileBytes = Buffer.byteLength(x.report.fileText);
    },
  ],
  [
    'report hash mismatch',
    (x) => {
      x.report.fileSha256 = '0'.repeat(64);
    },
  ],
  [
    'missing report',
    (x) => {
      x.report.fileText = null;
    },
  ],
  [
    'seventh model call',
    (x) => {
      x.operations.splice(
        7,
        0,
        ...Array.from({ length: 2 }, () => ({
          type: 'model',
          producer: 'primary',
          retry: false,
        })),
      );
    },
  ],
  [
    'retry',
    (x) => {
      x.operations[0].retry = true;
    },
  ],
  [
    'secondary producer',
    (x) => {
      x.operations[0].producer = 'evaluator';
    },
  ],
  [
    'extra search',
    (x) => {
      x.operations.splice(2, 0, structuredClone(op(x, 'web_search')));
    },
  ],
  [
    'extra fetch',
    (x) => {
      x.operations.splice(4, 0, structuredClone(op(x, 'web_fetch')));
    },
  ],
  [
    'helper tool',
    (x) => {
      x.operations.splice(4, 0, {
        type: 'tool',
        name: 'delegate',
        args: {},
        status: 'success',
      });
    },
  ],
  [
    'wrong search query',
    (x) => {
      op(x, 'web_search').args.query += ' expanded';
    },
  ],
  [
    'wrong search count',
    (x) => {
      op(x, 'web_search').args.num_results = 10;
    },
  ],
  [
    'wrong report path',
    (x) => {
      op(x, 'write_file').args.file_path = 'other.md';
    },
  ],
  [
    'denied approval',
    (x) => {
      op(x, 'write_file').approval = 'denied';
    },
  ],
  [
    'missing approval',
    (x) => {
      delete op(x, 'web_fetch').approval;
    },
  ],
  [
    'unsupported final claim',
    (x) => {
      x.operations.at(-1).text =
        'Saved rust-2024-research.md; I ran cargo test and migrated the project.';
    },
  ],
  [
    'final omits file',
    (x) => {
      x.operations.at(-1).text = 'Research complete.';
    },
  ],
  [
    'restart changed messages',
    (x) => {
      x.restart.after.messages.push('new');
    },
  ],
  [
    'restart changed sources',
    (x) => {
      x.restart.after.sources.pop();
    },
  ],
  [
    'restart changed report',
    (x) => {
      x.restart.after.reportSha256 = '1'.repeat(64);
    },
  ],
  [
    'restart changed usage',
    (x) => {
      x.restart.after.usage.modelCalls++;
    },
  ],
  [
    'restart new action',
    (x) => {
      x.restart.newActions = 1;
    },
  ],
  [
    'short quiet window',
    (x) => {
      x.restart.quietSeconds = 1;
    },
  ],
];
for (const [label, mutate] of evidenceMutants) {
  test(`evidence rejects ${label}`, () => {
    const input = valid();
    mutate(input);
    assert.notEqual(status(input), 'pass');
  });
}
test('honest missing read-back is PARTIAL', () => {
  const input = valid();
  input.operations = input.operations.filter((e) => e.name !== 'read_file');
  assert.equal(status(input), 'partial');
});
test('unknown cleanup is PARTIAL', () => {
  const input = valid();
  input.cleanup.state = 'unknown';
  assert.equal(status(input), 'partial');
});

function setReport(input, text) {
  input.report.text = text;
  input.report.fileText = text;
  input.report.fileSha256 = sha(text);
  input.report.fileBytes = Buffer.byteLength(text);
  op(input, 'write_file').args.content = text;
  op(input, 'read_file').output.text = numbered(text);
  input.restart.before.reportSha256 = input.report.fileSha256;
  input.restart.after.reportSha256 = input.report.fileSha256;
}

test('extra read-back is FAIL while absent read-back is PARTIAL', () => {
  const extra = valid();
  extra.operations.splice(
    extra.operations.findIndex((e) => e.name === 'read_file') + 1,
    0,
    structuredClone(op(extra, 'read_file')),
  );
  assert.equal(status(extra), 'fail');
  const absent = valid();
  absent.operations = absent.operations.filter((e) => e.name !== 'read_file');
  assert.equal(status(absent), 'partial');
});

test('unselected third-party ranked result does not invalidate official selected sources', () => {
  const input = valid();
  op(input, 'web_search').output.urls.push('https://example.com/third-result');
  assert.equal(status(input), 'pass');
});
test('selecting or citing a ranked third-party result fails source authority', () => {
  const selected = valid();
  op(selected, 'web_search').output.urls.push(
    'https://example.com/third-result',
  );
  selected.operations.filter((e) => e.name === 'web_fetch')[1].args.url =
    'https://example.com/third-result';
  assert.equal(status(selected), 'fail');
  const cited = valid();
  op(cited, 'web_search').output.urls.push('https://example.com/third-result');
  setReport(
    cited,
    report.replaceAll(transition, 'https://example.com/third-result'),
  );
  assert.equal(status(cited), 'fail');
});

test('the second fetch and write in one model response cannot establish causal use', () => {
  const input = valid();
  const beforeWrite = input.operations.findIndex(
    (e) => e.name === 'write_file',
  );
  const model = input.operations.splice(beforeWrite - 1, 1)[0];
  input.operations.splice(
    input.operations.findIndex((e) => e.name === 'web_fetch') + 1,
    0,
    model,
  );
  assert.equal(status(input), 'fail');
});

for (const encoded of ['&quot;', '&#34;']) {
  test(`native fetched ${encoded} quotes and harmless assignment whitespace pass`, () => {
    const input = valid();
    setReport(input, report.replace('`edition = "2024"`', '`edition="2024"`'));
    input.operations.filter((e) => e.name === 'web_fetch')[1].output.text =
      `Run cargo fix --edition, then change Cargo.toml to edition = ${encoded}2024${encoded}.`;
    assert.equal(status(input), 'pass');
  });
  test(`native fetched ${encoded} wrong manifest literal fails`, () => {
    const input = valid();
    input.operations.filter((e) => e.name === 'web_fetch')[1].output.text =
      `Run cargo fix --edition, then change Cargo.toml to edition = ${encoded}2021${encoded}.`;
    assert.equal(status(input), 'fail');
  });
}

for (const [label, before, after] of [
  ['version HTML comment', 'Rust 1.85.0 is', 'Rust 1.84.0 <!-- 1.85.0 --> is'],
  [
    'manifest HTML comment',
    '`edition = "2024"`',
    '`edition = "2021"` <!-- edition = "2024" -->',
  ],
  [
    'command HTML comment',
    '`cargo fix --edition`',
    '`cargo fix` <!-- cargo fix --edition -->',
  ],
  ['extended version token', 'Rust 1.85.0 is', 'Rust 1.85.00 is'],
  ['version suffix token', 'Rust 1.85.0 is', 'Rust 1.85.0beta is'],
  [
    'uppercase migration command',
    '`cargo fix --edition`',
    '`CARGO FIX --EDITION`',
  ],
]) {
  test(`visible answer rejects ${label}`, () => {
    const input = valid();
    setReport(input, report.replace(before, after));
    assert.equal(status(input), 'fail');
  });
}

test('sentence punctuation after the stable version remains a valid token', () => {
  const input = valid();
  setReport(
    input,
    report.replace(
      'Rust 1.85.0 is the first stable release',
      'Rust 1.85.0. This is the first stable release',
    ),
  );
  assert.equal(status(input), 'pass');
});

for (const [label, mutate] of [
  [
    'object search urls',
    (x) => {
      op(x, 'web_search').output.urls = {};
    },
  ],
  [
    'object fetch text',
    (x) => {
      x.operations.filter((e) => e.name === 'web_fetch')[1].output.text = {};
    },
  ],
  [
    'null operation',
    (x) => {
      x.operations.splice(3, 0, null);
    },
  ],
  [
    'null search args',
    (x) => {
      op(x, 'web_search').args = null;
    },
  ],
  [
    'missing read output',
    (x) => {
      delete op(x, 'read_file').output;
    },
  ],
  [
    'null restart snapshot',
    (x) => {
      x.restart.after = null;
    },
  ],
]) {
  test(`malformed ${label} gives structured nonpass`, () => {
    const input = valid();
    mutate(input);
    const verdict = evaluateResearch(input);
    assert.ok(['fail', 'partial'].includes(verdict.status));
    assert.ok(Array.isArray(verdict.reasons) && verdict.reasons.length > 0);
  });
}

for (const [label, value] of [
  ['missing', undefined],
  ['string', '5'],
  ['nonfinite', Infinity],
]) {
  test(`${label} quiet window is PARTIAL`, () => {
    const input = valid();
    input.restart.quietSeconds = value;
    assert.equal(status(input), 'partial');
  });
}
