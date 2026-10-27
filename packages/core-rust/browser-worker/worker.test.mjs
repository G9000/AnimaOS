import assert from 'node:assert/strict';
import test from 'node:test';
import { createRequire } from 'node:module';
import { EventEmitter, once } from 'node:events';
import { PassThrough, Writable } from 'node:stream';
import { createHash, randomUUID } from 'node:crypto';
import * as filesystem from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import {
  createServer as createHttpServer,
  request as httpRequest,
} from 'node:http';
import { createConnection } from 'node:net';
import {
  createServer as createHttpsServer,
  request as httpsRequest,
} from 'node:https';
import * as worker from './worker.mjs';
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

function publicScope() {
  return {
    ...scope(),
    origin: 'https://m63-browser.anima.tools',
    publicIpv4Pin: '93.184.216.34',
    inputIds: [],
    routes: [
      { id: 'index', method: 'GET', path: '/' },
      { id: 'csv', method: 'GET', path: '/quote.csv' },
    ],
  };
}

function interactiveScope() {
  const value = publicScope();
  value.interactive = {
    origins: [{ origin: value.origin, publicIpv4Pin: value.publicIpv4Pin }],
    resourceBytes: 4194304,
    networkBytes: 33554432,
  };
  return value;
}

test('interactive public scope admits JS resource and form requests only on owner approved origins', () => {
  const value = interactiveScope();
  assert.deepEqual(validateScope(value), value);
  const window = {
    requestsUsed: 0,
    blockedRequests: 0,
    nowMs: 10,
    expiresAtMs: 100,
    pageId: 'page',
    effectActive: true,
    navigationRemaining: 0,
  };
  const request = {
    pageId: 'page',
    isMainFrame: true,
    channel: 'http',
    method: 'GET',
    url: value.origin + '/assets/app.js?v=2',
    navigation: false,
    download: false,
    bodyBytes: 0,
    headerBytes: 0,
  };
  assert.equal(guardRequest(value, request, window).path, '/assets/app.js?v=2');
  assert.equal(
    guardRequest(
      value,
      {
        ...request,
        method: 'POST',
        url: value.origin + '/submit',
        bodyBytes: 10,
      },
      window,
    ).method,
    'POST',
  );
  rejected(
    () =>
      guardRequest(
        value,
        { ...request, url: 'https://foreign.example/app.js' },
        window,
      ),
    'denied',
  );
  rejected(() =>
    validateScope({
      ...value,
      interactive: {
        ...value.interactive,
        origins: [{ origin: value.origin, publicIpv4Pin: '127.0.0.1' }],
      },
    }),
  );
});

test('interactive bounded PDF download preserves the complete body and rejects oversized streams', async () => {
  const body = Buffer.from('%PDF-1.7\ncontrolled result\n%%EOF');
  const result = await readBoundedResponse(
    new Response(body, { headers: { 'content-type': 'application/pdf' } }),
    {
      kind: 'pdf',
      bodyBytes: 4194304,
      totalBytes: 33554432,
      allowAttachment: true,
    },
    { aggregateBytes: 0, deadlineMs: Date.now() + 1000 },
  );
  assert.deepEqual(Buffer.from(result.body), body);
  await assert.rejects(
    readBoundedResponse(
      new Response('<html>not a PDF</html>', {
        headers: { 'content-type': 'application/pdf' },
      }),
      {
        kind: 'pdf',
        bodyBytes: 4194304,
        totalBytes: 33554432,
        allowAttachment: true,
      },
      { aggregateBytes: 0, deadlineMs: Date.now() + 1000 },
    ),
    (error) => error.code === 'denied',
  );
});

test('interactive forms expose typed select and a closed key set', () => {
  for (const operation of [
    { kind: 'selectOption', node: { token: 'node-1' }, value: 'yearly' },
    { kind: 'pressKey', node: { token: 'node-1' }, key: 'Enter' },
  ])
    assert.deepEqual(
      decodeOperation(
        interactiveScope(),
        Buffer.from(JSON.stringify(operation)),
      ),
      operation,
    );
  rejected(() =>
    decodeOperation(
      interactiveScope(),
      Buffer.from(
        JSON.stringify({
          kind: 'pressKey',
          node: { token: 'node-1' },
          key: 'Control+L',
        }),
      ),
    ),
  );
});

test('interactive maintained snapshot preserves frame-qualified refs after a real navigation', () => {
  const parsed = snapshotEvidence(
    '- heading "Protected report" [level=1] [ref=f2e1]',
    pinnedYaml,
  );
  assert.equal(parsed.entries[0].ref, 'f2e1');
});

test('m63_public_scope_and_bootstrap_admit_only_the_operator_pin_and_preserve_loopback', () => {
  assert.deepEqual(validateScope(publicScope()), publicScope());
  assert.equal(
    validateScope({ ...publicScope(), origin: 'https://host.0xyz' }).origin,
    'https://host.0xyz',
  );
  const supplied = { ...bootstrap(), scope: publicScope(), inputs: [] };
  assert.deepEqual(
    parseBootstrap(Buffer.from(JSON.stringify(supplied)), supplied.privateRoot),
    supplied,
  );
  const old = scope();
  assert.deepEqual(validateScope(old), old);
  assert.equal(Object.hasOwn(old, 'publicIpv4Pin'), false);
  assert.notEqual(
    canonicalDigest(publicScope()),
    canonicalDigest({ ...publicScope(), publicIpv4Pin: '93.184.216.35' }),
  );
  for (const extra of [
    { ca: 'fixture' },
    { dialHost: '127.0.0.1' },
    { requestImpl: 'override' },
  ]) {
    rejected(() =>
      parseBootstrap(
        Buffer.from(JSON.stringify({ ...supplied, ...extra })),
        supplied.privateRoot,
      ),
    );
    rejected(() => validateScope({ ...publicScope(), ...extra }));
  }
});

test('m63_public_pin_matches_the_complete_bound_special_use_prefix_edges', () => {
  // Independent literal endpoints, not values derived from the production table.
  const denied = [
    '0.0.0.0',
    '0.255.255.255',
    '10.0.0.0',
    '10.255.255.255',
    '100.64.0.0',
    '100.127.255.255',
    '127.0.0.0',
    '127.255.255.255',
    '169.254.0.0',
    '169.254.255.255',
    '172.16.0.0',
    '172.31.255.255',
    '192.0.0.0',
    '192.0.0.255',
    '192.0.0.9',
    '192.0.0.10',
    '192.0.2.0',
    '192.0.2.255',
    '192.31.196.0',
    '192.31.196.255',
    '192.52.193.0',
    '192.52.193.255',
    '192.88.99.0',
    '192.88.99.255',
    '192.168.0.0',
    '192.168.255.255',
    '192.175.48.0',
    '192.175.48.255',
    '198.18.0.0',
    '198.19.255.255',
    '198.51.100.0',
    '198.51.100.255',
    '203.0.113.0',
    '203.0.113.255',
    '224.0.0.0',
    '239.255.255.255',
    '240.0.0.0',
    '255.255.255.255',
    '168.63.129.16',
  ];
  for (const pin of denied)
    rejected(() => validateScope({ ...publicScope(), publicIpv4Pin: pin }));
  const accepted = [
    '1.0.0.0',
    '9.255.255.255',
    '11.0.0.0',
    '100.63.255.255',
    '100.128.0.0',
    '126.255.255.255',
    '128.0.0.0',
    '169.253.255.255',
    '169.255.0.0',
    '172.15.255.255',
    '172.32.0.0',
    '191.255.255.255',
    '192.0.1.0',
    '192.0.1.255',
    '192.0.3.0',
    '192.31.195.255',
    '192.31.197.0',
    '192.52.192.255',
    '192.52.194.0',
    '192.88.98.255',
    '192.88.100.0',
    '192.167.255.255',
    '192.169.0.0',
    '192.175.47.255',
    '192.175.49.0',
    '198.17.255.255',
    '198.20.0.0',
    '198.51.99.255',
    '198.51.101.0',
    '203.0.112.255',
    '203.0.114.0',
    '223.255.255.255',
    '168.63.129.15',
    '168.63.129.17',
  ];
  for (const pin of accepted)
    assert.equal(
      validateScope({ ...publicScope(), publicIpv4Pin: pin }).publicIpv4Pin,
      pin,
    );
});

test('m63_public_policy_refuses_malformed_hosts_pins_post_and_selected_inputs', () => {
  for (const origin of [
    'https://M63-browser.anima.tools',
    'https://localhost',
    'https://127.0.0.1',
    'https://[::1]',
    'https://2130706433',
    'https://host.123',
    'https://host.0x',
    'https://host.0x7f',
    'https://xn--a.tools',
    'https://xn--bcher-kva.example.com',
    'https://host.anima.tools:443',
    'https://host.anima.tools/',
    'https://host.anima.tools.',
    'https://user@host.anima.tools',
    'https://host.anima.tools?x',
    'https://host.anima.tools#x',
    'https://%68ost.anima.tools',
    'https://é.anima.tools',
    'https://-host.anima.tools',
    'https://host-.anima.tools',
    'https://host..anima.tools',
    'https://host_name.anima.tools',
    'https://host.anima.tools\n',
    'http://host.anima.tools',
    `https://${'a'.repeat(64)}.tools`,
    `https://${('a'.repeat(63) + '.').repeat(4)}tools`,
    ...[
      'localhost',
      'local',
      'internal',
      'lan',
      'home',
      'onion',
      'test',
      'invalid',
      'example',
      'arpa',
    ].map((suffix) => `https://host.${suffix}`),
  ])
    rejected(() => validateScope({ ...publicScope(), origin }));
  for (const publicIpv4Pin of [
    undefined,
    null,
    '',
    '93.184.216',
    '93.184.216.34.1',
    '093.184.216.34',
    '+93.184.216.34',
    '0x5d.184.216.34',
    '93.184.216.256',
    '93.184.216.-1',
    '93.184.216.34\n',
    ' 93.184.216.34',
    '1572395042',
    '::ffff:93.184.216.34',
  ]) {
    rejected(() => validateScope({ ...publicScope(), publicIpv4Pin }));
  }
  const noPin = publicScope();
  delete noPin.publicIpv4Pin;
  rejected(() => validateScope(noPin));
  rejected(() => validateScope({ ...scope(), publicIpv4Pin: '93.184.216.34' }));
  for (const publicIpv4Pin of [null, undefined])
    rejected(() => validateScope({ ...scope(), publicIpv4Pin }));
  rejected(() => validateScope({ ...publicScope(), routes: scope().routes }));
  rejected(() => validateScope({ ...publicScope(), inputIds: ['input-1'] }));
});

test('m63_public_guard_charges_refusals_and_only_admits_exact_navigation_or_typed_download', () => {
  const window = () => ({
    requestsUsed: 0,
    blockedRequests: 0,
    pageId: 'page-1',
    effectActive: true,
    navigationRemaining: 1,
    navigationRouteId: 'index',
    nowMs: 1,
    expiresAtMs: 1000,
  });
  const request = {
    pageId: 'page-1',
    isMainFrame: true,
    channel: 'http',
    method: 'GET',
    url: publicScope().origin + '/',
    navigation: true,
    download: false,
    bodyBytes: 0,
    headerBytes: 0,
  };
  const admitted = window();
  assert.deepEqual(
    guardRequest(publicScope(), request, admitted),
    publicScope().routes[0],
  );
  assert.equal(admitted.requestsUsed, 1);
  assert.equal(admitted.navigationRemaining, 0);
  const csvWindow = {
    ...window(),
    downloadRouteId: 'csv',
    navigationRemaining: 0,
  };
  assert.equal(
    guardRequest(
      publicScope(),
      {
        ...request,
        url: publicScope().origin + '/quote.csv',
        navigation: false,
        download: true,
      },
      csvWindow,
    ).id,
    'csv',
  );
  for (const change of [
    { method: 'POST' },
    { isMainFrame: false },
    { pageId: 'popup' },
    { channel: 'websocket' },
    { navigation: false },
    { url: publicScope().origin + '/other' },
    { url: publicScope().origin.replace('https:', 'http:') + '/' },
    { url: publicScope().origin + ':443/' },
    { url: 'https://other.anima.tools/' },
  ]) {
    const state = window();
    rejected(
      () => guardRequest(publicScope(), { ...request, ...change }, state),
      'denied',
    );
    assert.equal(state.requestsUsed, 1);
    assert.equal(state.blockedRequests, 1);
  }
});

function m63Owner(scopeValue, options) {
  assert.equal(
    typeof worker.createPinnedHttpsRequest,
    'function',
    'public transport owner must exist',
  );
  return worker.createPinnedHttpsRequest(
    scopeValue,
    scopeValue.routes[0],
    options,
  );
}

async function m63Read(owner, controller = new AbortController()) {
  return readBoundedResponse(
    await owner.response,
    {
      kind: 'asset',
      bodyBytes: 65536,
      totalBytes: 1048576,
      allowAttachment: false,
    },
    {
      aggregateBytes: 0,
      deadlineMs: Date.now() + 2000,
      now: Date.now,
      signal: controller.signal,
      abort: () => controller.abort(),
    },
  );
}

function m63HeldRequest() {
  const request = new EventEmitter(),
    socket = new EventEmitter();
  const captured = [];
  let destroys = 0,
    socketCloses = 0;
  socket.destroyed = false;
  socket.destroy = () => {
    socket.destroyed = true;
    return socket;
  };
  request.destroyed = false;
  request.destroy = () => {
    destroys++;
    request.destroyed = true;
    socket.destroy();
    return request;
  };
  request.end = () => {
    queueMicrotask(() => request.emit('socket', socket));
    return request;
  };
  return {
    request,
    socket,
    captured,
    requestImpl(options) {
      captured.push(options);
      return request;
    },
    release() {
      request.emit('close');
      socketCloses++;
      socket.emit('close');
    },
    destroys: () => destroys,
    socketCloses: () => socketCloses,
  };
}

test(
  'interactive preflight forwards only bounded approved origin and closed requested methods',
  { timeout: 2000 },
  async () => {
    const value = interactiveScope();
    const fake = m63HeldRequest();
    const owner = worker.createPinnedHttpsRequest(
      value,
      {
        id: 'index',
        origin: value.origin,
        path: '/api/report',
        method: 'OPTIONS',
      },
      {
        requestImpl: fake.requestImpl,
        requestOrigin: value.origin,
        requestedMethod: 'POST',
        requestedHeaders: 'content-type, x-report-mode',
      },
    );
    owner.response.catch(() => {});
    await new Promise((resolve) => setImmediate(resolve));
    try {
      assert.equal(
        fake.captured[0].headers['Access-Control-Request-Method'],
        'POST',
      );
      assert.equal(
        fake.captured[0].headers['Access-Control-Request-Headers'],
        'content-type, x-report-mode',
      );
      assert.equal(fake.captured[0].headers.Origin, value.origin);
    } finally {
      fake.release();
      await owner.close();
    }
  },
);

test(
  'm63_pinned_request_options_fail_once_without_dns_fetch_or_identity_override',
  { timeout: 3000 },
  async () => {
    const fake = m63HeldRequest();
    const owner = m63Owner(publicScope(), { requestImpl: fake.requestImpl });
    const refusal = assert.rejects(
      owner.response,
      (error) =>
        error instanceof BrowserWorkerError &&
        !error.message.includes('private-sentinel'),
    );
    try {
      await new Promise((resolve) => setImmediate(resolve));
      assert.equal(fake.captured.length, 1);
      assert.deepEqual(fake.captured[0], {
        hostname: '93.184.216.34',
        port: 443,
        path: '/',
        method: 'GET',
        servername: 'm63-browser.anima.tools',
        headers: {
          Host: 'm63-browser.anima.tools',
          'Accept-Encoding': 'identity',
        },
        rejectUnauthorized: true,
        agent: false,
        maxHeaderSize: 8192,
      });
      fake.request.emit('error', new Error('private-sentinel'));
      await refusal;
      assert.equal(fake.captured.length, 1, 'no retry or fallback');
    } finally {
      fake.release();
      await owner.close();
    }
  },
);

test(
  'm63_preaborted_request_never_dispatches_and_late_socket_close_retains_owner',
  { timeout: 3000 },
  async () => {
    const pre = m63HeldRequest();
    const preOwner = m63Owner(publicScope(), {
      signal: AbortSignal.abort(),
      requestImpl: pre.requestImpl,
    });
    await assert.rejects(
      preOwner.response,
      (error) => error.code === 'cancelled',
    );
    await preOwner.close();
    assert.equal(pre.captured.length, 0);
    const fake = m63HeldRequest(),
      controller = new AbortController();
    const owner = m63Owner(publicScope(), {
      signal: controller.signal,
      requestImpl: fake.requestImpl,
    });
    const refusal = assert.rejects(
      owner.response,
      (error) => error.code === 'cancelled',
    );
    try {
      controller.abort(); // Socket delivery is still queued: closure must account for it.
      let retired = false;
      const closing = owner.close().then(() => {
        retired = true;
      });
      await refusal;
      await new Promise((resolve) => setImmediate(resolve));
      assert.equal(fake.socket.destroyed, true);
      assert.equal(retired, false);
      fake.request.emit('close');
      await new Promise((resolve) => setImmediate(resolve));
      assert.equal(
        retired,
        false,
        'request close alone is not socket retirement',
      );
      fake.release();
      await closing;
      await owner.close();
      assert.equal(fake.socketCloses(), 1);
      assert.ok(fake.destroys() >= 1);
    } finally {
      fake.release();
      await owner.close();
    }
  },
);

async function m63TlsPeer(t, certificate, handler) {
  const directory = new URL('./fixtures/m63/', import.meta.url);
  const [key, cert, ca] = await Promise.all(
    ['server-key.pem', certificate, 'ca.pem'].map((name) =>
      filesystem.readFile(new URL(name, directory)),
    ),
  );
  const sockets = new Set();
  let connections = 0,
    retired = 0;
  const server = createHttpsServer({ key, cert }, handler);
  server.on('tlsClientError', () => {});
  server.on('connection', (socket) => {
    connections++;
    sockets.add(socket);
    socket.once('close', () => {
      retired++;
      sockets.delete(socket);
    });
  });
  t.after(async () => {
    for (const socket of sockets) socket.destroy();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const captured = [];
  return {
    captured,
    connections: () => connections,
    retired: () => retired,
    requestImpl(options) {
      captured.push(options);
      // The only dial/CA substitution is this test-owned factory, never bootstrap/CLI.
      return httpsRequest({
        ...options,
        hostname: '127.0.0.1',
        port: server.address().port,
        ca,
      });
    },
  };
}

test(
  'm63_local_tls_verifies_logical_name_and_trust_without_a_public_dial',
  { timeout: 8000 },
  async (t) => {
    assert.equal(typeof worker.createPinnedHttpsRequest, 'function');
    for (const certificate of [
      'server.pem',
      'wrong-name.pem',
      'untrusted.pem',
    ]) {
      let replies = 0;
      const peer = await m63TlsPeer(t, certificate, (request, response) => {
        replies++;
        assert.equal(request.socket.servername, 'm63-browser.anima.tools');
        assert.equal(request.headers.host, 'm63-browser.anima.tools');
        assert.equal(request.headers['accept-encoding'], 'identity');
        response.writeHead(200, { 'content-type': 'text/html' });
        response.end('bounded fixture');
      });
      const owner = m63Owner(publicScope(), {
        requestImpl: peer.requestImpl,
        signal: t.signal,
      });
      try {
        if (certificate === 'server.pem')
          assert.equal(
            (await m63Read(owner)).body.toString(),
            'bounded fixture',
          );
        else
          await assert.rejects(
            owner.response,
            (error) => error.code === 'unavailable',
          );
      } finally {
        await owner.close();
      }
      assert.equal(peer.captured.length, 1);
      assert.equal(peer.captured[0].hostname, '93.184.216.34');
      assert.equal(peer.captured[0].rejectUnauthorized, true);
      assert.equal(replies, certificate === 'server.pem' ? 1 : 0);
      assert.equal(peer.connections(), 1);
    }
  },
);

test(
  'm63_local_tls_rejects_redirect_encoding_and_chunked_overrun_without_retry',
  { timeout: 8000 },
  async (t) => {
    assert.equal(typeof worker.createPinnedHttpsRequest, 'function');
    for (const mode of ['redirect', 'encoding', 'overrun']) {
      let requests = 0;
      const peer = await m63TlsPeer(t, 'server.pem', (_request, response) => {
        requests++;
        if (mode === 'redirect') {
          response.writeHead(302, { location: 'https://other.anima.tools/' });
          response.end();
        } else if (mode === 'encoding') {
          response.writeHead(200, { 'content-encoding': 'gzip' });
          response.end('not decoded');
        } else {
          response.writeHead(200);
          response.write(Buffer.alloc(32768));
          response.write(Buffer.alloc(32768));
          response.end('x');
        }
      });
      const controller = new AbortController();
      const owner = m63Owner(publicScope(), {
        requestImpl: peer.requestImpl,
        signal: controller.signal,
      });
      try {
        await assert.rejects(
          m63Read(owner, controller),
          (error) => error instanceof BrowserWorkerError,
        );
      } finally {
        await owner.close();
      }
      assert.equal(requests, 1);
      assert.equal(peer.captured.length, 1);
    }
  },
);

test(
  'm63_local_tls_abort_during_headers_or_body_joins_owned_transport',
  { timeout: 8000 },
  async (t) => {
    assert.equal(typeof worker.createPinnedHttpsRequest, 'function');
    for (const phase of ['headers', 'body']) {
      let entered;
      const started = new Promise((resolve) => {
        entered = resolve;
      });
      const peer = await m63TlsPeer(t, 'server.pem', (_request, response) => {
        if (phase === 'body') {
          response.writeHead(200);
          response.write('partial');
        }
        entered();
      });
      const controller = new AbortController();
      const owner = m63Owner(publicScope(), {
        requestImpl: peer.requestImpl,
        signal: controller.signal,
      });
      const reading = m63Read(owner, controller);
      const refusal = assert.rejects(
        reading,
        (error) => error instanceof BrowserWorkerError,
      );
      try {
        await started;
        controller.abort();
        await refusal;
      } finally {
        controller.abort();
        await owner.close();
      }
      assert.equal(peer.captured.length, 1);
    }
  },
);

test(
  'm63_session_never_uses_fetch_and_close_waits_for_the_public_socket',
  { timeout: 4000 },
  async () => {
    assert.equal(typeof worker.createPinnedHttpsRequest, 'function');
    for (const phase of ['headers', 'body']) {
      const fake = m63HeldRequest();
      let fetches = 0,
        entered;
      const dispatched = new Promise((resolve) => {
        entered = resolve;
      });
      const setup = adapterSetupSpy(
        {
          httpsRequestImpl(options) {
            entered();
            return fake.requestImpl(options);
          },
          async fetchImpl() {
            fetches++;
            throw new Error('ordinary fetch forbidden');
          },
        },
        true,
      );
      const supplied = { ...bootstrap(), scope: publicScope(), inputs: [] };
      const session = await createMaintainedSession(
        supplied,
        setup.dependencies,
      );
      assert.equal(setup.hooks.contextOptions.javaScriptEnabled, false);
      let attempt;
      setup.page.goto = async (url) => {
        attempt = simulatedRoute(setup, url, true);
        await setup.hooks.http(attempt.route);
      };
      const message = executeMessage({ kind: 'navigate', routeId: 'index' });
      message.payload.contextId = session.contextId;
      const execution = session.execute(message);
      execution.catch(() => {});
      try {
        await Promise.race([
          dispatched,
          execution.then(() => assert.fail('expected a pinned request')),
        ]);
        if (phase === 'body') {
          const response = new PassThrough();
          response.statusCode = 200;
          response.rawHeaders = ['Content-Type', 'text/html'];
          fake.request.emit('response', response);
          response.end('bounded body already read before Stop');
          const end = Date.now() + 1000;
          while (!fake.destroys() && Date.now() < end)
            await new Promise((resolve) => setImmediate(resolve));
          assert.ok(
            fake.destroys() > 0,
            'body completion must enter physical close',
          );
        }
        session.cancel();
        let closed = false;
        const closing = session.close().then((result) => {
          closed = true;
          return result;
        });
        await new Promise((resolve) => setImmediate(resolve));
        assert.equal(fetches, 0);
        assert.equal(closed, false);
        assert.equal(attempt.fulfilled(), 0);
        fake.release();
        assert.equal((await closing).engineClosed, true);
        await assert.rejects(execution);
        assert.equal(fake.captured.length, 1);
        assert.equal(session.counters().requestsUsed, 1);
      } finally {
        fake.release();
        await session.close();
        await execution.catch(() => {});
      }
    }
  },
);

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
      ...(['navigate', 'navigateUrl', 'ownerControl', 'observe'].includes(
        operation.kind,
      )
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
  {
    skip:
      process.platform !== 'win32' ||
      ![22, 24].includes(Number(process.versions.node.split('.')[0])),
  },
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

function adapterSetupSpy(overrides = {}, publicPolicy = false) {
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
  context.pages = () => [];
  context.setDefaultTimeout = (value) => {
    assert.equal(value, 10000);
    order.push('timeout');
  };
  context.exposeBinding = async (name, callback) => {
    order.push('binding');
    hooks.binding = callback;
    hooks.bindings ??= {};
    hooks.bindings[name] = callback;
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
        ...(publicPolicy === true ? { javaScriptEnabled: false } : {}),
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
        assert.equal(options.headless, publicPolicy !== 'interactive');
        assert.deepEqual(options.proxy, { server: 'http://127.0.0.1:43218' });
        assert.deepEqual(options.env, environment);
        assert.ok(options.args.includes('--disable-quic'));
        assert.ok(
          options.args.includes(
            '--force-webrtc-ip-handling-policy=disable_non_proxied_udp',
          ),
        );
        assert.equal(Object.hasOwn(options, 'channel'), false);
        return browser;
      },
    },
    ...overrides,
  };
  dependencies.chromium.launchPersistentContext ??= async (
    _directory,
    options,
  ) => {
    const {
      serviceWorkers,
      acceptDownloads,
      viewport,
      permissions,
      ignoreHTTPSErrors,
      bypassCSP,
      javaScriptEnabled,
      ...launch
    } = options;
    const launched = await dependencies.chromium.launch(launch);
    return launched.newContext({
      serviceWorkers,
      acceptDownloads,
      viewport,
      permissions,
      ignoreHTTPSErrors,
      bypassCSP,
      ...(javaScriptEnabled === undefined ? {} : { javaScriptEnabled }),
    });
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

test(
  'interactive routed credentials honor omitted browser cookies and only forward owned included pairs',
  { timeout: 5000 },
  async (t) => {
    const seen = [];
    const otherOrigin = 'https://wrong-browser.anima.tools';
    const peer = await m63TlsPeer(t, 'server.pem', (request, response) => {
      seen.push({
        cookie: request.headers.cookie,
        origin: request.headers.origin,
      });
      response.writeHead(200, {
        'content-type': 'application/json',
        'access-control-allow-origin': '*',
        'set-cookie': [
          'response_one=one; Path=/; Secure; HttpOnly',
          'response_two=two; Path=/; Secure; HttpOnly',
        ],
      });
      response.end('{"ok":true}');
    });
    const supplied = {
      ...bootstrap(),
      scope: { ...interactiveScope(), authenticated: true },
      inputs: [],
      profile: {
        id: 'cookie-owner',
        revision: 1,
        digest: 'd'.repeat(64),
        origin: interactiveScope().origin,
        keyHex: 'a'.repeat(64),
        sealedPath: path.join(
          tmpdir(),
          'absent-cookie-profile-' + randomUUID(),
        ),
        ownerLogin: true,
        loginRouteId: 'index',
      },
    };
    supplied.scope.interactive.origins.push({
      origin: otherOrigin,
      publicIpv4Pin: '93.184.216.35',
    });
    const setup = adapterSetupSpy(
      { httpsRequestImpl: peer.requestImpl },
      'interactive',
    );
    pinDownloadPageSpy(setup);
    const added = [];
    setup.context.pages = () => [];
    setup.context.cookies = async () => [
      {
        name: 'session',
        value: 'synthetic-owner',
        domain: new URL(supplied.scope.origin).hostname,
        path: '/',
        secure: true,
        httpOnly: true,
        expires: -1,
      },
      {
        name: 'another',
        value: 'not-selected',
        domain: new URL(supplied.scope.origin).hostname,
        path: '/',
        secure: true,
        httpOnly: true,
        expires: -1,
      },
    ];
    setup.context.addCookies = async (cookies) => {
      added.push(...cookies);
    };
    setup.context.setStorageState = async () => {};
    setup.dependencies.chromium.launchPersistentContext = async () =>
      setup.context;
    const session = await createMaintainedSession(supplied, setup.dependencies);
    try {
      for (const cookie of [undefined, 'session=synthetic-owner']) {
        const attempt = simulatedRoute(
          setup,
          supplied.scope.origin + '/api/account',
        );
        const request = attempt.route.request();
        request.allHeaders = async () =>
          cookie === undefined
            ? { origin: otherOrigin }
            : { origin: otherOrigin, cookie };
        attempt.route.request = () => request;
        let headers;
        attempt.route.fulfill = async (response) => {
          headers = response.headers;
        };
        await setup.hooks.http(attempt.route);
        assert.equal(attempt.aborted(), 0);
        assert.equal(
          headers['set-cookie'],
          'response_one=one; Path=/; Secure; HttpOnly\nresponse_two=two; Path=/; Secure; HttpOnly',
        );
      }
      assert.deepEqual(seen, [
        { cookie: undefined, origin: otherOrigin },
        { cookie: 'session=synthetic-owner', origin: otherOrigin },
      ]);
      assert.deepEqual(
        added,
        [],
        'the browser, not a manual jar write, applies routed Set-Cookie credentials rules',
      );
      const invalid = simulatedRoute(
        setup,
        supplied.scope.origin + '/api/account',
      );
      const request = invalid.route.request();
      request.allHeaders = async () => ({ cookie: 'session=forged' });
      invalid.route.request = () => request;
      await setup.hooks.http(invalid.route);
      assert.equal(invalid.aborted(), 1);
      assert.equal(
        peer.captured.length,
        2,
        'an unowned cookie is denied before a native request',
      );
    } finally {
      await session.close();
    }
  },
);

test('interactive owner takeover fences observations and invalidates old nodes without extending scope', async () => {
  const supplied = bootstrap();
  supplied.scope = interactiveScope();
  supplied.inputs = [];
  const setup = adapterSetupSpy({}, 'interactive');
  pinDownloadPageSpy(setup);
  let focused = 0;
  setup.page.bringToFront = async () => {
    focused++;
  };
  const session = await createMaintainedSession(supplied, setup.dependencies);
  const message = (operation, document) => {
    const value = executeMessage(operation, 2, document);
    value.payload.contextId = session.contextId;
    return value;
  };
  try {
    const first = await session.execute(
      message({ kind: 'navigate', routeId: 'index' }),
    );
    const held = await session.execute(
      message({ kind: 'ownerControl', enabled: true }),
    );
    assert.deepEqual(held.observation.nodes, []);
    assert.equal(focused, 1);
    await assert.rejects(
      session.execute(message({ kind: 'observe', screenshot: false })),
      (e) => e.code === 'denied' && e.effect === 'notDispatched',
    );
    await session.execute(message({ kind: 'ownerControl', enabled: false }));
    await assert.rejects(
      session.execute(
        message(
          { kind: 'click', node: first.observation.nodes[0].node },
          first.document,
        ),
      ),
      (e) => e.code === 'stale',
    );
    const fresh = await session.execute(
      message({ kind: 'observe', screenshot: false }),
    );
    assert.equal(fresh.observation.currentUrl, supplied.scope.origin + '/');
    assert.equal(supplied.scope.limits.scopeMs, 120000);
  } finally {
    await session.close();
  }
});

test('interactive speculative CONNECT stays denied and charged only for the exact currently admitted navigation', async () => {
  const approved = new URL(interactiveScope().origin).hostname + ':443';
  for (const metadata of [
    { method: 'CONNECT', authority: approved },
    { method: 'CONNECT', authority: approved.replace(':443', ':444') },
    { method: 'CONNECT', authority: 'foreign.example:443' },
    { method: 'GET', authority: approved },
    { method: 'CONNECT', authority: approved + '@foreign.example:443' },
    { method: 'CONNECT', authority: 'background.example:443' },
    undefined,
  ]) {
    const supplied = bootstrap();
    supplied.scope = interactiveScope();
    supplied.inputs = [];
    const fatals = [];
    const setup = adapterSetupSpy(
      { onFatal: (error) => fatals.push(error.code) },
      'interactive',
    );
    pinDownloadPageSpy(setup);
    const goto = setup.page.goto;
    setup.page.goto = async (url) => {
      setup.hooks.denyProxy.onRequest('denied', metadata);
      await goto(url);
    };
    const session = await createMaintainedSession(supplied, setup.dependencies);
    const navigate = executeMessage({ kind: 'navigate', routeId: 'index' });
    navigate.payload.contextId = session.contextId;
    try {
      if (metadata?.authority === approved && metadata?.method === 'CONNECT') {
        await session.execute(navigate);
        assert.deepEqual(fatals, []);
        assert.equal(session.counters().requestsUsed, 1);
        setup.hooks.denyProxy.onRequest('denied', metadata);
        assert.deepEqual(
          fatals,
          ['denied'],
          'same host outside a navigation still retires',
        );
      } else {
        await assert.rejects(session.execute(navigate));
        assert.deepEqual(fatals, ['denied']);
      }
    } finally {
      await session.close();
    }
  }
});

test('interactive owner navigation binding grants no witness outside live owner main-frame control', async () => {
  const supplied = bootstrap();
  supplied.scope = interactiveScope();
  supplied.inputs = [];
  const fatals = [];
  const setup = adapterSetupSpy(
    { onFatal: (error) => fatals.push(error.code) },
    'interactive',
  );
  pinDownloadPageSpy(setup);
  setup.page.bringToFront = async () => {};
  const session = await createMaintainedSession(supplied, setup.dependencies);
  const message = (operation) => {
    const value = executeMessage(operation);
    value.payload.contextId = session.contextId;
    return value;
  };
  const binding = Object.entries(setup.hooks.bindings).find(([name]) =>
    name.startsWith('__anima_owner_navigation_'),
  )[1];
  const source = { page: setup.page, frame: setup.frame };
  try {
    await session.execute(message({ kind: 'navigate', routeId: 'index' }));
    assert.equal(binding(source, supplied.scope.origin), false);
    await session.execute(message({ kind: 'ownerControl', enabled: true }));
    assert.equal(
      binding({ page: {}, frame: setup.frame }, supplied.scope.origin),
      false,
    );
    assert.equal(
      binding({ page: setup.page, frame: {} }, supplied.scope.origin),
      false,
    );
    assert.equal(binding(source, 'https://foreign.example'), false);
    assert.equal(binding(source, supplied.scope.origin), true);
    setup.hooks.denyProxy.onRequest('denied', {
      method: 'CONNECT',
      authority: new URL(supplied.scope.origin).hostname + ':443',
    });
    assert.deepEqual(fatals, []);
    assert.equal(session.counters().requestsUsed, 1);
    setup.page.emit('load');
    setup.hooks.denyProxy.onRequest('denied', {
      method: 'CONNECT',
      authority: new URL(supplied.scope.origin).hostname + ':443',
    });
    assert.deepEqual(fatals, ['denied']);
  } finally {
    await session.close();
  }
});

test('interactive owner navigation hints have bounded uses and never suppress request limits', async () => {
  for (const scenario of ['uses', 'budget']) {
    const supplied = bootstrap();
    supplied.scope = interactiveScope();
    supplied.inputs = [];
    if (scenario === 'budget') supplied.scope.limits.requests = 1;
    const fatals = [];
    const setup = adapterSetupSpy(
      { onFatal: (error) => fatals.push(error.code) },
      'interactive',
    );
    pinDownloadPageSpy(setup);
    setup.page.bringToFront = async () => {};
    const session = await createMaintainedSession(supplied, setup.dependencies);
    const message = (operation) => {
      const value = executeMessage(operation);
      value.payload.contextId = session.contextId;
      return value;
    };
    try {
      await session.execute(message({ kind: 'navigate', routeId: 'index' }));
      await session.execute(message({ kind: 'ownerControl', enabled: true }));
      const binding = Object.entries(setup.hooks.bindings).find(([name]) =>
        name.startsWith('__anima_owner_navigation_'),
      )[1];
      assert.equal(
        binding(
          { page: setup.page, frame: setup.frame },
          supplied.scope.origin,
        ),
        true,
      );
      const requests = scenario === 'budget' ? 2 : 4;
      for (let count = 0; count < requests; count++)
        setup.hooks.denyProxy.onRequest('denied', {
          method: 'CONNECT',
          authority: new URL(supplied.scope.origin).hostname + ':443',
        });
      assert.deepEqual(fatals, [
        scenario === 'budget' ? 'limitsExceeded' : 'denied',
      ]);
      assert.equal(session.counters().requestsUsed, requests);
    } finally {
      await session.close();
    }
  }
});

test(
  'interactive native Chromium handles JS relative assets login redirects forms owner takeover and PDF download',
  { timeout: 60000 },
  async (t) => {
    const browserExecutable = path.join(
      process.env.LOCALAPPDATA ?? '',
      'ms-playwright',
      'chromium-1248',
      'chrome-win64',
      'chrome.exe',
    );
    await filesystem.access(browserExecutable); // Required acceptance: missing runtime is a failure.
    const { chromium } = createRequire(import.meta.url)('playwright-core');
    const directory = await filesystem.mkdtemp(
      path.join(tmpdir(), 'anima-interactive-acceptance-'),
    );
    const pdf = Buffer.from('%PDF-1.7\ncontrolled-browser-report\n%%EOF');
    let formPosts = 0,
      reportPosts = 0,
      ownerPosts = 0;
    let omittedCookie = 'not-requested';
    const peer = await m63TlsPeer(t, 'server.pem', (request, response) => {
      if (request.url === '/public') {
        response.setHeader('content-type', 'text/html');
        response.end(
          '<h1>Public application</h1><script src="/public.js"></script>',
        );
        return;
      }
      if (request.url === '/public.js') {
        response.setHeader('content-type', 'application/javascript');
        response.end(
          'document.body.insertAdjacentHTML("beforeend","<button>Public JS ready</button>")',
        );
        return;
      }
      if (request.url === '/') {
        response.writeHead(302, { location: '/app/login' });
        response.end();
        return;
      }
      if (request.url === '/app/login') {
        response.setHeader('content-type', 'text/html');
        response.end('<h1>Owner sign in</h1><script src="login.js"></script>');
        return;
      }
      if (request.url === '/app/login.js') {
        response.setHeader('content-type', 'application/javascript');
        response.end(
          'document.body.insertAdjacentHTML("beforeend",`<form method="post" action="/session"><label>Passcode<input name="code"></label><button>Sign in</button></form>`);',
        );
        return;
      }
      if (request.url === '/session' && request.method === 'POST') {
        formPosts++;
        request.resume();
        response.writeHead(303, {
          location: '/app/home',
          'set-cookie':
            'session=synthetic-owner-only; Path=/; Secure; HttpOnly; SameSite=Strict',
        });
        response.end();
        return;
      }
      if (request.url === '/credential-omit') {
        omittedCookie = request.headers.cookie;
        response.writeHead(200, {
          'content-type': 'application/json',
          'access-control-allow-origin': '*',
          'set-cookie':
            'session=must-not-store; Path=/; Secure; HttpOnly; SameSite=Strict',
        });
        response.end('{"public":true}');
        return;
      }
      if (request.headers.cookie !== 'session=synthetic-owner-only') {
        response.writeHead(403);
        response.end();
        return;
      }
      if (request.url === '/owner-confirm' && request.method === 'POST') {
        ownerPosts++;
        request.resume();
        response.writeHead(303, { location: '/app/confirmed' });
        response.end();
        return;
      }
      if (request.url === '/app/home' || request.url === '/app/confirmed') {
        response.setHeader('content-type', 'text/html');
        response.end(
          '<h1>Protected report</h1><form method="post" action="/owner-confirm"><label>Owner code<input name="owner-code"></label><button>Continue as owner</button></form><label>Quantity<input name="quantity" value="1"></label><label>Period<select><option value="month">Monthly</option><option value="year">Yearly</option></select></label><button onclick="fetch(\'/report\',{method:\'POST\',body:document.querySelector(\'input[name=quantity]\').value}).then(()=>{document.body.insertAdjacentHTML(\'beforeend\',\'<a href=/report.pdf>Download PDF</a>\')})">Generate report</button>',
        );
        return;
      }
      if (request.url === '/report' && request.method === 'POST') {
        reportPosts++;
        request.resume();
        response.setHeader('content-type', 'application/json');
        response.end('{"ready":true}');
        return;
      }
      if (request.url === '/report.pdf') {
        response.setHeader('content-type', 'application/pdf');
        response.setHeader(
          'content-disposition',
          'attachment; filename="untrusted-name.pdf"',
        );
        response.end(pdf);
        return;
      }
      response.writeHead(404);
      response.end();
    });
    const base = interactiveScope();
    base.authenticated = true;
    base.limits.outputBytes = 4194304;
    const profile = {
      id: 'interactive-owner',
      revision: 1,
      digest: 'd'.repeat(64),
      origin: base.origin,
      keyHex: 'a'.repeat(64),
      sealedPath: path.join(directory, 'owner.sealed'),
      ownerLogin: true,
      loginRouteId: 'index',
    };
    const contexts = [];
    const make = async (ownerLogin, anonymous = false) => {
      const privateRoot = await filesystem.mkdtemp(
        path.join(directory, 'worker-'),
      );
      const supplied = {
        ...bootstrap(),
        scope: { ...base, authenticated: !anonymous },
        inputs: [],
        outputs: [
          {
            id: 'csv-1',
            kind: 'csv',
            filePath: path.join(privateRoot, 'download-1.csv'),
          },
          {
            id: 'csv-2',
            kind: 'csv',
            filePath: path.join(privateRoot, 'download-2.csv'),
          },
          {
            id: 'shot-1',
            kind: 'screenshot',
            filePath: path.join(privateRoot, 'shot-1.jpg'),
          },
          {
            id: 'shot-2',
            kind: 'screenshot',
            filePath: path.join(privateRoot, 'shot-2.jpg'),
          },
        ],
        privateRoot,
        expiresAtMs: Date.now() + 120000,
        installation: {
          id: 'installed-1',
          browserExecutable,
          browserSha256: createHash('sha256')
            .update(await filesystem.readFile(browserExecutable))
            .digest('hex'),
        },
        ...(!anonymous ? { profile: { ...profile, ownerLogin } } : {}),
      };
      const captured = {
        launch: async (...args) => {
          const browser = await chromium.launch(...args);
          const createContext = browser.newContext.bind(browser);
          browser.newContext = async (...options) => {
            const context = await createContext(...options);
            contexts.push(context);
            return context;
          };
          return browser;
        },
        launchPersistentContext: async (...args) => {
          const context = await chromium.launchPersistentContext(...args);
          contexts.push(context);
          return context;
        },
      };
      const session = await createMaintainedSession(supplied, {
        chromium: captured,
        yaml: pinnedYaml,
        environment: {
          SystemRoot: process.env.SystemRoot ?? 'C:\\Windows',
          WINDIR: process.env.WINDIR ?? 'C:\\Windows',
          TEMP: privateRoot,
          TMP: privateRoot,
        },
        profileHeadlessForTests: true,
        httpsRequestImpl: peer.requestImpl,
      });
      return { session, supplied, context: contexts.at(-1) };
    };
    let login, task, publicTask;
    try {
      publicTask = await make(false, true);
      const publicMessage = executeMessage({
        kind: 'navigateUrl',
        url: base.origin + '/public',
      });
      publicMessage.payload.contextId = publicTask.session.contextId;
      const publicReply = await publicTask.session.execute(publicMessage);
      assert.ok(
        publicReply.observation.nodes.some(
          (node) => node.label === 'Public JS ready',
        ),
      );
      await publicTask.session.close();
      publicTask = undefined;
      login = await make(true);
      const ownerPage = login.context.pages()[0];
      await ownerPage.getByLabel('Passcode').fill('synthetic-passcode');
      await ownerPage.getByRole('button', { name: 'Sign in' }).click();
      await ownerPage
        .getByRole('heading', { name: 'Protected report' })
        .waitFor();
      assert.equal(ownerPage.url(), base.origin + '/app/home');
      assert.equal(formPosts, 1);
      await filesystem.writeFile(
        path.join(login.supplied.privateRoot, 'profile.finish'),
        'finish',
      );
      await login.session.close();
      login = undefined;
      assert.ok((await filesystem.readFile(profile.sealedPath)).length > 0);
      assert.equal(
        worker.openProfileState(
          profile,
          JSON.parse(await filesystem.readFile(profile.sealedPath, 'utf8')),
        ).cookies.length,
        1,
      );
      task = await make(false);
      assert.equal((await task.context.cookies(base.origin)).length, 1);
      const message = (operation, document) => {
        const value = executeMessage(operation, 2, document);
        value.payload.contextId = task.session.contextId;
        return value;
      };
      let reply = await task.session.execute(
        message({ kind: 'navigateUrl', url: base.origin + '/app/home' }),
      );
      assert.ok(
        reply.observation.nodes.some(
          (node) => node.label === 'Protected report',
        ),
      );
      await task.session.execute(
        message({ kind: 'ownerControl', enabled: true }),
      );
      await assert.rejects(
        task.session.execute(message({ kind: 'observe', screenshot: false })),
        (e) => e.code === 'denied',
      );
      await task.context.pages()[0].getByLabel('Quantity').fill('7');
      await task.context
        .pages()[0]
        .getByLabel('Owner code')
        .fill('synthetic-owner-confirmation');
      await task.context
        .pages()[0]
        .getByRole('button', { name: 'Continue as owner' })
        .click();
      await task.context.pages()[0].waitForURL(base.origin + '/app/confirmed');
      await task.context
        .pages()[0]
        .getByRole('heading', { name: 'Protected report' })
        .waitFor();
      assert.equal(
        ownerPosts,
        1,
        'one explicit human form navigation during takeover',
      );
      await task.session.execute(
        message({ kind: 'ownerControl', enabled: false }),
      );
      reply = await task.session.execute(
        message({ kind: 'observe', screenshot: false }),
      );
      const period = reply.observation.nodes.find(
        (node) => node.role === 'combobox',
      );
      assert.ok(period);
      reply = await task.session.execute(
        message(
          { kind: 'selectOption', node: period.node, value: 'year' },
          reply.document,
        ),
      );
      const generate = reply.observation.nodes.find(
        (node) => node.role === 'button' && node.label === 'Generate report',
      );
      assert.ok(generate);
      await task.session.execute(
        message({ kind: 'click', node: generate.node }, reply.document),
      );
      await task.context
        .pages()[0]
        .getByRole('link', { name: 'Download PDF' })
        .waitFor();
      reply = await task.session.execute(
        message({ kind: 'observe', screenshot: false }),
      );
      const download = reply.observation.nodes.find(
        (node) => node.role === 'link',
      );
      assert.ok(download);
      reply = await task.session.execute(
        message({ kind: 'download', node: download.node }, reply.document),
      );
      const artifact = reply.observation.artifacts[0];
      assert.equal(artifact.kind, 'pdf');
      assert.equal(artifact.bytes, pdf.length);
      assert.equal(
        artifact.sha256,
        createHash('sha256').update(pdf).digest('hex'),
      );
      assert.deepEqual(
        await filesystem.readFile(task.supplied.outputs[0].filePath),
        pdf,
      );
      assert.equal(formPosts, 1);
      assert.equal(ownerPosts, 1);
      assert.equal(
        reportPosts,
        1,
        'one owner navigation plus one model submission, without effect retry',
      );
      assert.ok(
        peer.captured.every(
          (request) =>
            request.hostname === base.publicIpv4Pin &&
            request.servername === new URL(base.origin).hostname,
        ),
      );
      assert.deepEqual(
        await task.context
          .pages()[0]
          .evaluate(() =>
            fetch('/credential-omit', { credentials: 'omit' }).then(
              (response) => response.json(),
            ),
          ),
        { public: true },
      );
      assert.equal(
        omittedCookie,
        undefined,
        'a real browser credentials:omit request sends no owned Cookie',
      );
      assert.equal(
        (await task.context.cookies(base.origin)).find(
          (cookie) => cookie.name === 'session',
        ).value,
        'synthetic-owner-only',
        'the browser ignores Set-Cookie on an omitted-credentials response',
      );
      const udp = (await import('node:dgram')).createSocket('udp4');
      let udpPackets = 0;
      udp.on('message', () => udpPackets++);
      await new Promise((resolve) => udp.bind(0, '127.0.0.1', resolve));
      try {
        let refused = false;
        try {
          refused = await task.context.pages()[0].evaluate((port) => {
            try {
              new RTCPeerConnection({
                iceServers: [{ urls: 'stun:127.0.0.1:' + port }],
              });
              return false;
            } catch (error) {
              return error.name === 'NotAllowedError';
            }
          }, udp.address().port);
        } catch (error) {
          refused = /closed|destroyed/.test(error.message);
        }
        assert.equal(refused, true);
        await new Promise((resolve) => setTimeout(resolve, 80));
        assert.equal(
          udpPackets,
          0,
          'the blocked WebRTC constructor sends no UDP to the owned sentinel',
        );
        await assert.rejects(
          task.session.execute(message({ kind: 'observe', screenshot: false })),
          (error) => ['denied', 'cancelled'].includes(error.code),
        );
      } finally {
        await new Promise((resolve) => udp.close(resolve));
      }
    } finally {
      await login?.session.close();
      await task?.session.close();
      await publicTask?.session.close();
      await filesystem.rm(directory, { recursive: true, force: true });
    }
  },
);

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

// Authenticated custody regression: the sealed state must survive a fresh worker
// and remain unusable for another exact origin. No secret enters observations.
test('profile state seals cookies and rejects foreign origins and tampering', async () => {
  const profile = {
    id: 'profile-test',
    revision: 1,
    digest: 'a'.repeat(64),
    origin: 'https://owner.example',
    keyHex: 'b'.repeat(64),
  };
  const state = {
    cookies: [
      {
        name: 'session',
        value: 'unique-private-oracle',
        domain: 'owner.example',
        path: '/',
        expires: -1,
        httpOnly: true,
        secure: true,
        sameSite: 'Lax',
      },
    ],
    origins: [],
  };
  const sealed = worker.sealProfileState(profile, state);
  assert.equal(JSON.stringify(sealed).includes('unique-private-oracle'), false);
  assert.deepEqual(worker.openProfileState(profile, sealed), state);
  assert.throws(() =>
    worker.openProfileState(
      { ...profile, origin: 'https://foreign.example' },
      sealed,
    ),
  );
  assert.throws(() =>
    worker.sealProfileState(profile, {
      ...state,
      cookies: [{ ...state.cookies[0], domain: '.example' }],
    }),
  );
  assert.throws(() =>
    worker.openProfileState(profile, { ...sealed, tag: '0'.repeat(32) }),
  );
});

test('authenticated cookie transport binds Cookie and Set-Cookie to the exact origin', async () => {
  const origin = 'https://owner.example';
  assert.equal(
    worker.profileCookieHeader(origin, origin + '/protected', [
      {
        name: 'session',
        value: 'private',
        domain: 'owner.example',
        path: '/',
        secure: true,
        expires: -1,
      },
    ]),
    'session=private',
  );
  assert.throws(() =>
    worker.profileCookieHeader(origin, 'https://foreign.example/protected', []),
  );
  assert.deepEqual(
    worker.profileResponseCookies(origin, origin + '/login', [
      'session=private; Path=/; HttpOnly; Secure; SameSite=Lax',
    ]),
    [
      {
        name: 'session',
        value: 'private',
        domain: 'owner.example',
        path: '/',
        httpOnly: true,
        secure: true,
        sameSite: 'Lax',
      },
    ],
  );
  assert.throws(() =>
    worker.profileResponseCookies(origin, origin + '/login', [
      'session=private; Domain=.example; Path=/',
    ]),
  );
  assert.throws(() =>
    worker.profileResponseCookies(origin, origin + '/login', [
      'session=private; Domain=foreign.example; Path=/',
    ]),
  );
});

test('authenticated scope is a separate explicit lane while ordinary public remains GET only', () => {
  const ordinary = publicScope();
  ordinary.routes.push({ id: 'sign-in', method: 'POST', path: '/login' });
  assert.throws(() => validateScope(ordinary));
  const authenticated = { ...ordinary, authenticated: true };
  assert.equal(validateScope(authenticated), authenticated);
});

test(
  'authenticated persistent profile logs in on controlled site then a fresh worker sees the protected oracle',
  { timeout: 30000 },
  async (t) => {
    const browserExecutable =
      process.env.ANIMAOS_BROWSER_TEST_EXECUTABLE ??
      path.join(
        process.env.LOCALAPPDATA ?? '',
        'ms-playwright',
        'chromium-1248',
        'chrome-win64',
        'chrome.exe',
      );
    try {
      await filesystem.access(browserExecutable);
    } catch {
      t.skip('Maintained Chromium 1248 is not installed');
      return;
    }
    const profileRequire = createRequire(import.meta.url);
    const { chromium } = profileRequire('playwright-core');
    const { yaml } = profileRequire('playwright-core/lib/utilsBundle');
    const directory = await filesystem.mkdtemp(
      path.join(tmpdir(), 'anima-profile-acceptance-'),
    );
    const server = createHttpServer((request, response) => {
      response.setHeader('content-type', 'text/html');
      if (request.url === '/login') {
        response.setHeader(
          'set-cookie',
          'session=protected-unique-session; Path=/; HttpOnly; SameSite=Strict',
        );
        response.end('<h1>Owner signed in</h1>');
      } else
        response.end(
          request.headers.cookie === 'session=protected-unique-session'
            ? '<button>PROTECTED-ORACLE-83fc7</button>'
            : '<h1>Sign in required</h1>',
        );
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    const origin = 'http://127.0.0.1:' + server.address().port;
    const profile = {
      id: 'owner-test',
      revision: 1,
      digest: 'd'.repeat(64),
      origin,
      keyHex: 'a'.repeat(64),
      sealedPath: path.join(directory, 'state.sealed'),
      ownerLogin: true,
      loginRouteId: 'login',
    };
    const make = async (ownerLogin) => {
      const privateRoot = await filesystem.mkdtemp(
        path.join(directory, 'worker-'),
      );
      const supplied = {
        ...bootstrap(),
        scope: {
          ...scope(),
          origin,
          authenticated: true,
          inputIds: [],
          routes: [
            { id: 'login', method: 'GET', path: '/login' },
            { id: 'protected', method: 'GET', path: '/protected' },
          ],
        },
        inputs: [],
        outputs: [],
        privateRoot,
        expiresAtMs: Date.now() + 120000,
        installation: {
          id: 'installed-1',
          browserExecutable,
          browserSha256: createHash('sha256')
            .update(await filesystem.readFile(browserExecutable))
            .digest('hex'),
        },
        profile: { ...profile, ownerLogin },
      };
      const session = await createMaintainedSession(supplied, {
        chromium,
        yaml,
        environment: {
          SystemRoot: 'C:\\Windows',
          WINDIR: 'C:\\Windows',
          TEMP: privateRoot,
          TMP: privateRoot,
        },
        profileHeadlessForTests: true,
      });
      return { session, supplied, privateRoot };
    };
    let first, second;
    try {
      first = await make(true);
      await first.session.close();
      await assert.rejects(filesystem.access(profile.sealedPath));
      first = await make(true);
      await filesystem.writeFile(
        path.join(first.privateRoot, 'profile.finish'),
        'finish',
      );
      await first.session.close();
      const sealed = await filesystem.readFile(profile.sealedPath, 'utf8');
      assert.equal(sealed.includes('protected-unique-session'), false);
      second = await make(false);
      const message = executeMessage({
        kind: 'navigate',
        routeId: 'protected',
      });
      message.payload.scopeId = second.supplied.scope.id;
      message.payload.contextId = second.session.contextId;
      const result = await second.session.execute(message);
      assert.ok(
        result.observation.nodes.some(
          (node) => node.label === 'PROTECTED-ORACLE-83fc7',
        ),
      );
      assert.equal(
        JSON.stringify(result).includes('protected-unique-session'),
        false,
      );
      await second.session.close();
      await filesystem.rm(profile.sealedPath);
      await assert.rejects(
        make(false),
        (error) => error instanceof BrowserWorkerError,
      );
      await assert.rejects(filesystem.access(profile.sealedPath));
    } finally {
      await first?.session.close();
      await second?.session.close();
      await new Promise((resolve) => server.close(resolve));
      await filesystem.rm(directory, { recursive: true, force: true });
    }
  },
);

test(
  'authenticated pinned HTTPS sends exact-origin cookies and bounded POST without weakening ordinary GET',
  { timeout: 4000 },
  async (t) => {
    let received;
    const peer = await m63TlsPeer(
      t,
      'server.pem',
      async (request, response) => {
        let body = '';
        for await (const chunk of request) body += chunk;
        received = {
          method: request.method,
          cookie: request.headers.cookie,
          origin: request.headers.origin,
          body,
        };
        response.writeHead(200, {
          'content-type': 'text/html',
          'set-cookie': 'session=next-private; Path=/; Secure; HttpOnly',
        });
        response.end('Signed in');
      },
    );
    const supplied = {
      ...publicScope(),
      authenticated: true,
      routes: [{ id: 'login', method: 'POST', path: '/login' }],
    };
    const cookie = worker.profileCookieHeader(
      supplied.origin,
      supplied.origin + '/login',
      [
        {
          name: 'session',
          value: 'prior-private',
          domain: 'm63-browser.anima.tools',
          path: '/',
          secure: true,
          expires: -1,
        },
      ],
    );
    const owner = m63Owner(supplied, {
      requestImpl: peer.requestImpl,
      cookie,
      contentType: 'application/x-www-form-urlencoded',
      body: Buffer.from('code=synthetic-fixture'),
    });
    try {
      const response = await owner.response;
      await m63Read({ response: Promise.resolve(response) });
      assert.deepEqual(received, {
        method: 'POST',
        cookie: 'session=prior-private',
        origin: supplied.origin,
        body: 'code=synthetic-fixture',
      });
      assert.equal(
        worker.profileResponseCookies(
          supplied.origin,
          supplied.origin + '/login',
          response.headers.getSetCookie(),
        )[0].value,
        'next-private',
      );
    } finally {
      await owner.close();
    }
    assert.throws(() =>
      worker.createPinnedHttpsRequest(
        { ...supplied, authenticated: undefined },
        supplied.routes[0],
      ),
    );
  },
);

test('authenticated profile key travels only on the private stdin Open handshake', () => {
  const supplied = bootstrap();
  supplied.scope.authenticated = true;
  supplied.profile = {
    id: 'owner-profile',
    revision: 1,
    digest: 'c'.repeat(64),
    origin: supplied.scope.origin,
    sealedPath: 'C:\\Users\\owner\\private-profile.sealed',
    ownerLogin: true,
    loginRouteId: 'index',
  };
  const decoded = parseBootstrap(
    Buffer.from(JSON.stringify(supplied)),
    supplied.privateRoot,
  );
  assert.equal(Object.hasOwn(decoded.profile, 'keyHex'), false);
  const opening = openMessage(supplied);
  opening.payload.profileKey = 'a'.repeat(64);
  assert.equal(
    validateOpen(decoded, opening, supplied.expiresAtMs - 1),
    opening.payload,
  );
  const missing = structuredClone(opening);
  delete missing.payload.profileKey;
  assert.throws(() => validateOpen(decoded, missing, supplied.expiresAtMs - 1));
  const diskKey = structuredClone(supplied);
  diskKey.profile.keyHex = 'a'.repeat(64);
  assert.throws(() =>
    parseBootstrap(Buffer.from(JSON.stringify(diskKey)), supplied.privateRoot),
  );
  const ordinary = openMessage();
  ordinary.payload.profileKey = 'a'.repeat(64);
  assert.throws(() =>
    validateOpen(bootstrap(), ordinary, bootstrap().expiresAtMs - 1),
  );
});

test(
  'authenticated profiles isolate same-origin login and persist default-path form logout across fresh contexts',
  { timeout: 30000 },
  async (t) => {
    const browserExecutable =
      process.env.ANIMAOS_BROWSER_TEST_EXECUTABLE ??
      path.join(
        process.env.LOCALAPPDATA ?? '',
        'ms-playwright',
        'chromium-1248',
        'chrome-win64',
        'chrome.exe',
      );
    try {
      await filesystem.access(browserExecutable);
    } catch {
      t.skip('Maintained Chromium 1248 is not installed');
      return;
    }
    const profileRequire = createRequire(import.meta.url);
    const { chromium } = profileRequire('playwright-core');
    const { yaml } = profileRequire('playwright-core/lib/utilsBundle');
    const directory = await filesystem.mkdtemp(
      path.join(tmpdir(), 'anima-profile-isolation-'),
    );
    const oracle = 'PROTECTED-ORACLE-profile-isolation-6b381';
    const sessionSecret = 'private-same-origin-session-53da7';
    let loginPosts = 0,
      logoutPosts = 0;
    const server = createHttpServer((request, response) => {
      response.setHeader('content-type', 'text/html');
      if (request.method === 'GET' && request.url === '/app/sign-in')
        response.end(
          '<form action="/app/login" method="post"><button>Sign in fixture</button></form>',
        );
      else if (request.method === 'POST' && request.url === '/app/login') {
        loginPosts++;
        response.setHeader(
          'set-cookie',
          'session=' + sessionSecret + '; HttpOnly; SameSite=Strict',
        );
        response.end('<h1>Owner signed in</h1>');
      } else if (request.method === 'POST' && request.url === '/app/logout') {
        logoutPosts++;
        assert.equal(request.headers.cookie, 'session=' + sessionSecret);
        response.setHeader(
          'set-cookie',
          'session=; Max-Age=0; HttpOnly; SameSite=Strict',
        );
        response.end('<h1>Signed out</h1>');
      } else if (request.method === 'GET' && request.url === '/app')
        response.end(
          request.headers.cookie === 'session=' + sessionSecret
            ? '<button disabled>' +
                oracle +
                '</button><form action="/app/logout" method="post"><button>Sign out fixture</button></form>'
            : '<h1>Sign in required</h1>',
        );
      else {
        response.statusCode = 404;
        response.end('Unavailable');
      }
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    const origin = 'http://127.0.0.1:' + server.address().port;
    const profileA = {
      id: 'profile-a',
      revision: 1,
      digest: 'a'.repeat(64),
      origin,
      keyHex: '1'.repeat(64),
      sealedPath: path.join(directory, 'a.sealed'),
      loginRouteId: 'sign-in',
    };
    const profileB = {
      id: 'profile-b',
      revision: 1,
      digest: 'b'.repeat(64),
      origin,
      keyHex: '2'.repeat(64),
      sealedPath: path.join(directory, 'b.sealed'),
      loginRouteId: 'sign-in',
    };
    const active = [];
    const make = async (profile, ownerLogin) => {
      const privateRoot = await filesystem.mkdtemp(
        path.join(directory, 'context-'),
      );
      const supplied = {
        ...bootstrap(),
        scope: {
          ...scope(),
          origin,
          authenticated: true,
          inputIds: [],
          routes: [
            { id: 'sign-in', method: 'GET', path: '/app/sign-in' },
            { id: 'login', method: 'POST', path: '/app/login' },
            { id: 'protected', method: 'GET', path: '/app' },
            { id: 'logout', method: 'POST', path: '/app/logout' },
          ],
        },
        inputs: [],
        outputs: [],
        privateRoot,
        expiresAtMs: Date.now() + 120000,
        installation: {
          id: 'installed-1',
          browserExecutable,
          browserSha256: createHash('sha256')
            .update(await filesystem.readFile(browserExecutable))
            .digest('hex'),
        },
        profile: { ...profile, ownerLogin },
      };
      const session = await createMaintainedSession(supplied, {
        chromium,
        yaml,
        environment: {
          SystemRoot: 'C:\\Windows',
          WINDIR: 'C:\\Windows',
          TEMP: privateRoot,
          TMP: privateRoot,
        },
        profileHeadlessForTests: true,
      });
      let id = 0;
      const context = {
        session,
        privateRoot,
        async execute(operation, prior) {
          const message = executeMessage(operation, ++id);
          message.payload.contextId = session.contextId;
          message.payload.scopeId = supplied.scope.id;
          if (prior) message.payload.documentWitness = prior.document;
          return session.execute(message);
        },
        async finish() {
          await filesystem.writeFile(
            path.join(privateRoot, 'profile.finish'),
            'finish',
          );
          await session.close();
        },
      };
      active.push(context);
      return context;
    };
    const labels = (result) =>
      result.observation.nodes.map((node) => node.label);
    try {
      const setupA = await make(profileA, true);
      const signIn = await setupA.execute({
        kind: 'observe',
        screenshot: false,
      });
      const signInButton = signIn.observation.nodes.find(
        (node) => node.label === 'Sign in fixture',
      );
      assert.ok(signInButton);
      await setupA.execute({ kind: 'click', node: signInButton.node }, signIn);
      await setupA.finish();
      assert.equal(loginPosts, 1);
      const setupB = await make(profileB, true);
      await setupB.finish();
      assert.equal(loginPosts, 1);
      const b = await make(profileB, false);
      const bResult = await b.execute({
        kind: 'navigate',
        routeId: 'protected',
      });
      assert.ok(labels(bResult).includes('Sign in required'));
      assert.equal(labels(bResult).includes(oracle), false);
      await b.session.close();
      const freshA = await make(profileA, false);
      const protectedA = await freshA.execute({
        kind: 'navigate',
        routeId: 'protected',
      });
      assert.ok(labels(protectedA).includes(oracle));
      assert.equal(JSON.stringify(protectedA).includes(sessionSecret), false);
      const signOut = protectedA.observation.nodes.find(
        (node) => node.label === 'Sign out fixture',
      );
      assert.ok(signOut);
      const signedOut = await freshA.execute(
        { kind: 'click', node: signOut.node },
        protectedA,
      );
      assert.ok(labels(signedOut).includes('Signed out'));
      assert.equal(logoutPosts, 1);
      await freshA.session.close();
      const afterLogout = await make(profileA, false);
      const refused = await afterLogout.execute({
        kind: 'navigate',
        routeId: 'protected',
      });
      assert.ok(labels(refused).includes('Sign in required'));
      assert.equal(labels(refused).includes(oracle), false);
      const sealedA = JSON.parse(
        await filesystem.readFile(profileA.sealedPath, 'utf8'),
      );
      assert.equal(
        worker
          .openProfileState(profileA, sealedA)
          .cookies.some((cookie) => cookie.name === 'session'),
        false,
      );
      assert.equal(loginPosts, 1);
      assert.equal(logoutPosts, 1);
    } finally {
      await Promise.allSettled(
        active.map((context) => context.session.close()),
      );
      await new Promise((resolve) => server.close(resolve));
      await filesystem.rm(directory, { recursive: true, force: true });
    }
  },
);

test('authenticated default cookie path excludes the rightmost slash and matches the exact directory', () => {
  const origin = 'https://owner.example';
  for (const [requestPath, defaultPath] of [
    ['/app/login', '/app'],
    ['/app/', '/app'],
    ['/login', '/'],
    ['/', '/'],
  ]) {
    const [cookie] = worker.profileResponseCookies(
      origin,
      origin + requestPath,
      ['session=directory-session; HttpOnly; Secure'],
    );
    assert.equal(cookie.path, defaultPath, requestPath);
    assert.equal(
      worker.profileCookieHeader(origin, origin + defaultPath, [cookie]),
      'session=directory-session',
      requestPath,
    );
  }
  const [expired] = worker.profileResponseCookies(
    origin,
    origin + '/app/logout',
    ['session=; Max-Age=0; HttpOnly; Secure'],
  );
  assert.equal(expired.path, '/app');
  assert.equal(
    worker.profileCookieHeader(origin, origin + '/app', [expired]),
    '',
  );
});
