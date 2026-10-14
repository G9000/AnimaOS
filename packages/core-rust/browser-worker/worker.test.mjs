import assert from 'node:assert/strict';
import test from 'node:test';
import { createRequire } from 'node:module';
import { EventEmitter, once } from 'node:events';
import { PassThrough, Writable } from 'node:stream';
import { createHash } from 'node:crypto';
import * as filesystem from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import {
  createServer as createHttpServer,
  request as httpRequest,
} from 'node:http';
import { createConnection } from 'node:net';
import {
  boundObservation,
  BrowserWorkerError,
  canonicalDigest,
  createDenyProxy,
  createMaintainedSession,
  decodeFrame,
  decodeOperation,
  encodeFrame,
  FrameReader,
  guardRequest,
  LIMITS,
  parseBootstrap,
  parseCliArguments,
  readBoundedResponse,
  runCli,
  runProtocol,
  snapshotEvidence,
  validateExecute,
  validateObservation,
  validateOpen,
  validateScope,
  validateWorkerEnvironment,
} from './worker.mjs';

function scope() {
  return {
    id: 'scope-1',
    revision: 1,
    origin: 'http://127.0.0.1:43219',
    routes: [
      { id: 'index', method: 'GET', path: '/' },
      { id: 'script', method: 'GET', path: '/app.js' },
      { id: 'submit', method: 'POST', path: '/quote' },
      { id: 'csv', method: 'GET', path: '/quote.csv' },
    ],
    inputIds: ['input-1'],
    limits: {
      actions: 40,
      requests: 80,
      scopeMs: 120_000,
      frameBytes: 65_536,
      wireBytes: 1_048_576,
      observationNodes: 128,
      observationTextBytes: 8_192,
      inputBytes: 16_384,
      outputBytes: 16_384,
      screenshotBytes: 262_144,
    },
  };
}

function bootstrap() {
  return {
    protocol: 1,
    policyId: 'supplier-quote',
    policyDigest: 'a'.repeat(64),
    scope: scope(),
    expiresAtMs: Date.now() + 120_000,
    installation: {
      id: 'installation-1',
      browserExecutable: 'C:\\m46-installed\\browser.exe',
      browserSha256: 'b'.repeat(64),
    },
    privateRoot: 'C:\\m46-owned\\task-1',
    inputs: [
      {
        id: 'input-1',
        filePath: 'C:\\m46-owned\\task-1\\inputs\\input-1.csv',
        bytes: Buffer.byteLength('quantity,max_delivery_days\n3,2\n'),
        sha256: 'c'.repeat(64),
      },
    ],
    outputs: [
      {
        id: 'output-csv',
        kind: 'csv',
        filePath: 'C:\\m46-owned\\task-1\\outputs\\output-csv.csv',
      },
      {
        id: 'output-image',
        kind: 'screenshot',
        filePath: 'C:\\m46-owned\\task-1\\outputs\\output-image.jpg',
      },
    ],
  };
}

// Construct the documented bytes independently: this fixture does not depend on
// an unfinished encoder, filesystem installation or browser process.
function frame(message) {
  const payload = Buffer.from(JSON.stringify(message), 'utf8');
  const header = Buffer.alloc(8);
  header.write('ABW1', 0, 'ascii');
  header.writeUInt32BE(payload.length, 4);
  return Buffer.concat([header, payload]);
}

test('m46_valid_host_bootstrap_decodes_to_immutable_operator_policy_without_starting_a_browser', () => {
  const supplied = bootstrap();
  const decoded = parseBootstrap(
    Buffer.from(JSON.stringify(supplied)),
    supplied.privateRoot,
  );
  assert.deepEqual(decoded, supplied);
  for (const value of [
    decoded,
    decoded.scope,
    decoded.scope.limits,
    decoded.scope.routes,
    decoded.scope.routes[0],
    decoded.installation,
    decoded.inputs,
    decoded.inputs[0],
    decoded.outputs,
    decoded.outputs[0],
  ])
    assert.equal(
      Object.isFrozen(value),
      true,
      'admitted policy/bootstrap data must remain immutable',
    );
});

test('m46_valid_abw1_open_frame_decodes_exact_scope_and_accounts_encoded_wire_bytes', () => {
  const supplied = bootstrap();
  const message = {
    protocol: 1,
    id: 1,
    op: 'open',
    payload: {
      scope: supplied.scope,
      policyId: supplied.policyId,
      policyDigest: supplied.policyDigest,
      installationId: supplied.installation.id,
      expiresAtMs: supplied.expiresAtMs,
    },
  };
  const bytes = frame(message);
  const state = { wireBytes: 0, lastIncomingId: 0, lastOutgoingId: 0 };
  const decoded = decodeFrame(bytes, state);
  assert.deepEqual(decoded, message);
  assert.equal(state.wireBytes, bytes.length);
  assert.equal(state.lastIncomingId, 1);
  assert.equal(state.lastOutgoingId, 0);
});

test('m46_finite_core_operation_decode_preserves_literal_type_and_selected_input_identity', () => {
  const selected = scope();
  const literal = '  supplierA "literal quotes"\n空  ';
  const operations = [
    { kind: 'type', node: { token: 'node-1' }, text: literal },
    { kind: 'type', node: { token: 'node-2' }, text: '' },
    { kind: 'upload', node: { token: 'node-3' }, inputId: 'input-1' },
    { kind: 'scroll', deltaY: -2_048 },
  ];
  for (const operation of operations) {
    const decoded = decodeOperation(
      selected,
      Buffer.from(JSON.stringify(operation)),
    );
    assert.deepEqual(decoded, operation);
  }
});

function rejected(callback, code = 'invalid') {
  assert.throws(
    callback,
    (error) => error.code === code && error.effect === 'notDispatched',
  );
}

function openMessage(supplied = bootstrap(), id = 1) {
  return {
    protocol: 1,
    id,
    op: 'open',
    payload: {
      scope: supplied.scope,
      policyId: supplied.policyId,
      policyDigest: supplied.policyDigest,
      installationId: supplied.installation.id,
      expiresAtMs: supplied.expiresAtMs,
    },
  };
}

function frameState() {
  return { wireBytes: 0, lastIncomingId: 0, lastOutgoingId: 0 };
}
function counts(actionsUsed = 0, requestsUsed = 0) {
  return { actionsUsed, requestsUsed, wireBytesUsed: 0 };
}
function opened() {
  const supplied = bootstrap();
  const state = frameState();
  decodeFrame(frame(openMessage(supplied)), state);
  const result = {
    contextId: 'context-1',
    pageId: 'page-1',
    counters: counts(),
  };
  const encoded = encodeFrame(
    { protocol: 1, id: 1, op: 'open', payload: { ok: true, result } },
    state,
  );
  return { supplied, state, encoded };
}
function documentWitness() {
  return {
    documentId: 'document-1',
    documentGeneration: 1,
    observationDigest: 'd'.repeat(64),
  };
}
function executeMessage(operation, id = 2, document = documentWitness()) {
  return {
    protocol: 1,
    id,
    op: 'execute',
    payload: {
      scopeId: 'scope-1',
      scopeRevision: 1,
      contextId: 'context-1',
      operation,
      ...(['navigate', 'observe'].includes(operation.kind)
        ? {}
        : { documentWitness: document }),
    },
  };
}
function closeMessage(id = 3) {
  return {
    protocol: 1,
    id,
    op: 'close',
    payload: { scopeId: 'scope-1', scopeRevision: 1, contextId: 'context-1' },
  };
}
function observation() {
  return {
    documentGeneration: 1,
    routeId: 'index',
    nodes: [
      { node: { token: 'node-1' }, role: 'button', label: 'Select supplier' },
    ],
    shortened: false,
    omittedNodes: 0,
    artifacts: [],
  };
}

test('m46_strict_json_rejects_recursive_duplicates_unknown_null_utf8_and_trailing_values', () => {
  const selected = scope();
  for (const raw of [
    '{"kind":"observe","kind":"observe","screenshot":false}',
    '{"kind":"click","node":{"token":"node-1","t\\u006fken":"node-2"}}',
    '{"kind":"click","node":{"token":"node-1","selector":"body"}}',
    '{"kind":"observe","screenshot":false,"node":{"token":"node-1"}}',
    '{"kind":"type","node":{"token":"node-1"},"text":null}',
    '{"kind":"type","node":{"token":"node-1"},"text":"\\ud800"}',
    '{"kind":"observe","screenshot":false} true',
    '{"kind":"observe","screenshot":false,}',
    '{"kind":"evaluate","script":"untrusted-private-sentinel"}',
    '[]',
    'null',
  ])
    rejected(() => decodeOperation(selected, Buffer.from(raw)));
  rejected(() => decodeOperation(selected, Buffer.from([0xff])));
  rejected(() =>
    decodeOperation(
      selected,
      Buffer.concat([
        Buffer.from([0xef, 0xbb, 0xbf]),
        Buffer.from('{"kind":"observe","screenshot":false}'),
      ]),
    ),
  );
  try {
    decodeOperation(
      selected,
      Buffer.from('{"kind":"evaluate","privatePath":"private-sentinel"}'),
    );
  } catch (failure) {
    assert.equal(failure.message.includes('private-sentinel'), false);
    assert.equal(failure.stack.includes('private-sentinel'), false);
  }
});

test('m46_strict_json_enforces_depth_node_count_and_raw_tighter_frame_limits', () => {
  rejected(
    () =>
      decodeOperation(
        scope(),
        Buffer.from('{"unknown":' + '['.repeat(8) + '0' + ']'.repeat(8) + '}'),
      ),
    'limitsExceeded',
  );
  rejected(
    () =>
      decodeOperation(
        scope(),
        Buffer.from(JSON.stringify({ unknown: Array(2048).fill(false) })),
      ),
    'limitsExceeded',
  );
  const selected = scope();
  selected.limits.frameBytes = 1024;
  const raw = Buffer.from(
    JSON.stringify({ kind: 'observe', screenshot: false }).padEnd(1024, ' '),
  );
  assert.deepEqual(decodeOperation(selected, raw), {
    kind: 'observe',
    screenshot: false,
  });
  rejected(
    () => decodeOperation(selected, Buffer.concat([raw, Buffer.from(' ')])),
    'limitsExceeded',
  );
  const encoded = Buffer.from(
    JSON.stringify({
      kind: 'type',
      node: { token: 'node-1' },
      text: 'π'.repeat(2048),
    }),
  );
  assert.equal(decodeOperation(scope(), encoded).text.length, 2048);
  rejected(
    () =>
      decodeOperation(
        scope(),
        Buffer.from(
          JSON.stringify({
            kind: 'type',
            node: { token: 'node-1' },
            text: 'π'.repeat(2048) + 'a',
          }),
        ),
      ),
    'limitsExceeded',
  );
});

test('m46_scope_validation_matches_core_exact_origins_routes_ids_and_caps', () => {
  for (const origin of [
    'http://localhost:43219',
    'http://127.1:43219',
    'https://127.0.0.1:43219',
    'http://127.0.0.1:043219',
    'http://127.0.0.1:65536',
    'http://127.0.0.1:0',
    'http://127.0.0.1:43219/',
    'http://127.0.0.1:43219\n',
    'http://user@127.0.0.1:43219',
  ]) {
    const changed = scope();
    changed.origin = origin;
    rejected(() => validateScope(changed));
  }
  for (const path of [
    '//sink',
    '/a//b',
    '/.',
    '/..',
    '/a/../b',
    '/%2e',
    '/a?q=1',
    '/a#x',
    '/a\\b',
    '/a\n',
    '/π',
  ]) {
    const changed = scope();
    changed.routes[0].path = path;
    rejected(() => validateScope(changed));
  }
  for (const id of ['', 'node\n', 'a'.repeat(65), 'π', '../node']) {
    const changed = scope();
    changed.id = id;
    rejected(() => validateScope(changed));
  }
  for (const revision of [0, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
    const changed = scope();
    changed.revision = revision;
    rejected(() => validateScope(changed));
  }
  const duplicate = scope();
  duplicate.routes[1].id = duplicate.routes[0].id;
  rejected(() => validateScope(duplicate));
  const duplicatePair = scope();
  duplicatePair.routes[1] = { ...duplicatePair.routes[0], id: 'other' };
  rejected(() => validateScope(duplicatePair));
  const duplicateInput = scope();
  duplicateInput.inputIds.push('input-1');
  rejected(() => validateScope(duplicateInput));
  for (const key of Object.keys(scope().limits)) {
    const changed = scope();
    changed.limits[key]++;
    rejected(() => validateScope(changed), 'limitsExceeded');
  }
  const zero = scope();
  zero.limits.actions = 0;
  rejected(() => validateScope(zero));
  const inverted = scope();
  inverted.limits.wireBytes--;
  rejected(() =>
    validateScope({
      ...inverted,
      limits: { ...inverted.limits, wireBytes: 65535 },
    }),
  );
  const unknown = scope();
  unknown.routes[0].hostPath = 'private-sentinel';
  rejected(() => validateScope(unknown));
});

test('m46_finite_operations_refuse_foreign_selection_mixed_variants_and_scroll_escape', () => {
  for (const operation of [
    { kind: 'navigate', routeId: 'foreign' },
    { kind: 'navigate', routeId: 'submit' },
    { kind: 'upload', node: { token: 'node-1' }, inputId: 'foreign' },
  ])
    rejected(
      () => decodeOperation(scope(), Buffer.from(JSON.stringify(operation))),
      'denied',
    );
  for (const operation of [
    { kind: 'scroll', deltaY: 0 },
    { kind: 'scroll', deltaY: 2049 },
    { kind: 'scroll', deltaY: -2049 },
    { kind: 'scroll', deltaY: 0.5 },
    { kind: 'click', node: { token: 'node-1', selector: 'body' } },
    { kind: 'type', node: { token: 'node-1' }, text: '\ud800' },
  ])
    rejected(() =>
      decodeOperation(scope(), Buffer.from(JSON.stringify(operation))),
    );
});

test('m46_bootstrap_refuses_ambient_paths_aliases_foreign_inputs_and_recursive_authority_fields', () => {
  for (const path of [
    'relative.csv',
    '\\\\server\\share\\file.csv',
    '\\\\?\\C:\\m46-owned\\task-1\\input.csv',
    'C:\\m46-owned\\task-10\\input.csv',
    'C:\\m46-owned\\task-1\\..\\outside.csv',
    'C:\\m46-owned\\task-1\\input.csv:stream',
    'C:\\m46-owned\\task-1\\NUL.csv',
    'C:\\m46-owned\\task-1\\input.csv.',
  ]) {
    const changed = bootstrap();
    changed.inputs[0].filePath = path;
    assert.throws(
      () =>
        parseBootstrap(
          Buffer.from(JSON.stringify(changed)),
          changed.privateRoot,
        ),
      (error) => ['invalid', 'denied'].includes(error.code),
    );
  }
  const paths = bootstrap();
  paths.outputs[0].filePath = paths.inputs[0].filePath.toUpperCase();
  rejected(() =>
    parseBootstrap(Buffer.from(JSON.stringify(paths)), paths.privateRoot),
  );
  const duplicateId = bootstrap();
  duplicateId.outputs[0].id = 'input-1';
  rejected(() =>
    parseBootstrap(
      Buffer.from(JSON.stringify(duplicateId)),
      duplicateId.privateRoot,
    ),
  );
  const foreign = bootstrap();
  foreign.scope.inputIds.push('foreign');
  rejected(
    () =>
      parseBootstrap(Buffer.from(JSON.stringify(foreign)), foreign.privateRoot),
    'denied',
  );
  const relativeBrowser = bootstrap();
  relativeBrowser.installation.browserExecutable = 'browser.exe';
  rejected(() =>
    parseBootstrap(
      Buffer.from(JSON.stringify(relativeBrowser)),
      relativeBrowser.privateRoot,
    ),
  );
  const flags = bootstrap();
  flags.installation.flags = ['--no-sandbox'];
  rejected(() =>
    parseBootstrap(Buffer.from(JSON.stringify(flags)), flags.privateRoot),
  );
  const unknownSlot = bootstrap();
  unknownSlot.outputs[0].suggestedFilename = 'private-sentinel';
  rejected(() =>
    parseBootstrap(
      Buffer.from(JSON.stringify(unknownSlot)),
      unknownSlot.privateRoot,
    ),
  );
  const valid = bootstrap();
  rejected(
    () =>
      parseBootstrap(
        Buffer.from(JSON.stringify(valid)),
        'C:\\m46-owned\\foreign',
      ),
    'denied',
  );
  const raw = JSON.stringify(valid).replace(
    '"browserSha256":',
    '"id":"duplicate","browserSha256":',
  );
  rejected(() => parseBootstrap(Buffer.from(raw), valid.privateRoot));
});

test('m46_bootstrap_and_open_bind_all_immutable_policy_installation_and_expiry_fields', () => {
  const supplied = bootstrap();
  const immutable = parseBootstrap(
    Buffer.from(JSON.stringify(supplied)),
    supplied.privateRoot,
  );
  const message = openMessage(supplied);
  assert.deepEqual(
    validateOpen(immutable, message, supplied.expiresAtMs - 120000),
    message.payload,
  );
  for (const mutation of [
    (value) => {
      value.payload.policyId = 'other';
    },
    (value) => {
      value.payload.policyDigest = 'e'.repeat(64);
    },
    (value) => {
      value.payload.installationId = 'other';
    },
    (value) => {
      value.payload.expiresAtMs++;
    },
    (value) => {
      value.payload.scope.routes.reverse();
    },
  ]) {
    const changed = structuredClone(message);
    mutation(changed);
    rejected(
      () => validateOpen(immutable, changed, supplied.expiresAtMs - 120000),
      'stale',
    );
  }
  rejected(
    () => validateOpen(immutable, message, supplied.expiresAtMs),
    'deadline',
  );
  rejected(() =>
    validateOpen(immutable, message, supplied.expiresAtMs - 120001),
  );
  assert.throws(() => {
    immutable.scope.routes[0].path = '/other';
  }, TypeError);
});

test('m46_abw1_rejects_bad_headers_lengths_unsafe_ids_and_duplicates_after_counting_raw_bytes', () => {
  const valid = frame(openMessage());
  const wrongMagic = Buffer.from(valid);
  wrongMagic.write('XYZ1');
  const maskedMagic = Buffer.from(valid);
  maskedMagic[0] |= 0x80;
  const zeroLength = Buffer.from(valid);
  zeroLength.writeUInt32BE(0, 4);
  const wrongLength = Buffer.from(valid);
  wrongLength.writeUInt32BE(valid.length, 4);
  const hugeHeader = Buffer.alloc(8);
  hugeHeader.write('ABW1');
  hugeHeader.writeUInt32BE(0xffffffff, 4);
  for (const raw of [
    wrongMagic,
    maskedMagic,
    zeroLength,
    wrongLength,
    valid.subarray(0, 4),
    Buffer.concat([valid, Buffer.from('x')]),
  ]) {
    const state = frameState();
    rejected(() => decodeFrame(raw, state), 'protocol');
    assert.equal(state.wireBytes, raw.length);
  }
  const readerState = frameState();
  const reader = new FrameReader(readerState);
  rejected(() => reader.push(hugeHeader), 'limitsExceeded');
  assert.equal(readerState.wireBytes, 8);
  assert.equal(reader.buffer.length, 0);
  for (const id of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
    rejected(() =>
      decodeFrame(frame(openMessage(bootstrap(), id)), frameState()),
    );
  }
  const floatingId = frame(openMessage())
    .toString('utf8', 8)
    .replace('"id":1,', '"id":1.0,');
  const floatingFrame = Buffer.from(floatingId);
  const floatingHeader = Buffer.alloc(8);
  floatingHeader.write('ABW1');
  floatingHeader.writeUInt32BE(floatingFrame.length, 4);
  rejected(() =>
    decodeFrame(Buffer.concat([floatingHeader, floatingFrame]), frameState()),
  );
  const state = frameState();
  decodeFrame(valid, state);
  rejected(() => decodeFrame(valid, state), 'protocol');
  assert.equal(state.wireBytes, valid.length * 2);
  const unknown = openMessage();
  unknown.payload.scope.limits.env = 'private-sentinel';
  rejected(() => decodeFrame(frame(unknown), frameState()));
});

test('m46_incremental_frame_reader_counts_partial_bytes_once_and_refuses_partial_eof', () => {
  const raw = frame(openMessage());
  const state = frameState();
  const reader = new FrameReader(state);
  assert.deepEqual(reader.push(raw.subarray(0, 3)), []);
  assert.equal(state.wireBytes, 3);
  assert.deepEqual(reader.push(raw.subarray(3, 8)), []);
  assert.equal(state.wireBytes, 8);
  const messages = reader.push(raw.subarray(8));
  assert.equal(messages.length, 1);
  assert.equal(messages[0].op, 'open');
  assert.equal(state.wireBytes, raw.length);
  reader.finish();
  const byteState = frameState();
  const byteReader = new FrameReader(byteState);
  const decoded = [];
  for (let at = 0; at < raw.length; at++)
    decoded.push(...byteReader.push(raw.subarray(at, at + 1)));
  assert.equal(decoded.length, 1);
  assert.equal(byteState.wireBytes, raw.length);
  byteReader.finish();
  const partialState = frameState();
  const partial = new FrameReader(partialState);
  partial.push(raw.subarray(0, 9));
  rejected(() => partial.finish(), 'protocol');
  assert.equal(partialState.wireBytes, 9);
  rejected(() => partial.push(raw.subarray(9)), 'protocol');
});

test('m46_echoed_reply_accounts_both_directions_and_never_accepts_unsolicited_or_unknown_output', () => {
  const { state, encoded } = opened();
  const response = JSON.parse(encoded.subarray(8).toString('utf8'));
  assert.equal(response.id, 1);
  assert.equal(response.payload.result.counters.wireBytesUsed, state.wireBytes);
  assert.equal(state.lastIncomingId, 1);
  assert.equal(state.lastOutgoingId, 1);
  rejected(() => encodeFrame(response, state), 'protocol');
  const active = opened();
  decodeFrame(
    frame(executeMessage({ kind: 'observe', screenshot: false })),
    active.state,
  );
  const view = observation();
  const result = {
    contextId: 'context-1',
    pageId: 'page-1',
    document: {
      documentId: 'document-1',
      documentGeneration: 1,
      observationDigest: canonicalDigest(view),
    },
    observation: view,
    counters: counts(1),
  };
  const valid = {
    protocol: 1,
    id: 2,
    op: 'execute',
    payload: { ok: true, result },
  };
  const wrongEcho = structuredClone(valid);
  wrongEcho.id = 3;
  rejected(() => encodeFrame(wrongEcho, active.state), 'protocol');
  const wrongOp = structuredClone(valid);
  wrongOp.op = 'close';
  rejected(() => encodeFrame(wrongOp, active.state), 'protocol');
  const rawDisclosure = structuredClone(valid);
  rawDisclosure.payload.result.observation.artifacts.push({
    id: 'image',
    kind: 'screenshot',
    bytes: 1,
    sha256: 'a'.repeat(64),
    base64: 'private-sentinel',
  });
  rejected(() => encodeFrame(rawDisclosure, active.state));
  const undefinedUnknown = structuredClone(valid);
  undefinedUnknown.hostPath = undefined;
  rejected(() => encodeFrame(undefinedUnknown, active.state));
  const wrongHash = structuredClone(valid);
  wrongHash.payload.result.document.observationDigest = 'e'.repeat(64);
  rejected(() => encodeFrame(wrongHash, active.state), 'stale');
  const output = encodeFrame(valid, active.state);
  assert.equal(
    JSON.parse(output.subarray(8).toString()).payload.result.counters
      .wireBytesUsed,
    active.state.wireBytes,
  );
});

test('m46_close_can_interrupt_one_operation_but_replies_remain_ordered_and_context_cannot_reopen', () => {
  const { state } = opened();
  decodeFrame(
    frame(executeMessage({ kind: 'observe', screenshot: false })),
    state,
  );
  decodeFrame(frame(closeMessage()), state);
  const close = {
    protocol: 1,
    id: 3,
    op: 'close',
    payload: { ok: true, result: { engineClosed: true, counters: counts(1) } },
  };
  rejected(() => encodeFrame(close, state), 'protocol');
  encodeFrame(
    {
      protocol: 1,
      id: 2,
      op: 'execute',
      payload: {
        ok: false,
        error: { code: 'cancelled', effect: 'mayHaveOccurred' },
        counters: counts(1),
      },
    },
    state,
  );
  const encoded = encodeFrame(close, state);
  assert.equal(
    JSON.parse(encoded.subarray(8).toString()).payload.result.engineClosed,
    true,
  );
  rejected(
    () => decodeFrame(frame(openMessage(bootstrap(), 4)), state),
    'protocol',
  );
  rejected(
    () =>
      decodeFrame(
        frame(executeMessage({ kind: 'observe', screenshot: false }, 5)),
        state,
      ),
    'protocol',
  );
  decodeFrame(frame(closeMessage(6)), state);
  encodeFrame({ ...close, id: 6 }, state);
});

test('m46_framing_refuses_unacknowledged_open_parallel_actions_mixed_witnesses_and_foreign_context', () => {
  const pending = frameState();
  decodeFrame(frame(openMessage()), pending);
  rejected(
    () =>
      decodeFrame(
        frame(executeMessage({ kind: 'observe', screenshot: false })),
        pending,
      ),
    'protocol',
  );
  const { state } = opened();
  const unwanted = executeMessage({ kind: 'observe', screenshot: false });
  unwanted.payload.documentWitness = documentWitness();
  rejected(() => decodeFrame(frame(unwanted), state));
  const missing = executeMessage({ kind: 'click', node: { token: 'node-1' } });
  delete missing.payload.documentWitness;
  rejected(() => decodeFrame(frame(missing), state));
  const foreign = executeMessage({ kind: 'observe', screenshot: false });
  foreign.payload.contextId = 'foreign';
  rejected(() => decodeFrame(frame(foreign), state), 'stale');
  decodeFrame(
    frame(executeMessage({ kind: 'observe', screenshot: false })),
    state,
  );
  rejected(
    () =>
      decodeFrame(
        frame(executeMessage({ kind: 'observe', screenshot: false }, 3)),
        state,
      ),
    'protocol',
  );
});

test('m46_session_wire_cap_includes_rejected_bytes_and_encoded_reply_header', () => {
  const supplied = bootstrap();
  supplied.scope.limits.wireBytes = 65536;
  const state = frameState();
  decodeFrame(frame(openMessage(supplied)), state);
  const reply = {
    protocol: 1,
    id: 1,
    op: 'open',
    payload: {
      ok: true,
      result: { contextId: 'context-1', pageId: 'page-1', counters: counts() },
    },
  };
  state.wireBytes = 65530;
  rejected(() => encodeFrame(reply, state), 'limitsExceeded');
  assert.equal(state.lastOutgoingId, 0);
  const inputState = frameState();
  rejected(
    () => decodeFrame(Buffer.alloc(LIMITS.wireBytes + 1), inputState),
    'limitsExceeded',
  );
  assert.equal(inputState.wireBytes, LIMITS.wireBytes + 1);
});

test('m46_current_document_witness_and_selected_node_are_required_before_effect_intent', () => {
  const supplied = bootstrap();
  const message = executeMessage({
    kind: 'type',
    node: { token: 'node-1' },
    text: '',
  });
  const current = {
    contextId: 'context-1',
    document: documentWitness(),
    nodeTokens: new Set(['node-1']),
  };
  assert.deepEqual(
    validateExecute(supplied, message, current, supplied.expiresAtMs - 1),
    message.payload.operation,
  );
  for (const change of [
    (value) => {
      value.contextId = 'other';
    },
    (value) => {
      value.document.documentId = 'other';
    },
    (value) => {
      value.document.documentGeneration++;
    },
    (value) => {
      value.document.observationDigest = 'e'.repeat(64);
    },
    (value) => {
      value.nodeTokens = new Set();
    },
  ]) {
    const changed = structuredClone(current);
    change(changed);
    rejected(
      () =>
        validateExecute(supplied, message, changed, supplied.expiresAtMs - 1),
      'stale',
    );
  }
  rejected(
    () => validateExecute(supplied, message, current, supplied.expiresAtMs),
    'deadline',
  );
});

test('m46_observation_drops_whole_oversized_nodes_with_honest_omitted_count_and_literal_labels', () => {
  const selected = scope();
  selected.limits.observationTextBytes = 32;
  selected.limits.observationNodes = 2;
  const source = observation();
  source.nodes = [
    { node: { token: 'huge' }, role: 'button', label: 'π'.repeat(20) },
    { node: { token: 'one' }, role: 'button', label: '  literal 空  ' },
    { node: { token: 'two' }, role: 'button', label: 'x' },
    { node: { token: 'three' }, role: 'button', label: '' },
  ];
  const bounded = boundObservation(selected, source);
  assert.deepEqual(
    bounded.nodes.map((node) => node.node.token),
    ['one', 'two'],
  );
  assert.equal(bounded.nodes[0].label, '  literal 空  ');
  assert.equal(bounded.omittedNodes, 2);
  assert.equal(bounded.shortened, true);
  assert.equal(Object.isFrozen(bounded.nodes[0].node), true);
  assert.equal(source.nodes.length, 4);
  const duplicate = observation();
  duplicate.nodes.push(duplicate.nodes[0]);
  rejected(() => validateObservation(scope(), duplicate));
  const misleading = observation();
  misleading.shortened = true;
  rejected(() => boundObservation(scope(), misleading));
  const withArtifacts = observation();
  withArtifacts.artifacts = [
    { id: 'csv', kind: 'csv', bytes: 16384, sha256: 'a'.repeat(64) },
    { id: 'image', kind: 'screenshot', bytes: 262144, sha256: 'b'.repeat(64) },
  ];
  validateObservation(scope(), withArtifacts);
  withArtifacts.artifacts[0].bytes++;
  rejected(() => validateObservation(scope(), withArtifacts), 'limitsExceeded');
});

test('m46_observation_digest_matches_core_jcs_and_preserves_array_order', () => {
  assert.equal(
    canonicalDigest({ kind: 'observe', screenshot: false }),
    '3d8917e55df5c45d96407b638e07a2ed462e99e23af2680f7e14ec896a9b6faf',
  );
  const selected = scope();
  assert.equal(
    canonicalDigest(selected),
    canonicalDigest(Object.fromEntries(Object.entries(selected).reverse())),
  );
  const reverse = scope();
  reverse.routes.reverse();
  assert.notEqual(canonicalDigest(selected), canonicalDigest(reverse));
  assert.notEqual(
    canonicalDigest({ kind: 'type', node: { token: 'one' }, text: '' }),
    canonicalDigest({ kind: 'type', node: { token: 'one' }, text: ' ' }),
  );
});

function requestWindow() {
  return {
    requestsUsed: 0,
    blockedRequests: 0,
    pageId: 'page-1',
    effectActive: true,
    navigationRouteId: 'index',
    navigationRemaining: 1,
    downloadRouteId: 'csv',
    nowMs: 1000,
    expiresAtMs: 2000,
  };
}
function intercepted(overrides = {}) {
  return {
    pageId: 'page-1',
    isMainFrame: true,
    channel: 'http',
    method: 'GET',
    url: scope().origin + '/',
    navigation: false,
    download: false,
    bodyBytes: 0,
    headerBytes: 20,
    ...overrides,
  };
}

test('m46_request_guard_denies_every_nonexact_destination_method_page_channel_and_counts_actual_refusals', () => {
  for (const change of [
    { url: scope().origin + '/?x=1' },
    { url: scope().origin + '/#x' },
    { url: scope().origin + '/%2e' },
    { url: 'http://localhost:43219/' },
    { url: 'http://127.0.0.1:43220/' },
    { url: 'file:///private-sentinel' },
    { url: scope().origin + '/undeclared' },
    { method: 'DELETE' },
    { pageId: 'popup' },
    { isMainFrame: false },
    { channel: 'websocket' },
  ]) {
    const window = requestWindow();
    rejected(
      () => guardRequest(scope(), intercepted(change), window),
      'denied',
    );
    assert.equal(window.requestsUsed, 1);
    assert.equal(window.blockedRequests, 1);
  }
  const window = requestWindow();
  assert.deepEqual(
    guardRequest(scope(), intercepted(), window),
    scope().routes[0],
  );
  assert.equal(window.requestsUsed, 1);
  assert.equal(window.blockedRequests, 0);
});

test('m46_request_guard_requires_active_post_download_and_single_navigation_windows_and_shared_budget', () => {
  const post = intercepted({
    method: 'POST',
    url: scope().origin + '/quote',
    bodyBytes: 1024,
  });
  const idle = requestWindow();
  idle.effectActive = false;
  rejected(() => guardRequest(scope(), post, idle), 'denied');
  const active = requestWindow();
  assert.equal(guardRequest(scope(), post, active).id, 'submit');
  const nav = requestWindow();
  guardRequest(scope(), intercepted({ navigation: true }), nav);
  assert.equal(nav.navigationRemaining, 0);
  rejected(
    () => guardRequest(scope(), intercepted({ navigation: true }), nav),
    'denied',
  );
  const wrongDownload = requestWindow();
  wrongDownload.downloadRouteId = undefined;
  rejected(
    () =>
      guardRequest(
        scope(),
        intercepted({ url: scope().origin + '/quote.csv', download: true }),
        wrongDownload,
      ),
    'denied',
  );
  for (const change of [{ bodyBytes: 1 }, { headerBytes: 8193 }]) {
    rejected(
      () => guardRequest(scope(), intercepted(change), requestWindow()),
      'limitsExceeded',
    );
  }
  const exhausted = requestWindow();
  exhausted.requestsUsed = 80;
  rejected(
    () => guardRequest(scope(), intercepted(), exhausted),
    'limitsExceeded',
  );
  assert.equal(exhausted.requestsUsed, 81);
  assert.equal(exhausted.blockedRequests, 1);
  const stopped = requestWindow();
  stopped.signal = AbortSignal.abort();
  rejected(() => guardRequest(scope(), intercepted(), stopped), 'cancelled');
  const expired = requestWindow();
  expired.nowMs = expired.expiresAtMs;
  rejected(() => guardRequest(scope(), intercepted(), expired), 'deadline');
});

function streamResponse(chunks, options = {}) {
  let cancelled = 0;
  let pulls = 0;
  const pending = chunks.map((chunk) => Buffer.from(chunk));
  const body = new ReadableStream({
    pull(controller) {
      pulls++;
      if (pending.length) controller.enqueue(pending.shift());
      else controller.close();
    },
    cancel() {
      cancelled++;
    },
  });
  return {
    response: new Response(body, { status: 200, ...options }),
    cancelled: () => cancelled,
    pulls: () => pulls,
  };
}
function responseControl(overrides = {}) {
  return {
    aggregateBytes: 0,
    deadlineMs: Date.now() + 1000,
    diagnostics: { blockedRequests: 0, redirectsRejected: 0 },
    ...overrides,
  };
}
function responseLimits(overrides = {}) {
  return {
    kind: 'asset',
    bodyBytes: 65536,
    totalBytes: 1048576,
    allowAttachment: false,
    ...overrides,
  };
}

test('m46_bounded_response_rejects_all_redirects_before_body_read_or_fulfillment', async () => {
  for (const status of [300, 301, 302, 303, 307, 308, 399]) {
    let read = 0;
    let cancelled = 0;
    let aborted = 0;
    const response = {
      status,
      headers: new Headers({ location: scope().origin + '/' }),
      body: {
        getReader() {
          read++;
          throw new Error('must not read');
        },
        cancel() {
          cancelled++;
          return Promise.resolve();
        },
      },
    };
    const control = responseControl({
      abort() {
        aborted++;
      },
    });
    await assert.rejects(
      readBoundedResponse(response, responseLimits(), control),
      (error) => error.code === 'denied',
    );
    assert.equal(read, 0);
    assert.equal(cancelled, 1);
    assert.equal(aborted, 1);
    assert.equal(control.aggregateBytes, 0);
    assert.equal(control.diagnostics.redirectsRejected, 1);
    assert.equal(control.diagnostics.blockedRequests, 1);
  }
});

test('m46_bounded_response_caps_streamed_asset_csv_and_session_bytes_without_partial_publication', async () => {
  let aborts = 0;
  const over = streamResponse(['abc', 'def', 'ghi']);
  const control = responseControl({
    abort() {
      aborts++;
    },
  });
  await assert.rejects(
    readBoundedResponse(
      over.response,
      responseLimits({ bodyBytes: 5 }),
      control,
    ),
    (error) => error.code === 'limitsExceeded',
  );
  assert.equal(aborts, 1);
  assert.equal(over.cancelled(), 1);
  assert.equal(control.aggregateBytes, 6);
  const session = streamResponse(['1234', '56']);
  const aggregate = responseControl({ aggregateBytes: 1048572 });
  await assert.rejects(
    readBoundedResponse(session.response, responseLimits(), aggregate),
    (error) => error.code === 'limitsExceeded',
  );
  assert.equal(aggregate.aggregateBytes, 1048578);
  const csv = streamResponse([Buffer.alloc(16384), Buffer.alloc(1)], {
    headers: { 'content-type': 'text/csv' },
  });
  await assert.rejects(
    readBoundedResponse(
      csv.response,
      responseLimits({ kind: 'csv', bodyBytes: 65536 }),
      responseControl(),
    ),
    (error) => error.code === 'limitsExceeded',
  );
  const good = streamResponse(['literal\n', 'bytes'], {
    headers: {
      'content-type': 'text/plain',
      'set-cookie': 'secret=unused',
      'content-security-policy': "default-src 'self'",
    },
  });
  const output = await readBoundedResponse(
    good.response,
    responseLimits(),
    responseControl(),
  );
  assert.equal(output.body.toString(), 'literal\nbytes');
  assert.deepEqual(output.headers, {
    'content-type': 'text/plain',
    'content-security-policy': "default-src 'self'",
  });
  assert.equal(good.cancelled(), 0);
});

test('m46_bounded_response_refuses_unsolicited_attachments_wrong_csv_type_and_announced_oversize', async () => {
  for (const headers of [
    { 'content-disposition': 'attachment; filename=private-sentinel.csv' },
    { 'content-length': '65537' },
    { 'content-length': '0001' },
  ]) {
    const value = streamResponse(['body'], { headers });
    await assert.rejects(
      readBoundedResponse(value.response, responseLimits(), responseControl()),
      (error) => ['denied', 'limitsExceeded'].includes(error.code),
    );
    assert.equal(value.cancelled(), 1);
  }
  const wrongType = streamResponse(['<html>'], {
    headers: { 'content-type': 'text/html' },
  });
  await assert.rejects(
    readBoundedResponse(
      wrongType.response,
      responseLimits({ kind: 'csv', bodyBytes: 16384, allowAttachment: true }),
      responseControl(),
    ),
    (error) => error.code === 'denied',
  );
  const correct = streamResponse(['supplier,quantity\nsupplierA,3\n'], {
    headers: {
      'content-type': 'text/csv; charset=utf-8',
      'content-disposition': 'attachment',
    },
  });
  const result = await readBoundedResponse(
    correct.response,
    responseLimits({ kind: 'csv', bodyBytes: 16384, allowAttachment: true }),
    responseControl(),
  );
  assert.equal(result.body.toString(), 'supplier,quantity\nsupplierA,3\n');
  assert.equal(Object.hasOwn(result.headers, 'content-disposition'), false);
});

test('m46_bounded_response_stop_interrupts_pending_read_and_deadline_returns_static_error', async () => {
  const controller = new AbortController();
  let cancelled = 0;
  let aborted = 0;
  const body = new ReadableStream({
    pull() {
      return new Promise(() => {});
    },
    cancel() {
      cancelled++;
    },
  });
  const response = new Response(body);
  const reading = readBoundedResponse(
    response,
    responseLimits(),
    responseControl({
      signal: controller.signal,
      abort() {
        aborted++;
      },
    }),
  );
  queueMicrotask(() => controller.abort());
  await assert.rejects(
    reading,
    (error) => error.code === 'cancelled' && !error.message.includes('private'),
  );
  assert.equal(cancelled, 1);
  assert.equal(aborted, 1);
  const expired = streamResponse(['private-sentinel']);
  await assert.rejects(
    readBoundedResponse(
      expired.response,
      responseLimits(),
      responseControl({ deadlineMs: Date.now() - 1 }),
    ),
    (error) => error.code === 'deadline',
  );
  assert.equal(expired.cancelled(), 1);
});

const pinnedYaml = createRequire(import.meta.url)(
  'playwright-core/lib/utilsBundle',
).yaml;

test('m46_cli_accepts_only_fixed_private_bootstrap_and_cleared_private_environment', async () => {
  const cwd = bootstrap().privateRoot;
  const file = cwd + '\\bootstrap.json';
  assert.equal(parseCliArguments(['--bootstrap', file], cwd), file);
  for (const args of [
    [],
    ['--bootstrap'],
    ['--url', 'http://127.0.0.1:43219/'],
    ['--bootstrap', file, '--no-sandbox'],
    ['--bootstrap', 'C:\\outside\\bootstrap.json'],
    ['--bootstrap', 'bootstrap.json'],
  ])
    assert.throws(
      () => parseCliArguments(args, cwd),
      (error) => ['invalid', 'denied'].includes(error.code),
    );
  const env = {
    SystemRoot: 'C:\\Windows',
    WINDIR: 'C:\\Windows',
    TEMP: cwd + '\\tmp',
    TMP: cwd + '\\tmp',
  };
  assert.deepEqual(validateWorkerEnvironment(env, cwd), env);
  for (const changed of [
    { ...env, PATH: 'private-sentinel' },
    { ...env, NODE_OPTIONS: '--require private-sentinel' },
    { ...env, USERPROFILE: 'C:\\Users\\ordinary-profile' },
    { ...env, TEMP: 'C:\\outside' },
  ])
    assert.throws(() => validateWorkerEnvironment(changed, cwd));
  const written = [];
  const output = new Writable({
    write(chunk, encoding, done) {
      written.push(Buffer.from(chunk));
      done();
    },
  });
  assert.equal(
    await runCli({
      argv: ['--url', 'private-sentinel'],
      cwd,
      environment: env,
      output,
    }),
    1,
  );
  assert.deepEqual(written, []);
});

test(
  'm46_windows_node22_cli_real_private_bootstrap_and_eof_exits_cleanly_without_open_or_output',
  { skip: process.platform !== 'win32' || process.versions.node !== '22.12.0' },
  async () => {
    const cwd = await filesystem.mkdtemp(path.join(tmpdir(), 'anima-m46-cli-'));
    try {
      const criteria = Buffer.from('quantity,max_delivery_days\n3,2\n');
      const supplied = bootstrap();
      supplied.privateRoot = cwd;
      supplied.installation.browserExecutable = path.join(
        cwd,
        'not-installed.exe',
      );
      supplied.inputs = [
        {
          id: 'input-1',
          filePath: path.join(cwd, 'input.csv'),
          bytes: criteria.length,
          sha256: createHash('sha256').update(criteria).digest('hex'),
        },
      ];
      supplied.outputs = [
        {
          id: 'output-csv',
          kind: 'csv',
          filePath: path.join(cwd, 'output.csv'),
        },
        {
          id: 'output-image',
          kind: 'screenshot',
          filePath: path.join(cwd, 'output.jpg'),
        },
      ];
      const bootstrapPath = path.join(cwd, 'bootstrap.json');
      await filesystem.writeFile(supplied.inputs[0].filePath, criteria, {
        flag: 'wx',
      });
      await filesystem.writeFile(bootstrapPath, JSON.stringify(supplied), {
        flag: 'wx',
      });
      const input = new PassThrough();
      input.end(); // No Open frame: file admission must not construct an engine.
      const written = [];
      const output = new Writable({
        write(chunk, encoding, done) {
          written.push(Buffer.from(chunk));
          done();
        },
      });
      const exit = await runCli({
        argv: ['--bootstrap', bootstrapPath],
        cwd,
        environment: {
          SystemRoot: process.env.SystemRoot,
          WINDIR: process.env.WINDIR,
          TEMP: cwd,
          TMP: cwd,
        },
        input,
        output,
      });
      assert.equal(
        exit,
        0,
        'valid real private files plus EOF must pass CLI admission',
      );
      assert.deepEqual(
        written,
        [],
        'EOF without Open must publish no protocol output',
      );
    } finally {
      await filesystem.rm(cwd, { recursive: true, force: true });
    }
  },
);

test('m46_pinned_yaml_snapshot_adaptation_preserves_names_refs_and_honest_omissions', () => {
  const snapshot = [
    '- main:',
    '  - button "Upload criteria" [ref=e1]',
    '  - textbox "Literal \\"quotes\\"" [active] [ref=e2]',
    '  - button [ref=e3]',
    '  - text: supplierA,3,12.50,37.50,2',
    '  - link "Download CSV" [ref=e4]:',
    '    - /url: /quote.csv',
  ].join('\n');
  const evidence = snapshotEvidence(snapshot, pinnedYaml);
  assert.equal(evidence.omittedNodes, 2);
  assert.deepEqual(evidence.entries, [
    { role: 'button', label: 'Upload criteria', ref: 'e1' },
    { role: 'textbox', label: 'Literal "quotes"', ref: 'e2' },
    { role: 'text', label: 'supplierA,3,12.50,37.50,2' },
    { role: 'link', label: 'Download CSV', ref: 'e4' },
  ]);
  const quoted = snapshotEvidence(
    "- 'button \"A: B''s\" [ref=e1]'",
    pinnedYaml,
  );
  assert.equal(quoted.entries[0].label, "A: B's");
  for (const raw of [
    '- &alias button "One"\n- *alias',
    '- button "One" [ref=body]',
    '- button "One" [ref=e1] [selector=body]',
    '- !untrusted tag',
    'plain-root',
  ])
    assert.throws(
      () => snapshotEvidence(raw, pinnedYaml),
      (error) => ['invalid', 'unavailable'].includes(error.code),
    );
  rejected(
    () => snapshotEvidence('x'.repeat(65537), pinnedYaml),
    'limitsExceeded',
  );
});

function adapterSetupSpy(overrides = {}) {
  const order = [];
  const hooks = {};
  const page = new EventEmitter();
  const frame = { page: () => page, url: () => 'about:blank' };
  page.mainFrame = () => frame;
  page.url = () => 'about:blank';
  page.evaluate = async () => ({
    token: 'a'.repeat(32),
    revision: 0,
    same: true,
    ready: true,
  });
  page.close = async () => {
    order.push('unexpected-page-close');
  };
  const context = new EventEmitter();
  context.setDefaultTimeout = (value) => {
    assert.equal(value, 10000);
    order.push('timeout');
  };
  context.exposeBinding = async (name, callback) => {
    order.push('binding');
    hooks.binding = callback;
  };
  context.addInitScript = async (script, args) => {
    order.push('init-script');
    assert.equal(typeof script, 'function');
    assert.equal(typeof args.marker, 'string');
    hooks.init = script;
  };
  context.route = async (pattern, callback) => {
    assert.equal(pattern, '**/*');
    order.push('http-route');
    hooks.http = callback;
  };
  context.routeWebSocket = async (pattern, callback) => {
    assert.equal(pattern, '**/*');
    order.push('websocket-route');
    hooks.websocket = callback;
  };
  context.newPage = async () => {
    order.push('new-page');
    context.emit('page', page);
    return page;
  };
  context.close = async () => {
    order.push('context-close');
  };
  const browser = {
    async newContext(options) {
      order.push('context');
      hooks.contextOptions = options;
      assert.deepEqual(options, {
        serviceWorkers: 'block',
        acceptDownloads: false,
        viewport: { width: 1024, height: 768 },
        permissions: [],
        ignoreHTTPSErrors: false,
        bypassCSP: false,
      });
      return context;
    },
    async close() {
      order.push('browser-close');
    },
  };
  const environment = {
    SystemRoot: 'C:\\Windows',
    WINDIR: 'C:\\Windows',
    TEMP: bootstrap().privateRoot,
    TMP: bootstrap().privateRoot,
  };
  const dependencies = {
    yaml: pinnedYaml,
    environment,
    async prepareRuntime() {
      order.push('verify-runtime');
    },
    async createDenyProxy(options) {
      order.push('deny-proxy');
      hooks.denyProxy = options;
      return {
        server: 'http://127.0.0.1:43218',
        async close() {
          order.push('deny-proxy-close');
        },
      };
    },
    chromium: {
      async launch(options) {
        order.push('launch');
        assert.equal(options.chromiumSandbox, true);
        assert.equal(
          options.executablePath,
          bootstrap().installation.browserExecutable,
        );
        assert.equal(options.headless, true);
        assert.deepEqual(options.proxy, { server: 'http://127.0.0.1:43218' });
        assert.deepEqual(options.env, environment);
        assert.equal(Object.hasOwn(options, 'args'), false);
        assert.equal(Object.hasOwn(options, 'channel'), false);
        return browser;
      },
    },
    ...overrides,
  };
  return { dependencies, order, hooks, page, context, frame };
}

function pinDownloadPageSpy(setup) {
  let currentUrl = 'about:blank';
  const calls = { clicks: 0, downloadWaiters: 0 };
  const anchor = {
    tagName: 'A',
    isConnected: true,
    getAttribute: (name) => (name === 'href' ? '/quote.csv' : null),
  };
  const elementHandle = () => ({
    node: anchor,
    ownerFrame: async () => setup.frame,
    evaluate: async (callback, argument) => callback(anchor, argument?.node),
    getAttribute: async (name) => anchor.getAttribute(name),
    dispose: async () => {},
    async click() {
      calls.clicks++;
      throw new Error('Typed Download must not click a browser-managed anchor');
    },
  });
  setup.page.url = () => currentUrl;
  setup.frame.url = () => currentUrl;
  setup.page.goto = async (url) => {
    currentUrl = url;
  };
  setup.page.ariaSnapshot = async () => '- link "Download quote CSV" [ref=e1]';
  setup.page.locator = (selector) => {
    assert.equal(selector, 'aria-ref=e1');
    return { count: async () => 1, elementHandle: async () => elementHandle() };
  };
  setup.page.waitForEvent = async () => {
    calls.downloadWaiters++;
    throw new Error('Typed Download must not await a browser download');
  };
  return calls;
}

function proxyResponse(proxy, target, options = {}) {
  return new Promise((resolve, reject) => {
    const address = new URL(proxy.server);
    const request = httpRequest({
      hostname: address.hostname,
      port: address.port,
      path: target,
      method: options.method ?? 'GET',
      headers: options.headers ?? {},
      agent: false,
    });
    request.setTimeout(2000, () =>
      request.destroy(new Error('bounded proxy test timed out')),
    );
    request.on('error', reject);
    request.on('connect', (response, socket) => {
      socket.destroy();
      resolve({ status: response.statusCode, body: '' });
    });
    request.on('response', (response) => {
      let body = '';
      response.on('data', (chunk) => {
        body += chunk;
      });
      response.on('error', reject);
      response.on('end', () => resolve({ status: response.statusCode, body }));
    });
    request.end();
  });
}

test(
  'm46_owned_deny_proxy_refuses_http_connect_upgrade_and_oversized_headers_without_forwarding',
  { timeout: 5000 },
  async (t) => {
    let sinkRequests = 0;
    const sink = createHttpServer((_request, reply) => {
      sinkRequests++;
      reply.end('forbidden sink');
    });
    const refused = [];
    let proxy;
    t.after(async () => {
      await proxy?.close();
      await new Promise((resolve) => sink.close(resolve));
    });
    try {
      await new Promise((resolve, reject) => {
        sink.once('error', reject);
        sink.listen(0, '127.0.0.1', resolve);
      });
      proxy = await createDenyProxy({
        signal: t.signal,
        onRequest: (code) => refused.push(code),
      });
      const target =
        'http://127.0.0.1:' + sink.address().port + '/private-sentinel';
      assert.deepEqual(await proxyResponse(proxy, target), {
        status: 403,
        body: '',
      });
      assert.deepEqual(
        await proxyResponse(proxy, '127.0.0.1:' + sink.address().port, {
          method: 'CONNECT',
        }),
        { status: 403, body: '' },
      );
      assert.deepEqual(
        await proxyResponse(proxy, target, {
          headers: { connection: 'Upgrade', upgrade: 'websocket' },
        }),
        { status: 403, body: '' },
      );
      assert.deepEqual(
        await proxyResponse(proxy, target, {
          headers: { 'x-private': 'x'.repeat(9000) },
        }),
        { status: 403, body: '' },
      );
      assert.equal(sinkRequests, 0);
      assert.deepEqual(refused, ['denied', 'denied', 'denied', 'denied']);
      await proxy.close();
      await proxy.close();
      await assert.rejects(proxyResponse(proxy, target), {
        code: 'ECONNREFUSED',
      });
    } finally {
      await proxy?.close();
      await new Promise((resolve) => sink.close(resolve));
    }
  },
);

test(
  'm46_owned_deny_proxy_close_joins_idle_sockets_and_rejects_already_cancelled_startup',
  { timeout: 5000 },
  async (t) => {
    await assert.rejects(
      createDenyProxy({ signal: AbortSignal.abort() }),
      (error) => error.code === 'cancelled',
    );
    const controller = new AbortController();
    const starting = createDenyProxy({ signal: controller.signal });
    controller.abort();
    await assert.rejects(starting, (error) => error.code === 'cancelled');
    let proxy, socket;
    t.after(async () => {
      socket?.destroy();
      await proxy?.close();
    });
    try {
      proxy = await createDenyProxy({ signal: t.signal });
      const address = new URL(proxy.server);
      socket = createConnection({
        host: address.hostname,
        port: Number(address.port),
      });
      await once(socket, 'connect');
      const closed = once(socket, 'close');
      await proxy.close();
      await closed;
      assert.equal(socket.destroyed, true);
      await assert.rejects(proxyResponse(proxy, '/'), { code: 'ECONNREFUSED' });
    } finally {
      socket?.destroy();
      await proxy?.close();
    }
  },
);

test(
  'm46_maintained_adapter_cancelled_late_proxy_or_browser_startup_joins_every_created_owner',
  { timeout: 5000 },
  async (t) => {
    for (const phase of ['proxy', 'browser']) {
      const controller = new AbortController();
      const setup = adapterSetupSpy({ signal: controller.signal });
      let entered;
      const starting = new Promise((resolve) => {
        entered = resolve;
      });
      let release;
      const held = new Promise((resolve) => {
        release = resolve;
      });
      if (phase === 'proxy') {
        const create = setup.dependencies.createDenyProxy;
        setup.dependencies.createDenyProxy = async (options) => {
          entered();
          await held;
          return create(options);
        };
      } else {
        const launch = setup.dependencies.chromium.launch;
        setup.dependencies.chromium.launch = async (options) => {
          entered();
          await held;
          return launch(options);
        };
      }
      let settled = false;
      const startup = createMaintainedSession(bootstrap(), setup.dependencies);
      t.after(async () => {
        controller.abort();
        release();
        await startup.catch(() => {});
      });
      startup.then(
        () => {
          settled = true;
        },
        () => {
          settled = true;
        },
      );
      try {
        await starting;
        controller.abort();
        await Promise.resolve();
        assert.equal(
          settled,
          false,
          'late startup is still owned while its resource has not returned',
        );
        release();
        await assert.rejects(startup, (error) => error.code === 'cancelled');
        assert.equal(
          setup.order.filter((item) => item === 'deny-proxy-close').length,
          1,
        );
        assert.equal(setup.order.includes('context'), false);
        if (phase === 'proxy')
          assert.equal(setup.order.includes('launch'), false);
        else
          assert.equal(
            setup.order.indexOf('browser-close') <
              setup.order.indexOf('deny-proxy-close'),
            true,
          );
      } finally {
        controller.abort();
        release();
        await startup.catch(() => {});
      }
    }
  },
);

test(
  'm46_deny_proxy_startup_refusals_remain_charged_until_first_action_but_limits_and_failures_retire',
  { timeout: 5000 },
  async () => {
    let fetches = 0;
    const fatals = [];
    const setup = adapterSetupSpy({
      onFatal: (error) => fatals.push(error.code),
      async fetchImpl() {
        fetches++;
        throw new Error('Proxy refusals must never forward');
      },
    });
    pinDownloadPageSpy(setup);
    const launch = setup.dependencies.chromium.launch;
    setup.dependencies.chromium.launch = async (options) => {
      assert.equal(setup.order.includes('context'), false);
      setup.hooks.denyProxy.onRequest('denied');
      setup.hooks.denyProxy.onRequest('denied');
      return launch(options);
    };
    let session;
    try {
      session = await createMaintainedSession(bootstrap(), setup.dependencies);
      assert.deepEqual(fatals, []);
      assert.equal(session.counters().actionsUsed, 0);
      assert.equal(session.counters().requestsUsed, 2);
      assert.equal(session.diagnostics().blockedRequests, 2);
      assert.equal(fetches, 0);

      const navigate = executeMessage({ kind: 'navigate', routeId: 'index' });
      navigate.payload.contextId = session.contextId;
      await session.execute(navigate);
      setup.hooks.denyProxy.onRequest('denied');
      assert.equal(session.counters().actionsUsed, 1);
      assert.equal(session.counters().requestsUsed, 3);
      assert.equal(session.diagnostics().blockedRequests, 3);
      assert.deepEqual(fatals, ['denied']);
      const observe = executeMessage({ kind: 'observe', screenshot: false }, 3);
      observe.payload.contextId = session.contextId;
      await assert.rejects(
        session.execute(observe),
        (error) => error.code === 'denied',
      );
      assert.equal(fetches, 0);
    } finally {
      await session?.close();
    }

    for (const failure of [
      'request-cap',
      'limitsExceeded',
      'listener-failure',
    ]) {
      const selected = bootstrap();
      selected.scope.limits.requests = 2;
      const startupFatals = [];
      const failing = adapterSetupSpy({
        onFatal: (error) => startupFatals.push(error.code),
      });
      const start = failing.dependencies.chromium.launch;
      failing.dependencies.chromium.launch = async (options) => {
        if (failure === 'request-cap') {
          for (let index = 0; index < 3; index++)
            failing.hooks.denyProxy.onRequest('denied');
        } else if (failure === 'limitsExceeded') {
          failing.hooks.denyProxy.onRequest('limitsExceeded');
        } else {
          failing.hooks.denyProxy.onFailure();
        }
        return start(options);
      };
      const expected =
        failure === 'listener-failure' ? 'unavailable' : 'limitsExceeded';
      await assert.rejects(
        createMaintainedSession(selected, failing.dependencies),
        (error) => error.code === expected,
      );
      assert.deepEqual(startupFatals, [expected]);
      assert.equal(failing.order.includes('context'), false);
      assert.equal(
        failing.order.filter((item) => item === 'deny-proxy-close').length,
        1,
      );
    }
  },
);

test('m46_maintained_adapter_setup_spy_installs_all_guards_before_page_and_closes_context_before_browser', async () => {
  const setup = adapterSetupSpy();
  const session = await createMaintainedSession(
    bootstrap(),
    setup.dependencies,
  );
  assert.equal(
    setup.order.indexOf('verify-runtime') < setup.order.indexOf('launch'),
    true,
  );
  assert.equal(
    setup.order.indexOf('deny-proxy') < setup.order.indexOf('launch'),
    true,
  );
  for (const guard of [
    'binding',
    'init-script',
    'http-route',
    'websocket-route',
  ])
    assert.equal(
      setup.order.indexOf(guard) < setup.order.indexOf('new-page'),
      true,
    );
  assert.deepEqual(session.counters(), {
    actionsUsed: 0,
    requestsUsed: 0,
    wireBytesUsed: 0,
  });
  const result = await session.close();
  assert.equal(result.engineClosed, true);
  assert.equal(Object.hasOwn(result, 'physicalClosed'), false);
  assert.equal(
    setup.order.indexOf('context-close') < setup.order.indexOf('browser-close'),
    true,
  );
  assert.equal(
    setup.order.indexOf('browser-close') <
      setup.order.indexOf('deny-proxy-close'),
    true,
  );
  await session.close();
  assert.equal(
    setup.order.filter((item) => item === 'context-close').length,
    1,
  );
  assert.equal(
    setup.order.filter((item) => item === 'browser-close').length,
    1,
  );
  assert.equal(
    setup.order.filter((item) => item === 'deny-proxy-close').length,
    1,
  );
});

test('m46_maintained_adapter_setup_refuses_cancelled_or_unverified_runtime_before_launch', async () => {
  const controller = new AbortController();
  controller.abort();
  const cancelled = adapterSetupSpy({ signal: controller.signal });
  await assert.rejects(
    createMaintainedSession(bootstrap(), cancelled.dependencies),
    (error) => error.code === 'cancelled',
  );
  assert.equal(cancelled.order.includes('launch'), false);
  const bad = adapterSetupSpy({
    async prepareRuntime() {
      throw new Error('private-sentinel');
    },
  });
  await assert.rejects(
    createMaintainedSession(bootstrap(), bad.dependencies),
    (error) =>
      error.code === 'unavailable' &&
      !error.message.includes('private-sentinel'),
  );
  assert.equal(bad.order.includes('launch'), false);
});

test('m46_maintained_adapter_setup_refuses_missing_guard_attestation_and_joins_created_resources', async () => {
  const setup = adapterSetupSpy();
  setup.page.evaluate = async () => ({
    token: 'a'.repeat(32),
    revision: 0,
    same: true,
    ready: false,
  });
  await assert.rejects(
    createMaintainedSession(bootstrap(), setup.dependencies),
    (error) => error.code === 'unavailable',
  );
  assert.equal(setup.order.includes('new-page'), true);
  assert.equal(
    setup.order.indexOf('context-close') < setup.order.indexOf('browser-close'),
    true,
  );
});

function simulatedRoute(setup, url, navigation = false) {
  let aborted = 0,
    fulfilled = 0;
  return {
    route: {
      request() {
        return {
          frame: () => setup.frame,
          allHeaders: async () => ({}),
          postDataBuffer: () => null,
          method: () => 'GET',
          url: () => url,
          isNavigationRequest: () => navigation,
        };
      },
      async abort() {
        aborted++;
      },
      async fulfill() {
        fulfilled++;
      },
    },
    aborted: () => aborted,
    fulfilled: () => fulfilled,
  };
}

test('m46_maintained_adapter_http_hook_uses_manual_fetch_and_never_fulfills_a_redirect', async () => {
  let fetches = 0,
    fatals = 0;
  const setup = adapterSetupSpy({
    onFatal() {
      fatals++;
    },
    async fetchImpl(url, options) {
      fetches++;
      assert.equal(options.redirect, 'manual');
      assert.equal(options.credentials, 'omit');
      assert.deepEqual(options.headers, { 'accept-encoding': 'identity' });
      return new Response(null, {
        status: 302,
        headers: { location: scope().origin + '/quote.csv' },
      });
    },
  });
  const session = await createMaintainedSession(
    bootstrap(),
    setup.dependencies,
  );
  const attempt = simulatedRoute(setup, scope().origin + '/app.js');
  await setup.hooks.http(attempt.route);
  assert.equal(fetches, 1);
  assert.equal(attempt.fulfilled(), 0);
  assert.equal(attempt.aborted(), 1);
  assert.equal(fatals, 1);
  assert.equal(session.counters().requestsUsed, 1);
  assert.equal(session.diagnostics().redirectsRejected, 1);
  assert.equal(session.diagnostics().blockedRequests, 1);
  await session.close();
});

test('m46_maintained_adapter_overlapping_responses_share_one_fetched_byte_budget_before_fulfillment', async () => {
  const maximumFetchedBytes = 1_048_576;
  const responseBytes = 65_536;
  const requestCount = 17;
  let fetches = 0;
  const fatalCodes = [];
  const publishedBodies = [];
  let releaseFetches;
  const overlap = new Promise((resolve) => {
    releaseFetches = resolve;
  });
  let allFetchesStarted;
  const started = new Promise((resolve) => {
    allFetchesStarted = resolve;
  });
  const setup = adapterSetupSpy({
    onFatal(error) {
      fatalCodes.push(error.code);
    },
    async fetchImpl(url, options) {
      assert.equal(url, scope().origin + '/app.js');
      assert.equal(options.redirect, 'manual');
      assert.equal(options.credentials, 'omit');
      fetches++;
      if (fetches === requestCount) allFetchesStarted();
      // Every admitted request is pending before any response body is returned.
      // Per-request copies of the same aggregate base must not grant 17 budgets.
      await overlap;
      return new Response(Buffer.alloc(responseBytes, 0x61), {
        status: 200,
        headers: { 'content-type': 'application/javascript' },
      });
    },
  });
  const session = await createMaintainedSession(
    bootstrap(),
    setup.dependencies,
  );
  try {
    const attempts = Array.from({ length: requestCount }, () => {
      const attempt = simulatedRoute(setup, scope().origin + '/app.js');
      attempt.route.fulfill = async ({ body }) => {
        publishedBodies.push(body.length);
      };
      return attempt;
    });
    const pending = attempts.map((attempt) => setup.hooks.http(attempt.route));
    await started;
    releaseFetches();
    await Promise.all(pending);
    assert.equal(fetches, requestCount);
    assert.equal(session.counters().requestsUsed, requestCount);
    const publishedBytes = publishedBodies.reduce(
      (sum, bytes) => sum + bytes,
      0,
    );
    assert.ok(
      publishedBytes <= maximumFetchedBytes,
      `parallel allowed responses fulfilled ${publishedBytes} bytes past the shared ${maximumFetchedBytes}-byte session cap`,
    );
    assert.ok(attempts.some((attempt) => attempt.aborted() > 0));
    assert.deepEqual(fatalCodes, ['limitsExceeded']);
  } finally {
    releaseFetches();
    await session.close();
  }
});

test(
  'm46_windows_typed_download_execute_uses_guarded_csv_fetch_shared_budget_and_no_browser_download',
  { skip: process.platform !== 'win32' },
  async () => {
    const csv = Buffer.from(
      'supplier,quantity,unit_price,total,delivery_days\nsupplierA,3,12.50,37.50,2\n',
    );
    for (const scenario of ['success', 'redirect', 'shared-budget']) {
      const cwd = await filesystem.mkdtemp(
        path.join(tmpdir(), 'anima-m46-download-'),
      );
      let session;
      try {
        const supplied = bootstrap();
        supplied.privateRoot = cwd;
        supplied.inputs = [];
        supplied.scope.inputIds = [];
        supplied.outputs = [
          { id: 'csv-1', kind: 'csv', filePath: path.join(cwd, 'quote-1.csv') },
          { id: 'csv-2', kind: 'csv', filePath: path.join(cwd, 'quote-2.csv') },
          {
            id: 'shot-1',
            kind: 'screenshot',
            filePath: path.join(cwd, 'shot-1.jpg'),
          },
          {
            id: 'shot-2',
            kind: 'screenshot',
            filePath: path.join(cwd, 'shot-2.jpg'),
          },
        ];
        const fetched = [];
        const fatals = [];
        const setup = adapterSetupSpy({
          onFatal: (error) => fatals.push(error.code),
          async fetchImpl(url, options) {
            fetched.push(url);
            assert.equal(options.method, 'GET');
            assert.equal(options.redirect, 'manual');
            assert.equal(options.credentials, 'omit');
            assert.equal(Object.hasOwn(options, 'body'), false);
            if (url === supplied.scope.origin + '/app.js')
              return new Response(Buffer.alloc(65_536), {
                headers: { 'content-type': 'application/javascript' },
              });
            assert.equal(url, supplied.scope.origin + '/quote.csv');
            if (scenario === 'redirect')
              return new Response(null, {
                status: 302,
                headers: { location: supplied.scope.origin + '/' },
              });
            return new Response(csv, {
              headers: {
                'content-type': 'text/csv',
                'content-disposition': 'attachment; filename=quote.csv',
              },
            });
          },
        });
        const browserCalls = pinDownloadPageSpy(setup);
        session = await createMaintainedSession(supplied, setup.dependencies);
        const message = (operation, document, id = 2) => {
          const request = executeMessage(operation, id, document);
          request.payload.contextId = session.contextId;
          return request;
        };
        let reply = await session.execute(
          message({ kind: 'navigate', routeId: 'index' }),
        );
        if (scenario === 'shared-budget') {
          for (let index = 0; index < 16; index++) {
            const asset = simulatedRoute(
              setup,
              supplied.scope.origin + '/app.js',
            );
            await setup.hooks.http(asset.route);
            assert.equal(asset.fulfilled(), 1);
          }
          assert.equal(session.counters().requestsUsed, 16);
        }
        let nextId = 3;
        const download = () =>
          message(
            { kind: 'download', node: reply.observation.nodes[0].node },
            reply.document,
            nextId++,
          );
        if (scenario === 'success') {
          for (let index = 0; index < 2; index++) {
            reply = await session.execute(download());
            const artifact = reply.observation.artifacts[0];
            assert.equal(artifact.id, supplied.outputs[index].id);
            assert.equal(artifact.kind, 'csv');
            assert.equal(artifact.bytes, csv.length);
            assert.equal(
              artifact.sha256,
              createHash('sha256').update(csv).digest('hex'),
            );
            assert.deepEqual(
              await filesystem.readFile(supplied.outputs[index].filePath),
              csv,
            );
            assert.equal(reply.counters.requestsUsed, index + 1);
          }
          assert.equal(reply.counters.actionsUsed, 3);
          assert.deepEqual(fetched, [
            supplied.scope.origin + '/quote.csv',
            supplied.scope.origin + '/quote.csv',
          ]);
          assert.deepEqual(fatals, []);
        } else {
          await assert.rejects(
            session.execute(download()),
            (error) =>
              error.code ===
                (scenario === 'redirect' ? 'denied' : 'limitsExceeded') &&
              error.effect === 'mayHaveOccurred',
          );
          assert.equal(
            session.counters().requestsUsed,
            scenario === 'redirect' ? 1 : 17,
          );
          assert.deepEqual(fatals, [
            scenario === 'redirect' ? 'denied' : 'limitsExceeded',
          ]);
          await assert.rejects(filesystem.stat(supplied.outputs[0].filePath), {
            code: 'ENOENT',
          });
          if (scenario === 'redirect')
            assert.equal(session.diagnostics().redirectsRejected, 1);
        }
        assert.deepEqual(browserCalls, { clicks: 0, downloadWaiters: 0 });
        assert.equal(setup.hooks.contextOptions.acceptDownloads, false);
      } finally {
        await session?.close();
        await filesystem.rm(cwd, { recursive: true, force: true });
      }
    }
  },
);

test('m46_maintained_adapter_route_spy_refuses_foreign_requests_and_websocket_without_connecting', async () => {
  let fetches = 0;
  const setup = adapterSetupSpy({
    async fetchImpl() {
      fetches++;
      throw new Error('must not fetch');
    },
  });
  const session = await createMaintainedSession(
    bootstrap(),
    setup.dependencies,
  );
  const attempt = simulatedRoute(setup, 'http://127.0.0.1:43220/sink');
  await setup.hooks.http(attempt.route);
  assert.equal(fetches, 0);
  assert.equal(attempt.aborted(), 1);
  assert.equal(session.diagnostics().blockedRequests, 1);
  await session.close();
  const websocketSetup = adapterSetupSpy();
  const websocketSession = await createMaintainedSession(
    bootstrap(),
    websocketSetup.dependencies,
  );
  let closes = 0,
    connects = 0;
  await websocketSetup.hooks.websocket({
    async close() {
      closes++;
    },
    connectToServer() {
      connects++;
    },
  });
  assert.equal(closes, 1);
  assert.equal(connects, 0);
  assert.equal(websocketSession.counters().requestsUsed, 1);
  assert.equal(websocketSession.diagnostics().blockedRequests, 1);
  await websocketSession.close();
});

function protocolOutput(onMessage = () => {}) {
  const messages = [];
  const output = new Writable({
    write(chunk, encoding, done) {
      const message = JSON.parse(Buffer.from(chunk).subarray(8).toString());
      messages.push(message);
      onMessage(message);
      done();
    },
  });
  return { messages, output };
}

test('m46_protocol_empty_or_partial_eof_never_constructs_an_engine', async () => {
  for (const raw of [Buffer.alloc(0), frame(openMessage()).subarray(0, 9)]) {
    const input = new PassThrough();
    const { output, messages } = protocolOutput();
    let creates = 0;
    const running = runProtocol(bootstrap(), input, output, async () => {
      creates++;
      throw new Error('must not create');
    });
    input.end(raw);
    assert.equal(await running, raw.length ? 1 : 0);
    assert.equal(creates, 0);
    assert.deepEqual(messages, []);
  }
});

test('m46_protocol_eof_during_owned_startup_cancels_and_joins_late_session_without_publication', async () => {
  const supplied = bootstrap();
  const input = new PassThrough();
  const { output, messages } = protocolOutput();
  let observedSignal,
    resolveStartup,
    closes = 0;
  const running = runProtocol(
    supplied,
    input,
    output,
    async (onFatal, signal) => {
      observedSignal = signal;
      return new Promise((resolve) => {
        resolveStartup = () =>
          resolve({
            contextId: 'context-1',
            pageId: 'page-1',
            counters: () => counts(),
            async close() {
              closes++;
              return { engineClosed: true, counters: counts() };
            },
          });
      });
    },
  );
  input.write(frame(openMessage(supplied)));
  input.end();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(observedSignal.aborted, true);
  resolveStartup();
  assert.equal(await running, 1);
  assert.equal(closes >= 1, true);
  assert.deepEqual(messages, []);
});

test('m46_protocol_close_interrupts_active_adapter_call_and_emits_only_ordered_cancellation_then_engine_ack', async () => {
  const supplied = bootstrap();
  const input = new PassThrough();
  let rejectAction,
    cancelled = 0,
    closes = 0,
    actions = 0;
  let openReceived, actionStarted, closeReceived;
  const openedReply = new Promise((resolve) => {
    openReceived = resolve;
  });
  const actionEntered = new Promise((resolve) => {
    actionStarted = resolve;
  });
  const closedReply = new Promise((resolve) => {
    closeReceived = resolve;
  });
  const { output, messages } = protocolOutput((message) => {
    if (message.op === 'open') openReceived();
    if (message.op === 'close') closeReceived();
  });
  const running = runProtocol(supplied, input, output, async () => ({
    contextId: 'context-1',
    pageId: 'page-1',
    counters: () => counts(actions),
    async execute() {
      actions = 1;
      actionStarted();
      return new Promise((resolve, reject) => {
        rejectAction = reject;
      });
    },
    cancel() {
      cancelled++;
      rejectAction?.(new BrowserWorkerError('cancelled', 'mayHaveOccurred'));
    },
    async close() {
      closes++;
      return { engineClosed: true, counters: counts(1) };
    },
  }));
  input.write(frame(openMessage(supplied)));
  await openedReply;
  input.write(frame(executeMessage({ kind: 'observe', screenshot: false })));
  await actionEntered;
  input.write(frame(closeMessage()));
  await closedReply;
  assert.deepEqual(
    messages.map((message) => [message.id, message.op]),
    [
      [1, 'open'],
      [2, 'execute'],
      [3, 'close'],
    ],
  );
  assert.equal(messages[1].payload.ok, false);
  assert.equal(messages[1].payload.error.effect, 'mayHaveOccurred');
  assert.equal(messages[2].payload.result.engineClosed, true);
  assert.equal(cancelled >= 1, true);
  assert.equal(closes >= 1, true);
  input.end();
  assert.equal(await running, 0);
});

test('m46_protocol_output_failure_is_static_and_joins_the_owned_adapter', async () => {
  const supplied = bootstrap();
  const input = new PassThrough();
  let closed = 0,
    cancelled = 0;
  const output = new Writable({
    write(chunk, encoding, done) {
      done(new Error('private-sentinel'));
    },
  });
  const running = runProtocol(supplied, input, output, async () => ({
    contextId: 'context-1',
    pageId: 'page-1',
    counters: () => counts(),
    cancel() {
      cancelled++;
    },
    async close() {
      closed++;
      return { engineClosed: true, counters: counts() };
    },
  }));
  input.write(frame(openMessage(supplied)));
  assert.equal(await running, 1);
  assert.equal(cancelled >= 1, true);
  assert.equal(closed >= 1, true);
});
