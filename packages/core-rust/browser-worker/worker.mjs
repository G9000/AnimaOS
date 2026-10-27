// M46 bounded worker. Imports expose pure helpers; only the explicit CLI loads
// the maintained engine. Physical process ownership remains with the native host.
import { createRequire } from 'node:module';
import {
  createHash,
  createCipheriv,
  createDecipheriv,
  randomBytes,
} from 'node:crypto';
import { randomUUID } from 'node:crypto';
import { win32 as windowsPath } from 'node:path';
import path from 'node:path';
import * as filesystem from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';
import { createServer } from 'node:http';
import { request as httpsRequest } from 'node:https';
import { Readable } from 'node:stream';

const require = createRequire(import.meta.url);
const engineMetadata = require('playwright-core/package.json');
export const PLAYWRIGHT_VERSION = '1.64.1';
const MAINTAINED_CHROMIUM_VERSION = require(
  path.join(
    path.dirname(require.resolve('playwright-core/package.json')),
    'browsers.json',
  ),
).browsers.find((browser) => browser.name === 'chromium').browserVersion;
if (engineMetadata.version !== PLAYWRIGHT_VERSION)
  throw new Error('Browser worker dependency version is unavailable');

export const PROTOCOL_VERSION = 1;
export const FRAME_MAGIC = 'ABW1';
export const FRAME_HEADER_BYTES = 8;
export const LIMITS = Object.freeze({
  frameBytes: 65_536,
  wireBytes: 1_048_576,
  depth: 8,
  jsonNodes: 2_048,
  actions: 40,
  requests: 80,
  scopeMs: 120_000,
  observationNodes: 128,
  observationTextBytes: 8_192,
  typeBytes: 4_096,
  scrollDelta: 2_048,
  inputBytes: 16_384,
  outputBytes: 16_384,
  screenshotBytes: 262_144,
  observationArtifacts: 2,
  sessionArtifacts: 4,
});

/** @typedef {{wireBytes:number,lastIncomingId:number,lastOutgoingId:number}} FrameState */
/** @typedef {{id:string,method:'GET'|'POST',path:string}} BrowserRoute */
/** @typedef {{actions:number,requests:number,scopeMs:number,frameBytes:number,wireBytes:number,observationNodes:number,observationTextBytes:number,inputBytes:number,outputBytes:number,screenshotBytes:number}} BrowserLimits */
/** @typedef {{id:string,revision:number,origin:string,publicIpv4Pin?:string,routes:BrowserRoute[],inputIds:string[],limits:BrowserLimits}} BrowserScope */
/** @typedef {{documentId:string,documentGeneration:number,observationDigest:string}} DocumentWitness */
/** @typedef {{id:string,filePath:string,bytes:number,sha256:string}} InputSlot */
/** @typedef {{id:string,kind:'csv'|'screenshot',filePath:string}} OutputSlot */
/** @typedef {{protocol:1,policyId:string,policyDigest:string,scope:BrowserScope,expiresAtMs:number,installation:{id:string,browserExecutable:string,browserSha256:string},privateRoot:string,inputs:InputSlot[],outputs:OutputSlot[]}} WorkerBootstrap */
/**
 * @typedef {{kind:'navigate',routeId:string}|{kind:'observe',screenshot:boolean}|
 * {kind:'click',node:{token:string}}|{kind:'type',node:{token:string},text:string}|
 * {kind:'scroll',deltaY:number}|{kind:'upload',node:{token:string},inputId:string}|
 * {kind:'download',node:{token:string}}} BrowserOperation
 */
/**
 * @typedef {{protocol:1,id:number,op:'open',payload:{scope:BrowserScope,policyId:string,policyDigest:string,installationId:string,expiresAtMs:number}}|
 * {protocol:1,id:number,op:'execute',payload:{scopeId:string,scopeRevision:number,contextId:string,operation:BrowserOperation,documentWitness?:DocumentWitness}}|
 * {protocol:1,id:number,op:'close',payload:{scopeId:string,scopeRevision:number,contextId:string}}} WorkerRequest
 */

export class MissingFeatureError extends Error {
  constructor() {
    super('Browser worker support is not implemented');
    this.name = 'MissingFeature';
    this.code = 'missingFeature';
    this.effect = 'notDispatched';
  }
}

const ERROR_CODES = new Set([
  'missingFeature',
  'unavailable',
  'invalid',
  'limitsExceeded',
  'denied',
  'stale',
  'cancelled',
  'deadline',
  'protocol',
]);
const EFFECTS = new Set(['notDispatched', 'completed', 'mayHaveOccurred']);
const LIMIT_KEYS = [
  'actions',
  'requests',
  'scopeMs',
  'frameBytes',
  'wireBytes',
  'observationNodes',
  'observationTextBytes',
  'inputBytes',
  'outputBytes',
  'screenshotBytes',
];
const transportStates = new WeakMap();
const textDecoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

export class BrowserWorkerError extends Error {
  constructor(code, effect = 'notDispatched') {
    super(
      effect === 'mayHaveOccurred'
        ? 'Browser action failed; effects may have occurred'
        : 'Browser worker refused the operation',
    );
    this.name = 'BrowserWorkerError';
    this.code = code;
    this.effect = effect;
  }
}

function fail(code = 'invalid', effect = 'notDispatched') {
  throw new BrowserWorkerError(code, effect);
}
function cap(value, maximum) {
  if (value > maximum) fail('limitsExceeded');
}
function integer(value, positive = true) {
  if (!Number.isSafeInteger(value) || value < (positive ? 1 : 0)) fail();
}
function identifier(value) {
  if (
    typeof value !== 'string' ||
    !value.length ||
    value.length > 64 ||
    /[^A-Za-z0-9_-]/.test(value)
  )
    fail();
}
function hash(value) {
  if (
    typeof value !== 'string' ||
    value.length !== 64 ||
    /[^a-f0-9]/.test(value)
  )
    fail();
}
function boolean(value) {
  if (typeof value !== 'boolean') fail();
}
function validString(value) {
  if (typeof value !== 'string') fail();
  for (let i = 0; i < value.length; i++) {
    const unit = value.charCodeAt(i);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(++i);
      if (!(next >= 0xdc00 && next <= 0xdfff)) fail();
    } else if (unit >= 0xdc00 && unit <= 0xdfff) fail();
  }
}
function exact(value, keys) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) fail();
  if (
    Object.getPrototypeOf(value) !== Object.prototype &&
    Object.getPrototypeOf(value) !== null
  )
    fail();
  const actual = Object.keys(value);
  if (
    actual.length !== keys.length ||
    actual.some((key) => !keys.includes(key)) ||
    keys.some((key) => !Object.hasOwn(value, key))
  )
    fail();
}
function array(value, maximum) {
  if (!Array.isArray(value)) fail();
  cap(value.length, maximum);
}
function freeze(value) {
  if (value && typeof value === 'object' && !Object.isFrozen(value)) {
    for (const child of Object.values(value)) freeze(child);
    Object.freeze(value);
  }
  return value;
}
function serializedBytes(value) {
  try {
    return Buffer.from(JSON.stringify(value), 'utf8');
  } catch {
    fail();
  }
}
function bytes(value) {
  if (!(value instanceof Uint8Array)) fail();
  return Buffer.from(value.buffer, value.byteOffset, value.byteLength);
}

// Count values (including containers), with root at depth one, as the core visitor does.
// A bounded recursive descent preserves duplicate/escaped keys before they can collapse.
function strictJson(input, maximum = LIMITS.frameBytes) {
  const raw = bytes(input);
  cap(raw.length, maximum);
  let source;
  try {
    source = textDecoder.decode(raw);
  } catch {
    fail();
  }
  let at = 0;
  let nodes = 0;
  function whitespace() {
    while (at < source.length && /[\x20\t\r\n]/.test(source[at])) at++;
  }
  function string() {
    const start = at++;
    let escaped = false;
    while (at < source.length) {
      const character = source[at++];
      if (escaped) {
        escaped = false;
        continue;
      }
      if (character === '\\') {
        escaped = true;
        continue;
      }
      if (character === '"') {
        let value;
        try {
          value = JSON.parse(source.slice(start, at));
        } catch {
          fail();
        }
        validString(value);
        return value;
      }
    }
    fail();
  }
  function value(depth) {
    cap(depth, LIMITS.depth);
    cap(++nodes, LIMITS.jsonNodes);
    whitespace();
    const character = source[at];
    if (character === '"') return string();
    if (character === '{') {
      at++;
      const result = {};
      const keys = new Set();
      whitespace();
      if (source[at] === '}') {
        at++;
        return result;
      }
      while (at < source.length) {
        whitespace();
        if (source[at] !== '"') fail();
        const key = string();
        if (keys.has(key)) fail();
        keys.add(key);
        whitespace();
        if (source[at++] !== ':') fail();
        Object.defineProperty(result, key, {
          value: value(depth + 1),
          enumerable: true,
          writable: true,
          configurable: true,
        });
        whitespace();
        const separator = source[at++];
        if (separator === '}') return result;
        if (separator !== ',') fail();
      }
      fail();
    }
    if (character === '[') {
      at++;
      const result = [];
      whitespace();
      if (source[at] === ']') {
        at++;
        return result;
      }
      while (at < source.length) {
        result.push(value(depth + 1));
        whitespace();
        const separator = source[at++];
        if (separator === ']') return result;
        if (separator !== ',') fail();
      }
      fail();
    }
    for (const [token, result] of [
      ['true', true],
      ['false', false],
    ]) {
      if (source.startsWith(token, at)) {
        at += token.length;
        return result;
      }
    }
    const number = source
      .slice(at)
      .match(/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/);
    if (!number) fail();
    // Every numeric field in the frozen domain is integral; do not turn a
    // floating JSON token into an integer that Rust's typed decoder refuses.
    if (/[.eE]/.test(number[0]) || number[0] === '-0') fail();
    at += number[0].length;
    const result = Number(number[0]);
    if (!Number.isFinite(result)) fail();
    return result;
  }
  const result = value(1);
  whitespace();
  if (
    at !== source.length ||
    !result ||
    typeof result !== 'object' ||
    Array.isArray(result)
  )
    fail();
  return result;
}

function routePath(value) {
  if (
    typeof value !== 'string' ||
    value.length > 256 ||
    !value.startsWith('/') ||
    /[^A-Za-z0-9/_\-.]/.test(value)
  )
    fail();
  const segments = value.slice(1).split('/');
  if (
    segments.some(
      (segment, index) =>
        segment === '.' ||
        segment === '..' ||
        (segment === '' && index < segments.length - 1),
    )
  )
    fail();
}

// IANA IPv4 Special-Purpose registry, 2025-10-09, conservatively collapsed;
// also multicast/reserved and Microsoft's documented platform virtual address.
// This static admission table makes no future-allocation or OS-isolation claim.
const PUBLIC_PIN_DENY = [
  [0x00000000, 8],
  [0x0a000000, 8],
  [0x64400000, 10],
  [0x7f000000, 8],
  [0xa9fe0000, 16],
  [0xac100000, 12],
  [0xc0000000, 24],
  [0xc0000200, 24],
  [0xc01fc400, 24],
  [0xc034c100, 24],
  [0xc0586300, 24],
  [0xc0a80000, 16],
  [0xc0af3000, 24],
  [0xc6120000, 15],
  [0xc6336400, 24],
  [0xcb007100, 24],
  [0xe0000000, 4],
  [0xf0000000, 4],
  [0xa83f8110, 32],
];
function publicPin(value) {
  if (
    typeof value !== 'string' ||
    !/^(?:0|[1-9][0-9]{0,2})(?:\.(?:0|[1-9][0-9]{0,2})){3}$(?![\s\S])/.test(
      value,
    )
  )
    fail();
  const parts = value.split('.').map(Number);
  if (parts.some((part) => part > 255)) fail();
  const address = parts.reduce((total, part) => total * 256 + part, 0);
  for (const [base, prefix] of PUBLIC_PIN_DENY) {
    const width = 2 ** (32 - prefix);
    if (Math.floor(address / width) === Math.floor(base / width)) fail();
  }
}
function publicHostname(origin) {
  if (typeof origin !== 'string' || !origin.startsWith('https://')) fail();
  const hostname = origin.slice(8);
  if (!hostname.length || hostname.length > 253 || /[^a-z0-9.-]/.test(hostname))
    fail();
  const labels = hostname.split('.');
  if (
    labels.length < 2 ||
    labels.some(
      (label) =>
        label.length > 63 ||
        label.startsWith('xn--') ||
        !/^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$(?![\s\S])/.test(label),
    )
  )
    fail();
  if (
    !/[a-z]/.test(labels.at(-1)) ||
    /^0x[0-9a-f]*$/.test(labels.at(-1)) ||
    [
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
    ].includes(labels.at(-1))
  )
    fail();
  try {
    if (new URL(origin).origin !== origin) fail();
  } catch {
    fail();
  }
  return hostname;
}

export function validateScope(scope) {
  const pinned = scope && Object.hasOwn(scope, 'publicIpv4Pin');
  exact(scope, [
    'id',
    'revision',
    'origin',
    'routes',
    'inputIds',
    'limits',
    ...(pinned ? ['publicIpv4Pin'] : []),
    ...(Object.hasOwn(scope ?? {}, 'authenticated') ? ['authenticated'] : []),
    ...(Object.hasOwn(scope ?? {}, 'interactive') ? ['interactive'] : []),
  ]);
  if (Object.hasOwn(scope, 'authenticated')) boolean(scope.authenticated);
  identifier(scope.id);
  integer(scope.revision);
  if (scope.interactive) {
    exact(scope.interactive, ['origins', 'resourceBytes', 'networkBytes']);
    array(scope.interactive.origins, 8);
    if (!pinned || !scope.interactive.origins.length) fail();
    const origins = new Set();
    for (const target of scope.interactive.origins) {
      exact(target, ['origin', 'publicIpv4Pin']);
      publicHostname(target.origin);
      publicPin(target.publicIpv4Pin);
      if (origins.has(target.origin)) fail();
      origins.add(target.origin);
    }
    if (
      !scope.interactive.origins.some(
        (o) =>
          o.origin === scope.origin && o.publicIpv4Pin === scope.publicIpv4Pin,
      )
    )
      fail();
    integer(scope.interactive.resourceBytes);
    cap(scope.interactive.resourceBytes, 4194304);
    integer(scope.interactive.networkBytes);
    cap(scope.interactive.networkBytes, 33554432);
  } else if (Object.hasOwn(scope, 'interactive')) fail();
  if (pinned) {
    publicHostname(scope.origin);
    publicPin(scope.publicIpv4Pin);
  } else if (
    typeof scope.origin !== 'string' ||
    !/^http:\/\/127\.0\.0\.1:[1-9][0-9]{0,4}$(?![\s\S])/.test(scope.origin) ||
    Number(scope.origin.split(':').at(-1)) > 65535
  )
    fail();
  array(scope.routes, 16);
  if (!scope.routes.length) fail();
  const routeIds = new Set();
  const pairs = new Set();
  for (const route of scope.routes) {
    exact(route, ['id', 'method', 'path']);
    identifier(route.id);
    if (route.method !== 'GET' && route.method !== 'POST') fail();
    if (
      pinned &&
      !scope.authenticated &&
      !scope.interactive &&
      route.method !== 'GET'
    )
      fail();
    if (scope.interactive) interactiveUrl(scope, scope.origin + route.path);
    else routePath(route.path);
    const pair = route.method + ' ' + route.path;
    if (routeIds.has(route.id) || pairs.has(pair)) fail();
    routeIds.add(route.id);
    pairs.add(pair);
  }
  array(scope.inputIds, 16);
  if (pinned && scope.inputIds.length) fail();
  const inputs = new Set();
  for (const input of scope.inputIds) {
    identifier(input);
    if (inputs.has(input)) fail();
    inputs.add(input);
  }
  exact(scope.limits, LIMIT_KEYS);
  for (const key of LIMIT_KEYS) {
    integer(scope.limits[key]);
    cap(
      scope.limits[key],
      scope.interactive && key === 'outputBytes' ? 4194304 : LIMITS[key],
    );
  }
  if (scope.limits.frameBytes > scope.limits.wireBytes) fail();
  cap(serializedBytes(scope).length, scope.limits.frameBytes);
  return scope;
}

export function interactiveUrl(scope, value) {
  if (
    !scope.interactive ||
    typeof value !== 'string' ||
    Buffer.byteLength(value) > 2048 ||
    /[\s\\\x00-\x1f\x7f]/.test(value)
  )
    fail('denied');
  let url;
  try {
    url = new URL(value);
  } catch {
    fail('denied');
  }
  if (
    url.protocol !== 'https:' ||
    url.username ||
    url.password ||
    url.hash ||
    url.port ||
    url.href !== value ||
    !scope.interactive.origins.some((o) => o.origin === url.origin)
  )
    fail('denied');
  return url;
}

function nodeRef(value) {
  exact(value, ['token']);
  identifier(value.token);
}
function operationShape(operation) {
  switch (operation?.kind) {
    case 'navigateUrl':
      exact(operation, ['kind', 'url']);
      validString(operation.url);
      cap(Buffer.byteLength(operation.url), 2048);
      break;
    case 'ownerControl':
      exact(operation, ['kind', 'enabled']);
      boolean(operation.enabled);
      break;
    case 'selectOption':
      exact(operation, ['kind', 'node', 'value']);
      nodeRef(operation.node);
      validString(operation.value);
      cap(Buffer.byteLength(operation.value), LIMITS.typeBytes);
      break;
    case 'pressKey':
      exact(operation, ['kind', 'node', 'key']);
      nodeRef(operation.node);
      if (
        !['Enter', 'Tab', 'Space', 'ArrowUp', 'ArrowDown', 'Escape'].includes(
          operation.key,
        )
      )
        fail();
      break;
    case 'navigate':
      exact(operation, ['kind', 'routeId']);
      identifier(operation.routeId);
      break;
    case 'observe':
      exact(operation, ['kind', 'screenshot']);
      boolean(operation.screenshot);
      break;
    case 'click':
    case 'download':
      exact(operation, ['kind', 'node']);
      nodeRef(operation.node);
      break;
    case 'type':
      exact(operation, ['kind', 'node', 'text']);
      nodeRef(operation.node);
      validString(operation.text);
      cap(Buffer.byteLength(operation.text), LIMITS.typeBytes);
      break;
    case 'scroll':
      exact(operation, ['kind', 'deltaY']);
      if (
        !Number.isSafeInteger(operation.deltaY) ||
        operation.deltaY === 0 ||
        Math.abs(operation.deltaY) > LIMITS.scrollDelta
      )
        fail();
      break;
    case 'upload':
      exact(operation, ['kind', 'node', 'inputId']);
      nodeRef(operation.node);
      identifier(operation.inputId);
      break;
    default:
      fail();
  }
}
function validateOperation(scope, operation) {
  operationShape(operation);
  if (operation.kind === 'navigateUrl') interactiveUrl(scope, operation.url);
  if (operation.kind === 'ownerControl' && !scope.interactive) fail('denied');
  if (
    operation.kind === 'navigate' &&
    !scope.routes.some(
      (route) => route.id === operation.routeId && route.method === 'GET',
    )
  )
    fail('denied');
  if (
    operation.kind === 'upload' &&
    !scope.inputIds.includes(operation.inputId)
  )
    fail('denied');
  cap(serializedBytes(operation).length, scope.limits.frameBytes);
}

/** Same strict finite core union, including literal Type and selected Upload IDs. */
export function decodeOperation(scope, input) {
  validateScope(scope);
  const operation = strictJson(input, scope.limits.frameBytes);
  validateOperation(scope, operation);
  return freeze(operation);
}

function absolutePath(value) {
  validString(value);
  if (
    value.length > 1024 ||
    !/^[A-Za-z]:[\\/]/.test(value) ||
    /[\x00-\x1f<>"|?*]/.test(value) ||
    value.slice(2).includes(':')
  )
    fail();
  const parts = value.slice(3).split(/[\\/]/);
  if (
    !parts.length ||
    parts.some(
      (part) =>
        !part ||
        part === '.' ||
        part === '..' ||
        /[ .]$/.test(part) ||
        /^(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)/i.test(part),
    )
  )
    fail();
  return windowsPath.normalize(value).toLowerCase();
}
function containedPath(value, privateRoot) {
  const candidate = absolutePath(value);
  const relative = windowsPath.relative(privateRoot, candidate);
  if (
    !relative ||
    relative === '..' ||
    relative.startsWith('..\\') ||
    windowsPath.isAbsolute(relative)
  )
    fail('denied');
  return candidate;
}

/** Pure lexical admission; actual files, reparse points and binaries are host checks. */
export function parseBootstrap(input, privateCwd) {
  const bootstrap = strictJson(input);
  exact(bootstrap, [
    'protocol',
    'policyId',
    'policyDigest',
    'scope',
    'expiresAtMs',
    'installation',
    'privateRoot',
    'inputs',
    'outputs',
    ...(Object.hasOwn(bootstrap, 'profile') ? ['profile'] : []),
  ]);
  if (bootstrap.profile) {
    const profile = bootstrap.profile;
    exact(profile, [
      'id',
      'revision',
      'digest',
      'origin',
      'sealedPath',
      'ownerLogin',
      'loginRouteId',
    ]);
    profileAad(profile);
    absolutePath(profile.sealedPath);
    boolean(profile.ownerLogin);
    identifier(profile.loginRouteId);
    if (
      bootstrap.scope?.authenticated !== true ||
      profile.origin !== bootstrap.scope.origin ||
      !bootstrap.scope.routes.some(
        (route) => route.id === profile.loginRouteId && route.method === 'GET',
      )
    )
      fail('denied');
  } else if (bootstrap.scope?.authenticated) fail('denied');
  if (bootstrap.protocol !== PROTOCOL_VERSION) fail('protocol');
  identifier(bootstrap.policyId);
  hash(bootstrap.policyDigest);
  validateScope(bootstrap.scope);
  integer(bootstrap.expiresAtMs);
  exact(bootstrap.installation, ['id', 'browserExecutable', 'browserSha256']);
  identifier(bootstrap.installation.id);
  absolutePath(bootstrap.installation.browserExecutable);
  hash(bootstrap.installation.browserSha256);
  const privateRoot = absolutePath(bootstrap.privateRoot);
  if (absolutePath(privateCwd) !== privateRoot) fail('denied');
  array(bootstrap.inputs, 16);
  array(bootstrap.outputs, LIMITS.sessionArtifacts);
  const ids = new Set();
  const paths = new Set();
  for (const inputSlot of bootstrap.inputs) {
    exact(inputSlot, ['id', 'filePath', 'bytes', 'sha256']);
    identifier(inputSlot.id);
    integer(inputSlot.bytes);
    cap(inputSlot.bytes, bootstrap.scope.limits.inputBytes);
    hash(inputSlot.sha256);
    const path = containedPath(inputSlot.filePath, privateRoot);
    if (ids.has(inputSlot.id) || paths.has(path)) fail();
    ids.add(inputSlot.id);
    paths.add(path);
  }
  if (
    ids.size !== bootstrap.scope.inputIds.length ||
    bootstrap.scope.inputIds.some((id) => !ids.has(id))
  )
    fail('denied');
  for (const outputSlot of bootstrap.outputs) {
    exact(outputSlot, ['id', 'kind', 'filePath']);
    identifier(outputSlot.id);
    if (outputSlot.kind !== 'csv' && outputSlot.kind !== 'screenshot') fail();
    const path = containedPath(outputSlot.filePath, privateRoot);
    if (ids.has(outputSlot.id) || paths.has(path)) fail();
    ids.add(outputSlot.id);
    paths.add(path);
  }
  return freeze(bootstrap);
}

function witness(value) {
  exact(value, ['documentId', 'documentGeneration', 'observationDigest']);
  identifier(value.documentId);
  integer(value.documentGeneration);
  hash(value.observationDigest);
}
function envelope(message) {
  exact(message, ['protocol', 'id', 'op', 'payload']);
  if (
    message.protocol !== PROTOCOL_VERSION ||
    !['open', 'execute', 'close'].includes(message.op)
  )
    fail('protocol');
  integer(message.id);
}
function request(message) {
  envelope(message);
  const payload = message.payload;
  if (message.op === 'open') {
    exact(payload, [
      'scope',
      'policyId',
      'policyDigest',
      'installationId',
      'expiresAtMs',
      ...(payload.scope?.authenticated ? ['profileKey'] : []),
    ]);
    if (payload.scope?.authenticated) hash(payload.profileKey);
    validateScope(payload.scope);
    identifier(payload.policyId);
    hash(payload.policyDigest);
    identifier(payload.installationId);
    integer(payload.expiresAtMs);
  } else if (message.op === 'execute') {
    operationShape(payload?.operation);
    const needsWitness = ![
      'navigate',
      'navigateUrl',
      'ownerControl',
      'observe',
    ].includes(payload.operation.kind);
    exact(payload, [
      'scopeId',
      'scopeRevision',
      'contextId',
      'operation',
      ...(needsWitness ? ['documentWitness'] : []),
    ]);
    identifier(payload.scopeId);
    integer(payload.scopeRevision);
    identifier(payload.contextId);
    if (needsWitness) witness(payload.documentWitness);
  } else {
    exact(payload, ['scopeId', 'scopeRevision', 'contextId']);
    identifier(payload.scopeId);
    integer(payload.scopeRevision);
    identifier(payload.contextId);
  }
}
function counters(value, scope) {
  exact(value, ['actionsUsed', 'requestsUsed', 'wireBytesUsed']);
  for (const key of Object.keys(value)) integer(value[key], false);
  cap(value.actionsUsed, scope.limits.actions);
  cap(value.requestsUsed, scope.limits.requests);
  cap(value.wireBytesUsed, scope.limits.wireBytes);
}
function artifact(value, scope) {
  exact(value, ['id', 'kind', 'bytes', 'sha256']);
  identifier(value.id);
  integer(value.bytes);
  hash(value.sha256);
  if (
    !['csv', 'text', 'json', 'pdf', 'png', 'jpeg', 'screenshot'].includes(
      value.kind,
    ) ||
    (!scope.interactive && !['csv', 'screenshot'].includes(value.kind))
  )
    fail();
  cap(
    value.bytes,
    value.kind === 'screenshot'
      ? scope.limits.screenshotBytes
      : scope.limits.outputBytes,
  );
}

export function validateObservation(scope, observation) {
  validateScope(scope);
  exact(observation, [
    'documentGeneration',
    'routeId',
    'nodes',
    'shortened',
    'omittedNodes',
    'artifacts',
    ...(Object.hasOwn(observation ?? {}, 'currentUrl') ? ['currentUrl'] : []),
  ]);
  if (Object.hasOwn(observation, 'currentUrl'))
    interactiveUrl(scope, observation.currentUrl);
  integer(observation.documentGeneration);
  identifier(observation.routeId);
  if (!scope.routes.some((route) => route.id === observation.routeId))
    fail('denied');
  boolean(observation.shortened);
  integer(observation.omittedNodes, false);
  cap(observation.omittedNodes, 0xffffffff);
  if (observation.shortened !== observation.omittedNodes > 0) fail();
  array(observation.nodes, scope.limits.observationNodes);
  const tokens = new Set();
  let textBytes = 0;
  for (const node of observation.nodes) {
    exact(node, ['node', 'role', 'label']);
    nodeRef(node.node);
    validString(node.role);
    validString(node.label);
    if (
      !node.role ||
      Buffer.byteLength(node.role) > 64 ||
      tokens.has(node.node.token)
    )
      fail();
    tokens.add(node.node.token);
    textBytes += Buffer.byteLength(node.role) + Buffer.byteLength(node.label);
    cap(textBytes, scope.limits.observationTextBytes);
  }
  array(observation.artifacts, LIMITS.observationArtifacts);
  const ids = new Set();
  const kinds = new Set();
  for (const item of observation.artifacts) {
    artifact(item, scope);
    if (ids.has(item.id) || kinds.has(item.kind)) fail();
    ids.add(item.id);
    kinds.add(item.kind);
  }
  cap(serializedBytes(observation).length, scope.limits.frameBytes);
  return observation;
}

/** A whole-node reduction of already collected evidence, never partial labels. */
export function boundObservation(
  scope,
  source,
  payloadBudget = scope.limits.frameBytes,
) {
  validateScope(scope);
  integer(payloadBudget);
  cap(payloadBudget, scope.limits.frameBytes);
  exact(source, [
    'documentGeneration',
    'routeId',
    'nodes',
    'shortened',
    'omittedNodes',
    'artifacts',
    ...(Object.hasOwn(source ?? {}, 'currentUrl') ? ['currentUrl'] : []),
  ]);
  array(source.nodes, LIMITS.jsonNodes);
  array(source.artifacts, LIMITS.observationArtifacts);
  integer(source.omittedNodes, false);
  boolean(source.shortened);
  if (source.shortened !== source.omittedNodes > 0) fail();
  const output = {
    ...source,
    nodes: [],
    artifacts: source.artifacts.map((item) => ({ ...item })),
  };
  let textBytes = 0;
  const tokens = new Set();
  for (const item of source.nodes) {
    exact(item, ['node', 'role', 'label']);
    nodeRef(item.node);
    validString(item.role);
    validString(item.label);
    if (
      !item.role ||
      Buffer.byteLength(item.role) > 64 ||
      tokens.has(item.node.token)
    )
      fail();
    tokens.add(item.node.token);
    const size = Buffer.byteLength(item.role) + Buffer.byteLength(item.label);
    if (
      output.nodes.length >= scope.limits.observationNodes ||
      textBytes + size > scope.limits.observationTextBytes
    )
      output.omittedNodes++;
    else {
      output.nodes.push({
        node: { ...item.node },
        role: item.role,
        label: item.label,
      });
      textBytes += size;
    }
  }
  output.shortened = output.omittedNodes > 0;
  while (
    serializedBytes(output).length > payloadBudget &&
    output.nodes.length
  ) {
    output.nodes.pop();
    output.omittedNodes++;
    output.shortened = true;
  }
  cap(serializedBytes(output).length, payloadBudget);
  validateObservation(scope, output);
  return freeze(output);
}

/** RFC8785 key order for the finite validated domain; a hash never grants authority. */
export function canonicalDigest(value) {
  const checked = strictJson(serializedBytes(value));
  function canonical(item) {
    if (Array.isArray(item)) return '[' + item.map(canonical).join(',') + ']';
    if (item && typeof item === 'object')
      return (
        '{' +
        Object.keys(item)
          .sort()
          .map((key) => JSON.stringify(key) + ':' + canonical(item[key]))
          .join(',') +
        '}'
      );
    return JSON.stringify(item);
  }
  return createHash('sha256').update(canonical(checked), 'utf8').digest('hex');
}

function stateMetadata(state) {
  exact(state, ['wireBytes', 'lastIncomingId', 'lastOutgoingId']);
  for (const value of Object.values(state)) integer(value, false);
  let metadata = transportStates.get(state);
  if (!metadata) {
    if (state.lastIncomingId || state.lastOutgoingId) fail('protocol');
    metadata = {
      phase: 'new',
      pending: new Map(),
      scope: undefined,
      contextId: undefined,
      pageId: undefined,
      lastCounters: { actionsUsed: 0, requestsUsed: 0 },
    };
    transportStates.set(state, metadata);
  }
  return metadata;
}
function account(state, length) {
  const metadata = stateMetadata(state);
  state.wireBytes += length;
  cap(state.wireBytes, metadata.scope?.limits.wireBytes ?? LIMITS.wireBytes);
  return metadata;
}
function decodeRecordedFrame(raw, state) {
  const metadata = stateMetadata(state);
  cap(raw.length, metadata.scope?.limits.frameBytes ?? LIMITS.frameBytes);
  if (
    raw.length < FRAME_HEADER_BYTES ||
    raw.subarray(0, 4).toString('latin1') !== FRAME_MAGIC
  )
    fail('protocol');
  const length = raw.readUInt32BE(4);
  if (!length || length !== raw.length - FRAME_HEADER_BYTES) fail('protocol');
  const message = strictJson(raw.subarray(FRAME_HEADER_BYTES));
  request(message);
  if (message.id <= state.lastIncomingId) fail('protocol');
  if (message.op === 'open') {
    if (metadata.phase !== 'new' || metadata.pending.size) fail('protocol');
    cap(raw.length, message.payload.scope.limits.frameBytes);
    cap(state.wireBytes, message.payload.scope.limits.wireBytes);
    metadata.scope = message.payload.scope;
    metadata.phase = 'opening';
  } else {
    if (
      !metadata.scope ||
      !metadata.contextId ||
      !['ready', 'closing', 'closed'].includes(metadata.phase)
    )
      fail('protocol');
    const payload = message.payload;
    if (
      payload.scopeId !== metadata.scope.id ||
      payload.scopeRevision !== metadata.scope.revision ||
      payload.contextId !== metadata.contextId
    )
      fail('stale');
    if (message.op === 'execute') {
      if (metadata.phase !== 'ready' || metadata.pending.size) fail('protocol');
      validateOperation(metadata.scope, payload.operation);
    } else {
      if (
        [...metadata.pending.values()].some((pending) => pending.op === 'close')
      )
        fail('protocol');
      metadata.phase = 'closing';
    }
  }
  state.lastIncomingId = message.id;
  metadata.pending.set(message.id, message);
  return freeze(message);
}

/** Strict ABW1/u32BE decode; even rejected/partial incoming bytes consume the budget. */
export function decodeFrame(input, state) {
  const raw = bytes(input);
  account(state, raw.length);
  return decodeRecordedFrame(raw, state);
}

/** Incremental reader caps allocation after eight header bytes and counts every chunk once. */
export class FrameReader {
  constructor(state) {
    stateMetadata(state);
    this.state = state;
    this.buffer = Buffer.alloc(FRAME_HEADER_BYTES);
    this.used = 0;
    this.expected = 0;
    this.failed = false;
  }
  push(input) {
    if (this.failed) fail('protocol');
    const raw = bytes(input);
    try {
      account(this.state, raw.length);
      const messages = [];
      let at = 0;
      while (at < raw.length) {
        if (!this.expected) {
          const take = Math.min(
            FRAME_HEADER_BYTES - this.used,
            raw.length - at,
          );
          raw.copy(this.buffer, this.used, at, at + take);
          this.used += take;
          at += take;
          if (this.used < FRAME_HEADER_BYTES) break;
          if (this.buffer.subarray(0, 4).toString('latin1') !== FRAME_MAGIC)
            fail('protocol');
          const length = this.buffer.readUInt32BE(4);
          if (!length) fail('protocol');
          this.expected = FRAME_HEADER_BYTES + length;
          cap(
            this.expected,
            stateMetadata(this.state).scope?.limits.frameBytes ??
              LIMITS.frameBytes,
          );
          const allocated = Buffer.alloc(this.expected);
          this.buffer.copy(allocated);
          this.buffer = allocated;
        }
        const take = Math.min(this.expected - this.used, raw.length - at);
        raw.copy(this.buffer, this.used, at, at + take);
        this.used += take;
        at += take;
        if (this.used === this.expected) {
          messages.push(decodeRecordedFrame(this.buffer, this.state));
          this.buffer = Buffer.alloc(FRAME_HEADER_BYTES);
          this.used = 0;
          this.expected = 0;
        }
      }
      return messages;
    } catch (error) {
      this.failed = true;
      this.buffer = Buffer.alloc(0);
      throw error;
    }
  }
  finish() {
    if (this.failed || this.used) {
      this.failed = true;
      fail('protocol');
    }
  }
}

function reply(message, metadata) {
  envelope(message);
  const pending = metadata.pending.get(message.id);
  if (
    !pending ||
    pending.op !== message.op ||
    message.id !== metadata.pending.keys().next().value
  )
    fail('protocol');
  const payload = message.payload;
  const currentCounters =
    payload?.ok === true ? payload.result?.counters : payload?.counters;
  counters(currentCounters, metadata.scope);
  if (
    currentCounters.actionsUsed < metadata.lastCounters.actionsUsed ||
    currentCounters.requestsUsed < metadata.lastCounters.requestsUsed
  )
    fail('protocol');
  if (payload?.ok === false) {
    exact(payload, ['ok', 'error', 'counters']);
    exact(payload.error, ['code', 'effect']);
    if (
      !ERROR_CODES.has(payload.error.code) ||
      !EFFECTS.has(payload.error.effect)
    )
      fail();
    counters(payload.counters, metadata.scope);
    return;
  }
  exact(payload, ['ok', 'result']);
  if (payload.ok !== true) fail();
  const result = payload.result;
  if (message.op === 'open') {
    exact(result, ['contextId', 'pageId', 'counters']);
    identifier(result.contextId);
    identifier(result.pageId);
  } else if (message.op === 'execute') {
    exact(result, [
      'contextId',
      'pageId',
      'document',
      'observation',
      'counters',
    ]);
    if (
      result.contextId !== metadata.contextId ||
      result.pageId !== metadata.pageId
    )
      fail('stale');
    witness(result.document);
    validateObservation(metadata.scope, result.observation);
    if (
      result.document.documentGeneration !==
        result.observation.documentGeneration ||
      result.document.observationDigest !== canonicalDigest(result.observation)
    )
      fail('stale');
  } else {
    exact(result, ['engineClosed', 'counters']);
    if (result.engineClosed !== true) fail();
  }
  counters(result.counters, metadata.scope);
}

/** Exact echoed replies; a close acknowledgement is never physical process proof. */
export function encodeFrame(message, state) {
  const metadata = stateMetadata(state);
  // Inspect direct fields before serialization can discard unknown undefined keys.
  reply(message, metadata);
  const checked = strictJson(serializedBytes(message));
  const replyCounters = checked.payload.ok
    ? checked.payload.result.counters
    : checked.payload.counters;
  // The reply's own decimal byte counter changes its length; settle that tiny fixed point.
  let payloadBytes;
  for (let attempt = 0; attempt < 4; attempt++) {
    payloadBytes = serializedBytes(checked);
    const total = state.wireBytes + FRAME_HEADER_BYTES + payloadBytes.length;
    if (replyCounters.wireBytesUsed === total) break;
    replyCounters.wireBytesUsed = total;
  }
  payloadBytes = serializedBytes(checked);
  if (
    replyCounters.wireBytesUsed !==
    state.wireBytes + FRAME_HEADER_BYTES + payloadBytes.length
  )
    fail('protocol');
  reply(checked, metadata);
  if (checked.id <= state.lastOutgoingId || checked.id > state.lastIncomingId)
    fail('protocol');
  const total = FRAME_HEADER_BYTES + payloadBytes.length;
  cap(total, metadata.scope.limits.frameBytes);
  const header = Buffer.alloc(FRAME_HEADER_BYTES);
  header.write(FRAME_MAGIC, 0, 'ascii');
  header.writeUInt32BE(payloadBytes.length, 4);
  account(state, total);
  state.lastOutgoingId = checked.id;
  metadata.lastCounters = {
    ...(checked.payload.ok
      ? checked.payload.result.counters
      : checked.payload.counters),
  };
  metadata.pending.delete(checked.id);
  if (checked.op === 'open') {
    if (checked.payload.ok) {
      metadata.contextId = checked.payload.result.contextId;
      metadata.pageId = checked.payload.result.pageId;
      metadata.phase = 'ready';
    } else metadata.phase = 'closed';
  } else if (checked.op === 'close') metadata.phase = 'closed';
  return Buffer.concat([header, payloadBytes]);
}

/** Compare the entire admitted policy and installation before a caller may open an engine. */
export function validateOpen(bootstrap, message, nowMs = Date.now()) {
  request(message);
  if (message.op !== 'open') fail('protocol');
  const payload = message.payload;
  if (
    payload.policyId !== bootstrap.policyId ||
    payload.policyDigest !== bootstrap.policyDigest ||
    payload.installationId !== bootstrap.installation.id ||
    payload.expiresAtMs !== bootstrap.expiresAtMs ||
    canonicalDigest(payload.scope) !== canonicalDigest(bootstrap.scope)
  )
    fail('stale');
  integer(nowMs, false);
  if (nowMs >= bootstrap.expiresAtMs) fail('deadline');
  if (bootstrap.expiresAtMs - nowMs > bootstrap.scope.limits.scopeMs)
    fail('invalid');
  return freeze(payload);
}

/** Current evidence check only; the engine must also resolve and pin the element again. */
export function validateExecute(
  bootstrap,
  message,
  current,
  nowMs = Date.now(),
) {
  request(message);
  if (message.op !== 'execute') fail('protocol');
  integer(nowMs, false);
  if (nowMs >= bootstrap.expiresAtMs) fail('deadline');
  const payload = message.payload;
  if (
    payload.scopeId !== bootstrap.scope.id ||
    payload.scopeRevision !== bootstrap.scope.revision ||
    payload.contextId !== current.contextId
  )
    fail('stale');
  validateOperation(bootstrap.scope, payload.operation);
  if (
    !['navigate', 'navigateUrl', 'ownerControl', 'observe'].includes(
      payload.operation.kind,
    )
  ) {
    if (!current.document) fail('stale');
    witness(current.document);
    if (
      canonicalDigest(payload.documentWitness) !==
      canonicalDigest(current.document)
    )
      fail('stale');
    if (
      payload.operation.node &&
      (!(current.nodeTokens instanceof Set) ||
        !current.nodeTokens.has(payload.operation.node.token))
    )
      fail('stale');
  }
  return payload.operation;
}

/** Each call represents one actual intercepted request, including an actual denied one. */
export function guardRequest(scope, intercepted, window) {
  validateScope(scope);
  integer(window.requestsUsed, false);
  integer(window.blockedRequests, false);
  window.requestsUsed++;
  function denied(code = 'denied') {
    window.blockedRequests++;
    fail(code);
  }
  if (window.requestsUsed > scope.limits.requests) denied('limitsExceeded');
  integer(window.nowMs, false);
  integer(window.expiresAtMs);
  identifier(window.pageId);
  boolean(window.effectActive);
  if (
    !Number.isSafeInteger(window.navigationRemaining) ||
    ![0, 1].includes(window.navigationRemaining)
  )
    denied();
  if (window.signal?.aborted) denied('cancelled');
  if (window.nowMs >= window.expiresAtMs) denied('deadline');
  if (
    intercepted.pageId !== window.pageId ||
    intercepted.isMainFrame !== true ||
    intercepted.channel !== 'http'
  )
    denied();
  if (scope.interactive) {
    let target;
    try {
      target = interactiveUrl(scope, intercepted.url);
    } catch {
      denied();
    }
    if (
      !['GET', 'HEAD', 'OPTIONS', 'POST', 'PUT', 'PATCH', 'DELETE'].includes(
        intercepted.method,
      ) ||
      (!['GET', 'HEAD', 'OPTIONS'].includes(intercepted.method) &&
        !window.effectActive)
    )
      denied();
    if (
      !Number.isSafeInteger(intercepted.bodyBytes) ||
      intercepted.bodyBytes < 0 ||
      intercepted.bodyBytes > 32768 ||
      !Number.isSafeInteger(intercepted.headerBytes) ||
      intercepted.headerBytes < 0 ||
      intercepted.headerBytes > 32768
    )
      denied('limitsExceeded');
    if (intercepted.navigation && !window.effectActive) denied();
    const pin = scope.interactive.origins.find(
      (o) => o.origin === target.origin,
    ).publicIpv4Pin;
    return freeze({
      id: scope.routes[0].id,
      method: intercepted.method,
      path: target.pathname + target.search,
      origin: target.origin,
      publicIpv4Pin: pin,
    });
  }
  const route = scope.routes.find(
    (candidate) =>
      candidate.method === intercepted.method &&
      scope.origin + candidate.path === intercepted.url,
  );
  if (!route) denied();
  if (
    typeof intercepted.navigation !== 'boolean' ||
    typeof intercepted.download !== 'boolean'
  )
    denied();
  const body = intercepted.bodyBytes;
  const headers = intercepted.headerBytes;
  if (
    !Number.isSafeInteger(body) ||
    body < 0 ||
    !Number.isSafeInteger(headers) ||
    headers < 0 ||
    headers > 8192 ||
    body > 32768 ||
    (route.method === 'GET' && body !== 0)
  )
    denied('limitsExceeded');
  if (route.method === 'POST' && window.effectActive !== true) denied();
  if (intercepted.navigation === true) {
    if (
      window.navigationRouteId !== route.id ||
      window.navigationRemaining !== 1 ||
      window.effectActive !== true
    )
      denied();
    window.navigationRemaining = 0;
  }
  if (intercepted.download === true && window.downloadRouteId !== route.id)
    denied();
  // Only trusted navigation admission or the worker's typed download can issue
  // public requests. Listed page fetches/resources do not gain that authority.
  if (
    scope.publicIpv4Pin &&
    !scope.authenticated &&
    !intercepted.navigation &&
    !intercepted.download
  )
    denied();
  return freeze({ id: route.id, method: route.method, path: route.path });
}

/** Manual redirects and stream caps checked before fulfillment; this performs no fetch. */
export async function readBoundedResponse(response, limits, control) {
  const mimeKinds = {
    csv: 'text/csv',
    text: 'text/plain',
    json: 'application/json',
    pdf: 'application/pdf',
    png: 'image/png',
    jpeg: 'image/jpeg',
  };
  if (!(limits.kind in mimeKinds) && limits.kind !== 'asset') fail();
  integer(limits.bodyBytes);
  boolean(limits.allowAttachment);
  integer(control.deadlineMs);
  const bodyCap =
    limits.kind === 'asset'
      ? Math.min(limits.bodyBytes, limits.interactive ? 4194304 : 65536)
      : Math.min(
          limits.bodyBytes,
          limits.kind === 'csv' && !limits.interactive
            ? LIMITS.outputBytes
            : 4194304,
        );
  integer(bodyCap);
  integer(limits.totalBytes);
  cap(
    limits.totalBytes,
    limits.interactive || !['asset', 'csv'].includes(limits.kind)
      ? 33554432
      : LIMITS.wireBytes,
  );
  integer(control.aggregateBytes, false);
  if (control.diagnostics) {
    integer(control.diagnostics.blockedRequests, false);
    integer(control.diagnostics.redirectsRejected, false);
  }
  let reader;
  let aborted = false;
  const abort = () => {
    if (!aborted) {
      aborted = true;
      try {
        control.abort?.();
      } catch {
        /* Static refusal remains authoritative. */
      }
    }
  };
  const cancelled = () => {
    if (control.signal?.aborted) fail('cancelled');
    const now = control.now?.() ?? Date.now();
    integer(now, false);
    if (now >= control.deadlineMs) fail('deadline');
    cap(control.deadlineMs - now, LIMITS.scopeMs);
  };
  try {
    cancelled();
    cap(control.aggregateBytes, limits.totalBytes);
    if (response.redirected === true) fail('denied');
    if (
      !Number.isInteger(response.status) ||
      response.status < 200 ||
      response.status >= 300
    ) {
      if (
        response.status >= 300 &&
        response.status < 400 &&
        control.diagnostics
      )
        control.diagnostics.redirectsRejected++;
      fail('denied');
    }
    let headerBytes = 0;
    for (const [name, value] of response.headers)
      headerBytes += Buffer.byteLength(name) + Buffer.byteLength(value) + 4;
    cap(headerBytes, 8192);
    if (
      response.headers.has('content-disposition') &&
      (limits.allowAttachment !== true || limits.kind === 'asset')
    )
      fail('denied');
    const announced = response.headers.get('content-length');
    const encoding = response.headers.get('content-encoding');
    if (encoding !== null && encoding.toLowerCase() !== 'identity')
      fail('denied');
    if (
      announced !== null &&
      (!/^(?:0|[1-9][0-9]*)$(?![\s\S])/.test(announced) ||
        !Number.isSafeInteger(Number(announced)))
    )
      fail('denied');
    if (announced !== null) cap(Number(announced), bodyCap);
    const safeHeaders = {};
    const contentType = response.headers.get('content-type');
    if (
      limits.kind !== 'asset' &&
      (contentType === null ||
        contentType.split(';')[0].trim().toLowerCase() !==
          mimeKinds[limits.kind])
    )
      fail('denied');
    for (const name of [
      'content-type',
      'content-security-policy',
      'content-security-policy-report-only',
      'x-content-type-options',
      'referrer-policy',
      ...(limits.interactive
        ? [
            'access-control-allow-origin',
            'access-control-allow-credentials',
            'access-control-allow-methods',
            'access-control-allow-headers',
            'vary',
          ]
        : []),
    ]) {
      const value = response.headers.get(name);
      if (value !== null) safeHeaders[name] = value;
    }
    const chunks = [];
    let count = 0;
    if (response.body) {
      reader = response.body.getReader();
      while (true) {
        cancelled();
        const read = reader.read();
        let timer;
        let listener;
        const interrupt = new Promise((_, reject) => {
          const remaining =
            control.deadlineMs - (control.now?.() ?? Date.now());
          timer = setTimeout(
            () => reject(new BrowserWorkerError('deadline')),
            Math.max(0, remaining),
          );
          if (control.signal) {
            listener = () => reject(new BrowserWorkerError('cancelled'));
            control.signal.addEventListener('abort', listener, { once: true });
          }
        });
        let next;
        try {
          next = await Promise.race([read, interrupt]);
        } finally {
          clearTimeout(timer);
          if (listener) control.signal.removeEventListener('abort', listener);
        }
        if (next.done) {
          cancelled();
          break;
        }
        const chunk = bytes(next.value);
        count += chunk.length;
        // Charge a consumed chunk synchronously, including a chunk subsequently
        // refused by a size/cancellation check, before retaining or publishing it.
        control.aggregateBytes += chunk.length;
        cap(count, bodyCap);
        cap(control.aggregateBytes, limits.totalBytes);
        cancelled();
        chunks.push(Buffer.from(chunk));
      }
    }
    if (announced !== null && Number(announced) !== count) fail('protocol');
    const body = Buffer.concat(chunks, count);
    if (
      limits.kind === 'pdf' &&
      !body.subarray(0, 5).equals(Buffer.from('%PDF-'))
    )
      fail('denied');
    if (
      limits.kind === 'png' &&
      !body
        .subarray(0, 8)
        .equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))
    )
      fail('denied');
    if (
      limits.kind === 'jpeg' &&
      !body.subarray(0, 3).equals(Buffer.from([255, 216, 255]))
    )
      fail('denied');
    if (['text', 'json'].includes(limits.kind)) {
      let content;
      try {
        content = textDecoder.decode(body);
      } catch {
        fail('denied');
      }
      if (limits.kind === 'json') {
        try {
          JSON.parse(content);
        } catch {
          fail('denied');
        }
      }
    }
    return {
      status: response.status,
      headers: Object.freeze(safeHeaders),
      body,
    };
  } catch (error) {
    if (control.diagnostics) control.diagnostics.blockedRequests++;
    abort();
    // Fetch abort is synchronous; an untrusted stream's cancellation acknowledgement
    // must not hold Stop/deadline. The owner still joins actual engine/process cleanup.
    try {
      const cancellation = reader ? reader.cancel() : response.body?.cancel();
      cancellation?.catch(() => {});
    } catch {
      /* No response data is disclosed. */
    }
    if (error instanceof BrowserWorkerError) throw error;
    fail('unavailable');
  } finally {
    if (reader) reader.releaseLock();
  }
}

// Runtime adapter. The engine and pinned maintained YAML bundle are loaded only
// by the explicit CLI. The bundle export is version-coupled, not a stable API.
// Playwright-core1.64.1 is Microsoft/Apache-2.0; its bundled notices remain with
// that distribution. This adapter imports it rather than copying its engine.
export function parseCliArguments(argv, privateCwd) {
  if (!Array.isArray(argv) || argv.length !== 2 || argv[0] !== '--bootstrap')
    fail('invalid');
  const root = absolutePath(privateCwd);
  containedPath(argv[1], root);
  return argv[1];
}

export function validateWorkerEnvironment(environment, privateCwd) {
  const root = absolutePath(privateCwd);
  const allowed = new Map();
  for (const [name, value] of Object.entries(environment)) {
    const normalized = name.toUpperCase();
    if (
      !['SYSTEMROOT', 'WINDIR', 'TEMP', 'TMP'].includes(normalized) ||
      allowed.has(normalized)
    )
      fail('unavailable');
    allowed.set(normalized, value);
  }
  for (const name of ['SYSTEMROOT', 'WINDIR', 'TEMP', 'TMP'])
    if (!allowed.has(name)) fail('unavailable');
  absolutePath(allowed.get('SYSTEMROOT'));
  absolutePath(allowed.get('WINDIR'));
  for (const name of ['TEMP', 'TMP']) {
    const candidate = absolutePath(allowed.get(name));
    if (candidate !== root) containedPath(candidate, root);
  }
  return freeze({
    SystemRoot: allowed.get('SYSTEMROOT'),
    WINDIR: allowed.get('WINDIR'),
    TEMP: allowed.get('TEMP'),
    TMP: allowed.get('TMP'),
  });
}

async function checkedFile(filePath, maximum, expectedHash, expectedBytes) {
  integer(maximum);
  cap(maximum, 512 * 1024 * 1024);
  if (expectedBytes !== undefined) {
    integer(expectedBytes, false);
    cap(expectedBytes, maximum);
  }
  const regular = (metadata) => metadata.isFile() && !metadata.isSymbolicLink();
  const sameMetadata = (left, right) =>
    left.ino === right.ino &&
    left.size === right.size &&
    left.mtimeNs === right.mtimeNs &&
    left.ctimeNs === right.ctimeNs &&
    left.birthtimeNs === right.birthtimeNs;
  const pathMatchesHandle = (metadata, opened) =>
    regular(metadata) &&
    sameMetadata(metadata, opened) &&
    (metadata.dev === opened.dev ||
      // Node 22.12 on Windows can omit the path volume while the opened handle
      // supplies it. Zero is unavailable evidence, never verified volume equality.
      (process.platform === 'win32' && metadata.dev === 0n));
  const before = await filesystem.lstat(filePath, { bigint: true });
  if (
    !regular(before) ||
    before.size > BigInt(maximum) ||
    (expectedBytes !== undefined && before.size !== BigInt(expectedBytes))
  )
    fail('unavailable');
  const file = await filesystem.open(filePath, 'r');
  try {
    const opened = await file.stat({ bigint: true });
    if (!regular(opened) || !pathMatchesHandle(before, opened))
      fail('unavailable');
    const digest = createHash('sha256');
    const parts = [];
    let length = 0;
    const buffer = Buffer.alloc(65536);
    while (true) {
      const next = await file.read(buffer, 0, buffer.length, null);
      if (!next.bytesRead) break;
      length += next.bytesRead;
      cap(length, maximum);
      digest.update(buffer.subarray(0, next.bytesRead));
      if (expectedBytes !== undefined)
        parts.push(Buffer.from(buffer.subarray(0, next.bytesRead)));
    }
    const after = await file.stat({ bigint: true });
    const afterPath = await filesystem.lstat(filePath, { bigint: true });
    if (
      !regular(after) ||
      after.dev !== opened.dev ||
      !sameMetadata(after, opened) ||
      !pathMatchesHandle(afterPath, after) ||
      BigInt(length) !== opened.size ||
      (expectedHash !== undefined && digest.digest('hex') !== expectedHash)
    )
      fail('unavailable');
    return expectedBytes === undefined
      ? undefined
      : Buffer.concat(parts, length);
  } finally {
    await file.close();
  }
}

async function realContained(filePath, privateRoot, allowRoot = false) {
  const canonicalRoot = absolutePath(await filesystem.realpath(privateRoot));
  const canonicalFile = absolutePath(await filesystem.realpath(filePath));
  if (allowRoot && canonicalFile === canonicalRoot) return;
  containedPath(canonicalFile, canonicalRoot);
}

async function loadBootstrap(filePath, privateCwd) {
  await realContained(filePath, privateCwd);
  const info = await filesystem.lstat(filePath, { bigint: true });
  if (info.size > BigInt(LIMITS.frameBytes)) fail('limitsExceeded');
  const raw = await checkedFile(
    filePath,
    LIMITS.frameBytes,
    undefined,
    Number(info.size),
  );
  const bootstrap = parseBootstrap(raw, privateCwd);
  for (const input of bootstrap.inputs) {
    await realContained(input.filePath, bootstrap.privateRoot);
    await checkedFile(
      input.filePath,
      bootstrap.scope.limits.inputBytes,
      input.sha256,
      input.bytes,
    );
  }
  for (const output of bootstrap.outputs)
    await realContained(
      windowsPath.dirname(output.filePath),
      bootstrap.privateRoot,
      true,
    );
  return bootstrap;
}

// Authentication is host-issued custody, never page or model data. Cookie domains
// deliberately use exact host equality: parent-domain and sibling reuse is denied.
export function profileCookieHeader(origin, target, cookies) {
  const url = new URL(target);
  if (url.origin !== origin) fail('denied');
  const now = Date.now() / 1000;
  return cookies
    .filter((cookie) => {
      if (cookie.domain !== url.hostname || !cookie.path?.startsWith('/'))
        fail('denied');
      if (
        !/^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/.test(cookie.name) ||
        /[;\r\n\x00-\x20\x7f]/.test(cookie.value)
      )
        fail('denied');
      return (
        (!cookie.secure || url.protocol === 'https:') &&
        (cookie.expires === undefined ||
          cookie.expires === -1 ||
          cookie.expires > now) &&
        (url.pathname === cookie.path ||
          url.pathname.startsWith(
            cookie.path.endsWith('/') ? cookie.path : cookie.path + '/',
          ))
      );
    })
    .map((cookie) => cookie.name + '=' + cookie.value)
    .join('; ');
}
export function profileResponseCookies(origin, target, values) {
  const url = new URL(target);
  if (url.origin !== origin) fail('denied');
  cap(values.length, 64);
  return values.map((value) => {
    if (
      typeof value !== 'string' ||
      Buffer.byteLength(value) > 4096 ||
      /[\r\n\x00]/.test(value)
    )
      fail('denied');
    const [pair, ...attributes] = value.split(';');
    const split = pair.indexOf('=');
    if (split < 1) fail('denied');
    const cookie = {
      name: pair.slice(0, split).trim(),
      value: pair.slice(split + 1).trim(),
      domain: url.hostname,
      // RFC 6265 section 5.1.4 excludes the rightmost slash.
      path: url.pathname.slice(0, url.pathname.lastIndexOf('/')) || '/',
      httpOnly: false,
      secure: false,
      sameSite: 'Lax',
    };
    let maxAge;
    for (const attribute of attributes) {
      const [rawName, ...parts] = attribute.trim().split('=');
      const item = parts.join('=').trim();
      switch (rawName.toLowerCase()) {
        case 'domain':
          if (item.toLowerCase() !== url.hostname) fail('denied');
          break;
        case 'path':
          if (!item.startsWith('/')) fail('denied');
          cookie.path = item;
          break;
        case 'httponly':
          cookie.httpOnly = true;
          break;
        case 'secure':
          cookie.secure = true;
          break;
        case 'samesite': {
          const match = ['Strict', 'Lax', 'None'].find(
            (x) => x.toLowerCase() === item.toLowerCase(),
          );
          if (!match) fail('denied');
          cookie.sameSite = match;
          break;
        }
        case 'max-age':
          if (!/^-?[0-9]+$/.test(item)) fail('denied');
          maxAge = Number(item);
          if (!Number.isSafeInteger(maxAge)) fail('denied');
          break;
        case 'expires': {
          const expiry = Date.parse(item);
          if (!Number.isFinite(expiry)) fail('denied');
          cookie.expires = Math.max(1, expiry / 1000);
          break;
        }
        case 'partitioned':
          fail('denied');
      }
    }
    if (maxAge !== undefined)
      cookie.expires = Math.max(1, Date.now() / 1000 + maxAge);
    profileCookieHeader(origin, target, [cookie]);
    if (cookie.secure && url.protocol !== 'https:') fail('denied');
    if (cookie.sameSite === 'None' && !cookie.secure) fail('denied');
    if (cookie.name.startsWith('__Secure-') && !cookie.secure) fail('denied');
    if (
      cookie.name.startsWith('__Host-') &&
      (!cookie.secure ||
        cookie.path !== '/' ||
        attributes.some((x) => /^domain=/i.test(x.trim())))
    )
      fail('denied');
    return cookie;
  });
}
function profileAad(profile) {
  identifier(profile.id);
  integer(profile.revision);
  hash(profile.digest);
  const url = new URL(profile.origin);
  if (url.origin !== profile.origin) fail('denied');
  return Buffer.from(
    JSON.stringify({
      id: profile.id,
      revision: profile.revision,
      digest: profile.digest,
      origin: profile.origin,
    }),
  );
}
function validateProfileState(profile, state) {
  exact(state, ['cookies', 'origins']);
  array(state.cookies, 64);
  array(state.origins, 1);
  profileCookieHeader(profile.origin, profile.origin + '/', state.cookies);
  for (const item of state.origins) {
    if (item.origin !== profile.origin) fail('denied');
    exact(item, ['origin', 'localStorage']);
    array(item.localStorage, 64);
  }
  cap(Buffer.byteLength(JSON.stringify(state)), 32768);
  return state;
}
export function sealProfileState(profile, state) {
  hash(profile.keyHex);
  validateProfileState(profile, state);
  const iv = randomBytes(12),
    cipher = createCipheriv(
      'aes-256-gcm',
      Buffer.from(profile.keyHex, 'hex'),
      iv,
    );
  cipher.setAAD(profileAad(profile));
  const plaintext = Buffer.from(JSON.stringify(state));
  try {
    return {
      version: 1,
      iv: iv.toString('hex'),
      data: Buffer.concat([cipher.update(plaintext), cipher.final()]).toString(
        'base64',
      ),
      ...{ tag: cipher.getAuthTag().toString('hex') },
    };
  } finally {
    plaintext.fill(0);
  }
}
export function openProfileState(profile, sealed) {
  hash(profile.keyHex);
  exact(sealed, ['version', 'iv', 'tag', 'data']);
  if (
    sealed.version !== 1 ||
    !/^[a-f0-9]{24}$/.test(sealed.iv) ||
    !/^[a-f0-9]{32}$/.test(sealed.tag) ||
    typeof sealed.data !== 'string'
  )
    fail('denied');
  cap(Buffer.byteLength(sealed.data), 48000);
  const decipher = createDecipheriv(
    'aes-256-gcm',
    Buffer.from(profile.keyHex, 'hex'),
    Buffer.from(sealed.iv, 'hex'),
  );
  decipher.setAAD(profileAad(profile));
  decipher.setAuthTag(Buffer.from(sealed.tag, 'hex'));
  let plaintext;
  try {
    plaintext = Buffer.concat([
      decipher.update(Buffer.from(sealed.data, 'base64')),
      decipher.final(),
    ]);
    return validateProfileState(
      profile,
      JSON.parse(plaintext.toString('utf8')),
    );
  } catch {
    fail('denied');
  } finally {
    plaintext?.fill(0);
  }
}

function snapshotKey(key) {
  const roleMatch = key.match(/^([A-Za-z]+)(?: |$(?![\s\S]))/);
  if (!roleMatch) fail('unavailable');
  const role = roleMatch[1];
  let remaining = key.slice(role.length);
  let label = '';
  if (remaining.startsWith(' "')) {
    const quoted = remaining.slice(1).match(/^"(?:\\.|[^"\\])*"/);
    if (!quoted) fail('unavailable');
    try {
      label = JSON.parse(quoted[0]);
    } catch {
      fail('unavailable');
    }
    remaining = remaining.slice(quoted[0].length + 1);
  } else if (remaining.startsWith(' /')) {
    const boundary = remaining.indexOf(' [');
    label = remaining.slice(1, boundary < 0 ? undefined : boundary);
    if (!label.endsWith('/')) fail('unavailable');
    remaining = boundary < 0 ? '' : remaining.slice(boundary);
  }
  let ref;
  while (remaining) {
    const attribute = remaining.match(
      /^ \[([A-Za-z]+)(?:=([A-Za-z0-9_-]+))?\]/,
    );
    if (
      !attribute ||
      ![
        'checked',
        'disabled',
        'expanded',
        'active',
        'level',
        'pressed',
        'selected',
        'ref',
        'cursor',
      ].includes(attribute[1])
    )
      fail('unavailable');
    if (attribute[1] === 'ref') {
      if (
        ref ||
        !/^(?:f[1-9][0-9]*)?e[1-9][0-9]*$(?![\s\S])/.test(attribute[2] ?? '')
      )
        fail('unavailable');
      ref = attribute[2];
    }
    remaining = remaining.slice(attribute[0].length);
  }
  validString(label);
  return { role, label, ref };
}

/** Adapt maintained snapshot serialization; no DOM traversal or selector engine. */
export function snapshotEvidence(snapshot, yaml) {
  validString(snapshot);
  cap(Buffer.byteLength(snapshot), LIMITS.frameBytes);
  if (!snapshot.trim()) return { entries: [], omittedNodes: 0 };
  let document;
  try {
    document = yaml.parseDocument(snapshot, {
      uniqueKeys: true,
      customTags: [],
    });
  } catch {
    fail('unavailable');
  }
  if (document.errors.length) fail('unavailable');
  let count = 0;
  function checkAst(node, depth) {
    if (!node) return;
    cap(++count, LIMITS.jsonNodes);
    cap(depth, 24);
    if (yaml.isAlias(node) || node.tag) fail('unavailable');
    for (const child of node.items ?? []) {
      if (yaml.isPair(child)) {
        checkAst(child.key, depth + 1);
        checkAst(child.value, depth + 1);
      } else checkAst(child, depth + 1);
    }
  }
  checkAst(document.contents, 1);
  let source;
  try {
    source = document.toJS({ maxAliasCount: 0 });
  } catch {
    fail('unavailable');
  }
  if (!Array.isArray(source)) fail('unavailable');
  const entries = [];
  let omittedNodes = 0;
  function append(entry) {
    // The pinned renderer omits names longer than900 characters. An unnamed
    // interactable therefore cannot honestly be published as a complete name.
    if (!entry.label) {
      omittedNodes++;
      return;
    }
    entries.push(entry);
    cap(entries.length + omittedNodes, LIMITS.jsonNodes);
  }
  function visit(sequence) {
    for (const item of sequence) {
      if (typeof item === 'string') {
        append(snapshotKey(item));
        continue;
      }
      if (!item || typeof item !== 'object' || Array.isArray(item))
        fail('unavailable');
      for (const [key, value] of Object.entries(item)) {
        if (key.startsWith('/')) continue;
        if (key === 'text') {
          if (typeof value !== 'string') fail('unavailable');
          append({ role: 'text', label: value });
          continue;
        }
        append(snapshotKey(key));
        if (Array.isArray(value)) visit(value);
        else if (['string', 'number', 'boolean'].includes(typeof value))
          append({ role: 'text', label: String(value) });
        else fail('unavailable');
      }
    }
  }
  visit(source);
  return { entries, omittedNodes };
}

// All page JavaScript below is fixed worker source installed before page scripts.
// These application hooks are not an OS egress or browser-memory sandbox.
function installDocumentGuards({ marker, report, ownerNavigation }) {
  const originalDocument = document;
  const documentToken = Array.from(
    crypto.getRandomValues(new Uint8Array(16)),
    (value) => value.toString(16).padStart(2, '0'),
  ).join('');
  let revision = 0;
  let ready = false;
  const observer = new MutationObserver(() => {
    revision++;
  });
  observer.observe(document, {
    subtree: true,
    childList: true,
    attributes: true,
    characterData: true,
  });
  Object.defineProperty(globalThis, marker, {
    configurable: false,
    writable: false,
    value: () => ({
      token: documentToken,
      revision,
      same: document === originalDocument,
      ready,
    }),
  });
  const refuse = function () {
    try {
      Promise.resolve(globalThis[report]()).catch(() => {});
    } catch {}
    throw new DOMException('Capability unavailable', 'NotAllowedError');
  };
  for (const name of [
    'open',
    'RTCPeerConnection',
    'webkitRTCPeerConnection',
    'WebSocket',
    'Worker',
    'SharedWorker',
    'EventSource',
    'WebTransport',
    'showOpenFilePicker',
    'showSaveFilePicker',
    'showDirectoryPicker',
  ]) {
    for (
      let prototype = Object.getPrototypeOf(globalThis);
      prototype;
      prototype = Object.getPrototypeOf(prototype)
    ) {
      if (Object.hasOwn(prototype, name))
        Object.defineProperty(prototype, name, {
          configurable: false,
          writable: false,
          value: refuse,
        });
    }
    Object.defineProperty(globalThis, name, {
      configurable: false,
      writable: false,
      value: refuse,
    });
  }
  for (const [object, methods] of [
    [
      navigator.mediaDevices,
      ['getUserMedia', 'getDisplayMedia', 'enumerateDevices'],
    ],
    [navigator.geolocation, ['getCurrentPosition', 'watchPosition']],
    [navigator.clipboard, ['read', 'readText', 'write', 'writeText']],
    [navigator.permissions, ['query', 'request']],
    [navigator.serviceWorker, ['register']],
    [navigator.credentials, ['get', 'create', 'store']],
    [navigator.usb, ['requestDevice']],
    [navigator.serial, ['requestPort']],
    [navigator.hid, ['requestDevice']],
    [navigator.bluetooth, ['requestDevice']],
  ]) {
    if (object)
      for (const method of methods) {
        const prototype = Object.getPrototypeOf(object);
        if (prototype && Object.hasOwn(prototype, method))
          Object.defineProperty(prototype, method, {
            configurable: false,
            writable: false,
            value: refuse,
          });
        Object.defineProperty(object, method, {
          configurable: false,
          writable: false,
          value: refuse,
        });
      }
  }
  if (globalThis.Notification)
    Object.defineProperty(Notification, 'requestPermission', {
      configurable: false,
      writable: false,
      value: refuse,
    });
  if (ownerNavigation) {
    const announce = (value) => {
      try {
        Promise.resolve(
          globalThis[ownerNavigation](new URL(value, document.baseURI).origin),
        ).catch(() => {});
      } catch {}
    };
    document.addEventListener(
      'submit',
      (event) => {
        if (event.isTrusted && event.target instanceof HTMLFormElement)
          announce(event.submitter?.formAction || event.target.action);
      },
      true,
    );
    document.addEventListener(
      'click',
      (event) => {
        if (!event.isTrusted || !(event.target instanceof Element)) return;
        const link = event.target.closest('a[href]');
        if (link) announce(link.href);
      },
      true,
    );
  }
  ready = true;
}

function staticFailure(error, dispatched = false) {
  const code = error instanceof BrowserWorkerError ? error.code : 'unavailable';
  return new BrowserWorkerError(
    code,
    dispatched ? 'mayHaveOccurred' : 'notDispatched',
  );
}

/** Owned deny-only fallback for browser HTTP that does not enter context routing. */
export async function createDenyProxy({ signal, onRequest, onFailure } = {}) {
  if (signal?.aborted) fail('cancelled');
  const sockets = new Set();
  const socketJoins = new Set();
  const observed = new WeakSet();
  let closing;
  let started = false;
  let resolveStart, rejectStart;
  const startup = new Promise((resolve, reject) => {
    resolveStart = resolve;
    rejectStart = reject;
  });
  const notify = (callback, code, metadata) => {
    try {
      callback?.(code, metadata);
    } catch {
      /* Only static refusal is exposed. */
    }
  };
  const refused = (socket, code = 'denied', metadata) => {
    if (socket) observed.add(socket);
    notify(onRequest, code, metadata);
  };
  const response =
    'HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: 0\r\n\r\n';
  const denySocket = (socket, count = true, metadata) => {
    if (!socket.destroyed && !socket.writableEnded) socket.end(response);
    if (count) refused(socket, 'denied', metadata);
  };
  const server = createServer(
    {
      maxHeaderSize: 8192,
      headersTimeout: 1000,
      requestTimeout: 1000,
      keepAliveTimeout: 1,
    },
    (request, reply) => {
      reply.writeHead(403, { connection: 'close', 'content-length': '0' });
      reply.end();
      refused(request.socket);
    },
  );
  server.maxHeadersCount = 32;
  server.maxConnections = 8;
  server.on('connection', (socket) => {
    sockets.add(socket);
    let joined;
    joined = new Promise((resolve) => {
      socket.once('close', () => {
        sockets.delete(socket);
        socketJoins.delete(joined);
        resolve();
      });
    });
    socketJoins.add(joined);
    socket.on('error', () => {});
    socket.setTimeout(1000, () => {
      if (!observed.has(socket)) refused(socket);
      socket.destroy();
    });
    if (closing) socket.destroy();
  });
  server.on('connect', (request, socket) =>
    denySocket(socket, true, { method: 'CONNECT', authority: request.url }),
  );
  server.on('upgrade', (_request, socket) => denySocket(socket));
  server.on('clientError', (_error, socket) =>
    denySocket(socket, !observed.has(socket)),
  );
  server.on('drop', () => refused(undefined, 'limitsExceeded'));
  server.on('error', () => {
    if (!started) rejectStart(new BrowserWorkerError('unavailable'));
    else notify(onFailure, 'unavailable');
  });
  server.once('listening', () => {
    started = true;
    resolveStart();
  });
  async function close() {
    if (!closing)
      closing = (async () => {
        // A late successful bind is still owned; never acknowledge cleanup while
        // a listen callback can create a new unjoined listener.
        await startup.catch(() => {});
        const stopped = new Promise((resolve, reject) => {
          server.close((error) => {
            if (error && error.code !== 'ERR_SERVER_NOT_RUNNING') reject(error);
            else resolve();
          });
        });
        for (const socket of sockets) socket.destroy();
        await stopped;
        await Promise.all([...socketJoins]);
        signal?.removeEventListener('abort', abort);
      })();
    return closing;
  }
  const abort = () => {
    void close().catch(() => notify(onFailure, 'unavailable'));
  };
  signal?.addEventListener('abort', abort, { once: true });
  try {
    try {
      server.listen({ host: '127.0.0.1', port: 0, exclusive: true });
    } catch {
      rejectStart(new BrowserWorkerError('unavailable'));
    }
    await startup;
    if (signal?.aborted) {
      await close();
      fail('cancelled');
    }
    const address = server.address();
    if (
      !address ||
      typeof address !== 'object' ||
      address.address !== '127.0.0.1'
    )
      fail('unavailable');
    // After admission the session owns ordering: retain this deny listener until
    // its browser and routed requests have joined, then explicitly close it.
    signal?.removeEventListener('abort', abort);
    return { server: 'http://127.0.0.1:' + address.port, close };
  } catch (error) {
    await close().catch(() => {});
    throw staticFailure(error);
  }
}

/**
 * One numeric HTTPS dial. The dependency seam is module-local test wiring, never
 * bootstrap/IPC data. Owning this object includes joining close(), even when its
 * response promise has already failed. No resolver, pool, retry or redirect path.
 */
export function createPinnedHttpsRequest(
  scope,
  route,
  {
    signal,
    requestImpl = httpsRequest,
    cookie,
    contentType,
    body,
    requestOrigin,
    requestedMethod,
    requestedHeaders,
  } = {},
) {
  validateScope(scope);
  if (
    !scope.publicIpv4Pin ||
    (!scope.interactive &&
      !scope.routes.some(
        (candidate) =>
          candidate.id === route?.id &&
          candidate.method === route.method &&
          (scope.authenticated || candidate.method === 'GET') &&
          candidate.path === route.path,
      ))
  )
    fail();
  const selectedOrigin = scope.interactive
    ? interactiveUrl(scope, (route.origin ?? scope.origin) + route.path).origin
    : scope.origin;
  const selectedPin = scope.interactive
    ? scope.interactive.origins.find((o) => o.origin === selectedOrigin)
        .publicIpv4Pin
    : scope.publicIpv4Pin;
  const hostname = publicHostname(selectedOrigin);
  if (
    requestOrigin !== undefined &&
    (!scope.interactive ||
      !scope.interactive.origins.some(
        (origin) => origin.origin === requestOrigin,
      ))
  )
    fail('denied');
  if (
    requestedMethod !== undefined &&
    (route.method !== 'OPTIONS' ||
      !scope.interactive ||
      !['GET', 'HEAD', 'POST', 'PUT', 'PATCH', 'DELETE'].includes(
        requestedMethod,
      ))
  )
    fail('denied');
  if (
    requestedHeaders !== undefined &&
    (route.method !== 'OPTIONS' ||
      typeof requestedHeaders !== 'string' ||
      requestedHeaders.length > 1024 ||
      requestedHeaders.split(',').length > 16 ||
      !requestedHeaders
        .split(',')
        .every((name) => /^[a-z0-9-]+$/.test(name.trim())))
  )
    fail('denied');
  const resources = new Map();
  let closing = false,
    settled = false,
    uncertain = false,
    scheduled = false;
  let resolveResponse, rejectResponse, resolveClosed;
  const response = new Promise((resolve, reject) => {
    resolveResponse = resolve;
    rejectResponse = reject;
  });
  // Close/cancellation may precede the caller's first await. Retain rejection
  // ownership immediately rather than leaking a process-level unhandled error.
  response.catch(() => {});
  const closed = new Promise((resolve) => {
    resolveClosed = resolve;
  });
  const refusal = () =>
    new BrowserWorkerError(signal?.aborted ? 'cancelled' : 'unavailable');
  const reject = () => {
    if (!settled) {
      settled = true;
      rejectResponse(refusal());
    }
  };
  const checkClosed = () => {
    if (
      !closing ||
      uncertain ||
      scheduled ||
      [...resources.values()].some((value) => !value)
    )
      return;
    scheduled = true;
    // Account for socket/response delivery queued around request cancellation.
    // A request-close acknowledgement, not an abort promise, is authoritative.
    setImmediate(() => {
      scheduled = false;
      if (!uncertain && [...resources.values()].every(Boolean)) {
        signal?.removeEventListener('abort', abort);
        resolveClosed();
      }
    });
  };
  const destroy = (resource) => {
    try {
      if (!resource.destroyed) resource.destroy();
    } catch {
      uncertain = true;
    }
  };
  const close = () => {
    closing = true;
    reject();
    for (const resource of resources.keys()) destroy(resource);
    checkClosed();
    return closed;
  };
  const abort = () => {
    void close();
  };
  const own = (resource) => {
    if (resources.has(resource)) return;
    resources.set(resource, resource.closed === true);
    resource.once('close', () => {
      resources.set(resource, true);
      checkClosed();
    });
    resource.on('error', () => {
      reject();
      void close();
    });
    if (closing) destroy(resource);
  };
  signal?.addEventListener('abort', abort, { once: true });
  if (signal?.aborted) {
    void close();
    return { response, close };
  }
  try {
    const request = requestImpl({
      hostname: selectedPin,
      port: 443,
      path: route.path,
      method: route.method,
      servername: hostname,
      headers: {
        Host: hostname,
        'Accept-Encoding': 'identity',
        ...(scope.authenticated && selectedOrigin === scope.origin && cookie
          ? { Cookie: cookie }
          : {}),
        ...((scope.authenticated || scope.interactive) && contentType
          ? { 'Content-Type': contentType }
          : {}),
        ...((scope.authenticated || scope.interactive) &&
        (requestOrigin || !['GET', 'HEAD'].includes(route.method))
          ? { Origin: requestOrigin ?? scope.origin }
          : {}),
        ...(route.method === 'OPTIONS' && requestedMethod
          ? { 'Access-Control-Request-Method': requestedMethod }
          : {}),
        ...(route.method === 'OPTIONS' && requestedHeaders
          ? { 'Access-Control-Request-Headers': requestedHeaders }
          : {}),
      },
      rejectUnauthorized: true,
      agent: false,
      maxHeaderSize: 8192,
    });
    own(request);
    request.on('socket', (socket) => own(socket));
    request.on('response', (incoming) => {
      own(incoming);
      if (closing || settled) {
        destroy(incoming);
        return;
      }
      try {
        const headers = new Headers();
        for (let index = 0; index < incoming.rawHeaders.length; index += 2)
          headers.append(
            incoming.rawHeaders[index],
            incoming.rawHeaders[index + 1],
          );
        const body = Readable.toWeb(incoming, {
          // One queued transport chunk, not a high-water mark measured in chunks
          // copied from Node's byte-based stream buffering.
          strategy: { highWaterMark: 1, size: () => 1 },
        });
        settled = true;
        resolveResponse({
          status: incoming.statusCode,
          headers,
          body,
          redirected: false,
        });
      } catch {
        reject();
        void close();
      }
    });
    // Unexpected close before headers must settle the response too, while still
    // retaining any independently delivered socket until its close event.
    request.once('close', () => {
      if (!settled) {
        reject();
        void close();
      }
    });
    request.end(
      (scope.authenticated || scope.interactive) &&
        !['GET', 'HEAD'].includes(route.method)
        ? body
        : undefined,
    );
  } catch {
    reject();
    void close();
  }
  return { response, close };
}

export async function createMaintainedSession(bootstrap, dependencies) {
  const {
    chromium,
    yaml,
    environment,
    onFatal = () => {},
    fetchImpl = fetch,
  } = dependencies;
  const scope = bootstrap.scope;
  const remaining = bootstrap.expiresAtMs - Date.now();
  if (remaining <= 0 || remaining > scope.limits.scopeMs) fail('deadline');
  const now = () => Math.floor(performance.now());
  const expiresAtMs = now() + remaining;
  const shutdown = new AbortController();
  const cancelStartup = () => shutdown.abort();
  dependencies.signal?.addEventListener('abort', cancelStartup, { once: true });
  if (dependencies.signal?.aborted) shutdown.abort();
  const contextId = 'context-' + randomUUID();
  const pageId = 'page-' + randomUUID();
  const marker = '__anima_document_' + randomUUID().replaceAll('-', '');
  const report = '__anima_refused_' + randomUUID().replaceAll('-', '');
  const ownerNavigation =
    '__anima_owner_navigation_' + randomUUID().replaceAll('-', '');
  const privateDiagnostics = {
    blockedRequests: 0,
    redirectsRejected: 0,
    fetchAttempts: 0,
    pageViolations: 0,
  };
  const network = {
    requestsUsed: 0,
    blockedRequests: 0,
    pageId,
    effectActive: false,
    navigationRemaining: 0,
    nowMs: now(),
    expiresAtMs,
    signal: shutdown.signal,
  };
  let browser,
    context,
    page,
    denyProxy,
    closing,
    fatal,
    expectPage = false,
    expectedNavigation;
  let profileLoaded = false;
  let ownerControl = false;
  let admittedNavigationAuthority;
  let ownerNavigationActive = false;
  let ownerNavigationExpires = 0;
  let ownerNavigationUses = 0;
  let pendingRedirect;
  let redirectHops = 0;
  let currentUrl = 'about:blank';
  let actionsUsed = 0,
    serial = 0,
    latest;
  let tokens = new Map();
  const fetchedLedger = { bytes: 0 };
  const usedOutputs = new Set();
  const routeTasks = new Set();
  const publicRequests = new Set();
  let finishStartup;
  const startup = new Promise((resolve) => {
    finishStartup = resolve;
  });
  const timeout = () => {
    if (fatal) throw fatal;
    if (shutdown.signal.aborted) fail('cancelled');
    if (now() >= expiresAtMs) fail('deadline');
    return Math.max(1, Math.min(10000, expiresAtMs - now()));
  };
  const counters = () => ({
    actionsUsed,
    requestsUsed: network.requestsUsed,
    wireBytesUsed: 0,
  });
  async function disposeTokens() {
    const previous = tokens;
    tokens = new Map();
    await Promise.allSettled(
      [...previous.values()].map((item) => item.handle?.dispose()),
    );
  }
  async function close() {
    if (!closing) {
      shutdown.abort();
      admittedNavigationAuthority = undefined;
      ownerNavigationActive = false;
      network.effectActive = false;
      closing = (async () => {
        await startup;
        await disposeTokens();
        let failed = false;
        if (bootstrap.profile && context && profileLoaded && !fatal) {
          try {
            const profile = bootstrap.profile;
            let save = !profile.ownerLogin;
            if (profile.ownerLogin) {
              try {
                const marker = await filesystem.readFile(
                  windowsPath.join(bootstrap.privateRoot, 'profile.finish'),
                  'utf8',
                );
                save = marker === 'finish';
              } catch {}
            }
            if (save) {
              const sealed = sealProfileState(
                profile,
                await context.storageState(),
              );
              const stage = profile.sealedPath + '.' + randomUUID() + '.tmp';
              const file = await filesystem.open(stage, 'wx', 0o600);
              try {
                await file.writeFile(JSON.stringify(sealed));
                await file.sync();
              } finally {
                await file.close();
              }
              await filesystem.rename(stage, profile.sealedPath);
            }
          } catch {
            failed = true;
          }
        }
        try {
          if (context)
            await context.close({ reason: 'Browser worker stopped' });
        } catch {
          failed = true;
        }
        try {
          if (browser)
            await browser.close({ reason: 'Browser worker stopped' });
        } catch {
          failed = true;
        }
        await Promise.allSettled([...routeTasks]);
        // Public response rejection does not imply socket retirement. This join
        // deliberately remains pending when the native host must quarantine/kill.
        await Promise.all([...publicRequests].map((owner) => owner.close()));
        try {
          if (denyProxy) await denyProxy.close();
        } catch {
          failed = true;
        }
        dependencies.signal?.removeEventListener('abort', cancelStartup);
        if (failed) fail('unavailable');
        return { engineClosed: true, counters: counters() };
      })();
    }
    return closing;
  }
  function retire(code = 'denied') {
    if (!fatal) {
      fatal = new BrowserWorkerError(code);
      privateDiagnostics.pageViolations++;
      shutdown.abort();
      void close().catch(() => {});
      onFatal(fatal);
    }
  }
  const leaseTimer = setTimeout(() => retire('deadline'), remaining);
  leaseTimer.unref();
  function allowedRoute(value, method = 'GET') {
    if (scope.interactive && typeof value === 'string') {
      try {
        const url = interactiveUrl(
          scope,
          new URL(
            value,
            currentUrl === 'about:blank' ? scope.origin : currentUrl,
          ).href,
        );
        return {
          id: scope.routes[0].id,
          method,
          path: url.pathname + url.search,
          origin: url.origin,
        };
      } catch {
        return undefined;
      }
    }
    return scope.routes.find(
      (route) =>
        route.method === method &&
        (value === route.path || value === scope.origin + route.path),
    );
  }
  async function writeArtifact(kind, body) {
    timeout();
    const slot = bootstrap.outputs.find(
      (candidate) =>
        candidate.kind === (kind === 'screenshot' ? 'screenshot' : 'csv') &&
        !usedOutputs.has(candidate.id),
    );
    if (!slot || usedOutputs.size >= LIMITS.sessionArtifacts)
      fail('limitsExceeded');
    cap(
      body.length,
      kind === 'screenshot'
        ? scope.limits.screenshotBytes
        : scope.limits.outputBytes,
    );
    if (!body.length) fail('invalid');
    usedOutputs.add(slot.id);
    await realContained(
      windowsPath.dirname(slot.filePath),
      bootstrap.privateRoot,
      true,
    );
    const output = await filesystem.open(slot.filePath, 'wx', 0o600);
    try {
      timeout();
      await output.writeFile(body);
      await output.sync();
    } finally {
      await output.close();
    }
    timeout();
    return {
      id: slot.id,
      kind,
      bytes: body.length,
      sha256: createHash('sha256').update(body).digest('hex'),
    };
  }
  async function guardedResponse(
    intercepted,
    headers = {},
    body,
    redirects = 0,
  ) {
    network.nowMs = now();
    if (bootstrap.profile?.ownerLogin || ownerControl) {
      network.effectActive = true;
      if (intercepted.navigation) {
        const candidate = scope.routes.find(
          (route) =>
            scope.origin + route.path === intercepted.url &&
            route.method === intercepted.method,
        );
        network.navigationRouteId = candidate?.id;
        network.navigationRemaining = 1;
      }
    }
    const target = guardRequest(scope, intercepted, network);
    const targetUrl = (target.origin ?? scope.origin) + target.path;
    let cookie;
    if (
      bootstrap.profile &&
      (target.origin ?? scope.origin) === scope.origin &&
      (intercepted.download || headers.cookie !== undefined)
    ) {
      const owned = profileCookieHeader(
        scope.origin,
        targetUrl,
        await context.cookies(targetUrl),
      );
      if (intercepted.download) cookie = owned;
      else {
        // Routed requests retain Chromium's credentials/SameSite decision.
        // A cookie in the owned jar is not permission to add it to an omit request.
        if (
          typeof headers.cookie !== 'string' ||
          Buffer.byteLength(headers.cookie) > 32768 ||
          /[\r\n\x00]/.test(headers.cookie)
        )
          fail('denied');
        const selected = headers.cookie.split(';').map((pair) => pair.trim());
        const permitted = new Set(owned.split('; '));
        if (
          !selected.length ||
          selected.length > 64 ||
          selected.some((pair) => !pair || !permitted.has(pair))
        )
          fail('denied');
        cookie = headers.cookie;
      }
    }
    if (intercepted.navigation) expectedNavigation = targetUrl;
    timeout();
    const controller = new AbortController();
    const abort = () => controller.abort();
    shutdown.signal.addEventListener('abort', abort, { once: true });
    const timer = setTimeout(abort, timeout());
    const downloading = intercepted.download;
    const responseControl = {
      // Browser resources and typed downloads charge one live chunk ledger.
      // Each actual request keeps its own cancellation and deadline.
      get aggregateBytes() {
        return fetchedLedger.bytes;
      },
      set aggregateBytes(value) {
        fetchedLedger.bytes = value;
      },
      deadlineMs: Math.min(expiresAtMs, now() + 10000),
      now,
      signal: controller.signal,
      abort,
      diagnostics: privateDiagnostics,
    };
    let publicOwner;
    try {
      privateDiagnostics.fetchAttempts++;
      let response;
      if (scope.publicIpv4Pin) {
        publicOwner = createPinnedHttpsRequest(scope, target, {
          signal: controller.signal,
          requestImpl: dependencies.httpsRequestImpl,
          cookie,
          contentType: headers['content-type'],
          body,
          ...(scope.interactive && headers.origin
            ? { requestOrigin: headers.origin }
            : {}),
          ...(target.method === 'OPTIONS'
            ? {
                requestedMethod: headers['access-control-request-method'],
                requestedHeaders: headers['access-control-request-headers'],
              }
            : {}),
        });
        publicRequests.add(publicOwner); // No yield between dispatch and ownership.
        response = await publicOwner.response;
      } else
        response = await fetchImpl(scope.origin + target.path, {
          method: target.method,
          redirect: 'manual',
          signal: controller.signal,
          headers: {
            'accept-encoding': 'identity',
            ...(cookie ? { cookie } : {}),
            ...(bootstrap.profile && target.method === 'POST'
              ? { origin: scope.origin }
              : {}),
            ...(headers['content-type']
              ? { 'content-type': headers['content-type'] }
              : {}),
          },
          ...(target.method === 'POST'
            ? { body: body ?? Buffer.alloc(0) }
            : {}),
          credentials: 'omit',
          cache: 'no-store',
        });
      let responseCookieHeaders = {};
      if (
        bootstrap.profile &&
        (target.origin ?? scope.origin) === scope.origin
      ) {
        const values = response.headers.getSetCookie();
        const received = profileResponseCookies(
          scope.origin,
          targetUrl,
          values,
        );
        if (intercepted.download) {
          if (received.length) await context.addCookies(received);
        } else if (received.length) {
          // Chromium applies routed response cookies, including credentials:omit.
          // Playwright splits this bounded newline form into separate Set-Cookie headers.
          responseCookieHeaders = { 'set-cookie': values.join('\n') };
        }
      }
      if (
        scope.interactive &&
        response.status >= 300 &&
        response.status < 400
      ) {
        if (redirects >= 5 || !response.headers.get('location')) fail('denied');
        const nextUrl = interactiveUrl(
          scope,
          new URL(response.headers.get('location'), targetUrl).href,
        );
        if (intercepted.navigation) {
          if (
            !['GET', 'HEAD'].includes(intercepted.method) &&
            response.status !== 303 &&
            ![301, 302].includes(response.status)
          )
            fail('denied');
          await response.body?.cancel();
          // Chromium can attempt a proxy CONNECT before interception on a
          // fulfilled HTTP redirect. Issue a fresh guarded navigation instead;
          // the destination remains the actual document URL for relative assets.
          if (++redirectHops > 5) fail('denied');
          pendingRedirect = nextUrl.href;
          if (admittedNavigationAuthority)
            admittedNavigationAuthority = nextUrl.hostname + ':443';
          const literal = JSON.stringify(nextUrl.href).replaceAll(
            '<',
            '\\u003c',
          );
          const nonce = randomBytes(16).toString('hex');
          return {
            status: 200,
            headers: {
              ...responseCookieHeaders,
              'content-type': 'text/html; charset=utf-8',
              'cache-control': 'no-store',
              'content-security-policy':
                "default-src 'none'; script-src 'nonce-" +
                nonce +
                "'; base-uri 'none'; form-action 'none'",
            },
            body: Buffer.from(
              '<!doctype html><script nonce="' +
                nonce +
                '">location.replace(' +
                literal +
                ')</script>',
            ),
          };
        }
        if (
          !intercepted.download &&
          nextUrl.origin !== new URL(targetUrl).origin
        )
          fail('denied');
        let method = intercepted.method;
        if (
          response.status === 303 ||
          ([301, 302].includes(response.status) && method === 'POST')
        )
          method = 'GET';
        if (!['GET', 'HEAD'].includes(method)) fail('denied');
        await response.body?.cancel();
        if (publicOwner) {
          await publicOwner.close();
          publicRequests.delete(publicOwner);
          publicOwner = undefined;
        }
        if (intercepted.navigation) expectedNavigation = nextUrl.href;
        return guardedResponse(
          {
            ...intercepted,
            url: nextUrl.href,
            method,
            bodyBytes: 0,
            navigation: false,
          },
          {},
          undefined,
          redirects + 1,
        );
      }
      const acceptedKinds = {
        'text/csv': 'csv',
        'text/plain': 'text',
        'application/json': 'json',
        'application/pdf': 'pdf',
        'image/png': 'png',
        'image/jpeg': 'jpeg',
      };
      const downloadKind = scope.interactive
        ? acceptedKinds[
            response.headers
              .get('content-type')
              ?.split(';')[0]
              .trim()
              .toLowerCase()
          ]
        : 'csv';
      if (downloading && !downloadKind) fail('denied');
      const bounded = await readBoundedResponse(
        response,
        {
          kind: downloading ? downloadKind : 'asset',
          bodyBytes: downloading
            ? scope.limits.outputBytes
            : (scope.interactive?.resourceBytes ?? 65536),
          totalBytes: scope.interactive?.networkBytes ?? 1048576,
          interactive: Boolean(scope.interactive),
          allowAttachment: downloading,
        },
        responseControl,
      );
      timeout();
      return {
        ...bounded,
        headers: { ...bounded.headers, ...responseCookieHeaders },
        kind: downloadKind,
      };
    } finally {
      clearTimeout(timer);
      shutdown.signal.removeEventListener('abort', abort);
      if (publicOwner) {
        await publicOwner.close();
        publicRequests.delete(publicOwner);
      }
    }
  }
  async function routeHttp(route) {
    const request = route.request();
    let requestPage,
      isMainFrame = false;
    try {
      if (scope.interactive) {
        let previous = request.redirectedFrom?.(),
          hops = 0;
        while (previous) {
          if (++hops > 5) fail('denied');
          previous = previous.redirectedFrom?.();
        }
      }
      const frame = request.frame();
      requestPage = frame.page();
      isMainFrame = frame === requestPage.mainFrame();
    } catch {}
    try {
      const headers = await request.allHeaders();
      const body = request.postDataBuffer();
      const bounded = await guardedResponse(
        {
          pageId: requestPage === page ? pageId : 'foreign-page',
          isMainFrame,
          channel: 'http',
          method: request.method(),
          url: request.url(),
          navigation: request.isNavigationRequest(),
          download: false,
          bodyBytes: body?.length ?? 0,
          headerBytes: Object.entries(headers).reduce(
            (sum, [key, value]) =>
              sum + Buffer.byteLength(key) + Buffer.byteLength(value) + 4,
            0,
          ),
        },
        headers,
        body,
      );
      // Joining physical public closure may yield after the last body check.
      // Stop/deadline must still fence publication after that join.
      timeout();
      await route.fulfill({
        status: bounded.status,
        headers: { ...bounded.headers, 'cache-control': 'no-store' },
        body: bounded.body,
      });
    } catch (error) {
      try {
        await route.abort('blockedbyclient');
      } catch {}
      if (!shutdown.signal.aborted)
        retire(
          error instanceof BrowserWorkerError ? error.code : 'unavailable',
        );
    }
  }
  function attachPage(owned) {
    owned.on('load', () => {
      if (
        ownerNavigationActive &&
        (!pendingRedirect || owned.url() === pendingRedirect)
      ) {
        admittedNavigationAuthority = undefined;
        ownerNavigationActive = false;
        pendingRedirect = undefined;
      }
    });
    owned.on('popup', () => retire('denied'));
    owned.on('worker', () => retire('denied'));
    owned.on('crash', () => retire('unavailable'));
    owned.on('dialog', (dialog) => {
      void dialog.dismiss().catch(() => {});
      retire('denied');
    });
    owned.on('frameattached', (frame) => {
      if (frame !== owned.mainFrame()) retire('denied');
    });
    owned.on('download', (download) => {
      void download.cancel().catch(() => {});
      retire('denied');
    });
    owned.on('framenavigated', (frame) => {
      if (frame !== owned.mainFrame()) {
        retire('denied');
        return;
      }
      if (frame.url() === 'about:blank' && currentUrl === 'about:blank') return;
      if (scope.interactive) {
        try {
          interactiveUrl(scope, frame.url());
        } catch {
          retire('denied');
          return;
        }
        currentUrl = frame.url();
        expectedNavigation = undefined;
        return;
      }
      if (!network.effectActive || frame.url() !== expectedNavigation) {
        retire('denied');
        return;
      }
      currentUrl = frame.url();
      expectedNavigation = undefined;
    });
    owned.on('close', () => {
      if (!closing) retire('unavailable');
    });
  }
  async function documentMarker() {
    timeout();
    const value = await page.evaluate((key) => globalThis[key](), marker);
    exact(value, ['token', 'revision', 'same', 'ready']);
    if (value.ready !== true) fail('unavailable');
    if (
      typeof value.token !== 'string' ||
      !/^[a-f0-9]{32}$(?![\s\S])/.test(value.token) ||
      value.same !== true
    )
      fail('stale');
    integer(value.revision, false);
    return value;
  }
  async function currentEvidence() {
    const before = await documentMarker();
    const snapshot = await page.ariaSnapshot({
      mode: 'ai',
      depth: 8,
      timeout: timeout(),
    });
    const after = await documentMarker();
    if (before.token !== after.token || before.revision !== after.revision)
      fail('stale');
    cap(Buffer.byteLength(snapshot), LIMITS.frameBytes);
    return {
      marker: after,
      snapshot,
      fingerprint: createHash('sha256').update(snapshot).digest('hex'),
    };
  }
  async function observeOnce(artifacts = []) {
    const evidence = await currentEvidence();
    const parsed = snapshotEvidence(evidence.snapshot, yaml);
    const route = scope.interactive
      ? scope.routes[0]
      : scope.routes.find(
          (candidate) => scope.origin + candidate.path === page.url(),
        );
    if (!route) fail('denied');
    await disposeTokens();
    const generation = ++serial;
    const candidates = parsed.entries.map((entry, index) => ({
      node: { token: 'node-' + generation + '-' + index },
      role: entry.role,
      label: entry.label,
    }));
    // Reserve enough exact envelope/identity/counter space even at64-character IDs.
    const observation = boundObservation(
      scope,
      {
        documentGeneration: generation,
        ...(scope.interactive ? { currentUrl: page.url() } : {}),
        routeId: route.id,
        nodes: candidates,
        shortened: parsed.omittedNodes > 0,
        omittedNodes: parsed.omittedNodes,
        artifacts,
      },
      Math.max(1, scope.limits.frameBytes - 1024),
    );
    for (const node of observation.nodes) {
      const entry =
        parsed.entries[
          candidates.findIndex(
            (candidate) => candidate.node.token === node.node.token,
          )
        ];
      if (!entry.ref) continue;
      const locator = page.locator('aria-ref=' + entry.ref);
      if ((await locator.count()) !== 1) fail('stale');
      const handle = await locator.elementHandle({ timeout: timeout() });
      if (!handle || (await handle.ownerFrame()) !== page.mainFrame())
        fail('stale');
      tokens.set(node.node.token, { ref: entry.ref, handle });
    }
    const after = await documentMarker();
    if (
      after.token !== evidence.marker.token ||
      after.revision !== evidence.marker.revision
    )
      fail('stale');
    const document = {
      documentId: 'document-' + evidence.marker.token,
      documentGeneration: generation,
      observationDigest: canonicalDigest(observation),
    };
    latest = {
      document,
      marker: evidence.marker,
      fingerprint: evidence.fingerprint,
    };
    return { contextId, pageId, document, observation, counters: counters() };
  }
  async function observe(artifacts = []) {
    // Asynchronous JS forms may mutate while their read-only evidence is being
    // collected. Re-read evidence only; never repeat the dispatched action.
    for (let attempt = 0; ; attempt++) {
      try {
        return await observeOnce(artifacts);
      } catch (error) {
        if (
          !(error instanceof BrowserWorkerError) ||
          error.code !== 'stale' ||
          attempt >= 2
        )
          throw error;
        timeout();
        await new Promise((resolve) => setTimeout(resolve, 20));
      }
    }
  }
  async function pinNode(token) {
    const known = tokens.get(token);
    if (!known) fail('stale');
    const locator = page.locator('aria-ref=' + known.ref);
    if ((await locator.count()) !== 1) fail('stale');
    const resolved = await locator.elementHandle({ timeout: timeout() });
    if (!resolved) fail('stale');
    try {
      if (
        (await known.handle.ownerFrame()) !== page.mainFrame() ||
        !(await resolved.evaluate(
          (node, original) => node === original && node.isConnected,
          known.handle,
        ))
      )
        fail('stale');
    } finally {
      await resolved.dispose();
    }
    return known.handle;
  }
  async function navigationFor(handle) {
    const destination = await handle.evaluate((element) => {
      if (element.getAttribute('href') !== null)
        return { value: element.getAttribute('href'), method: 'GET' };
      const tag = element.tagName;
      const type = (
        element.getAttribute('type') ?? (tag === 'BUTTON' ? 'submit' : 'text')
      ).toLowerCase();
      const submits =
        (tag === 'BUTTON' && type === 'submit') ||
        (tag === 'INPUT' && ['submit', 'image'].includes(type));
      if (element.form && submits)
        return {
          value:
            element.getAttribute('formaction') ??
            element.form.getAttribute('action') ??
            '',
          method: (
            element.getAttribute('formmethod') ??
            element.form.getAttribute('method') ??
            'GET'
          ).toUpperCase(),
        };
      return { value: '', method: 'GET' };
    });
    return allowedRoute(destination.value, destination.method);
  }
  async function execute(message) {
    let dispatched = false;
    try {
      timeout();
      const operation = validateExecute(bootstrap, message, {
        contextId,
        document: latest?.document,
        nodeTokens: new Set(tokens.keys()),
      });
      if (operation.kind === 'ownerControl') {
        if (ownerControl === operation.enabled) fail('stale');
        ownerControl = operation.enabled;
        await disposeTokens();
        latest = undefined;
        network.effectActive = ownerControl;
        if (ownerControl) await page.bringToFront();
        actionsUsed++;
        if (actionsUsed > scope.limits.actions) fail('limitsExceeded');
        const observation = {
          documentGeneration: ++serial,
          routeId: scope.routes[0].id,
          nodes: [],
          shortened: false,
          omittedNodes: 0,
          artifacts: [],
        };
        return {
          contextId,
          pageId,
          document: {
            documentId: 'control-' + serial,
            documentGeneration: serial,
            observationDigest: canonicalDigest(observation),
          },
          observation,
          counters: counters(),
        };
      }
      if (ownerControl) fail('denied');
      if (['navigate', 'navigateUrl'].includes(operation.kind))
        redirectHops = 0;
      if (actionsUsed >= scope.limits.actions) fail('limitsExceeded');
      if (!['navigate', 'navigateUrl', 'observe'].includes(operation.kind)) {
        if (!latest) fail('stale');
        const current = await currentEvidence();
        if (
          current.marker.token !== latest.marker.token ||
          current.marker.revision !== latest.marker.revision ||
          current.fingerprint !== latest.fingerprint
        )
          fail('stale');
      }
      const handle = operation.node
        ? await pinNode(operation.node.token)
        : undefined;
      let uploadBytes, selectedRoute;
      if (operation.kind === 'upload') {
        const input = bootstrap.inputs.find(
          (candidate) => candidate.id === operation.inputId,
        );
        if (!input) fail('denied');
        await realContained(input.filePath, bootstrap.privateRoot);
        uploadBytes = await checkedFile(
          input.filePath,
          scope.limits.inputBytes,
          input.sha256,
          input.bytes,
        );
      }
      if (operation.kind === 'navigate')
        selectedRoute = scope.routes.find(
          (route) => route.id === operation.routeId && route.method === 'GET',
        );
      if (operation.kind === 'navigateUrl')
        selectedRoute = allowedRoute(operation.url);
      if (operation.kind === 'click')
        selectedRoute = await navigationFor(handle);
      if (operation.kind === 'click' && selectedRoute) redirectHops = 0;
      if (operation.kind === 'download') {
        const href = await handle.evaluate((element) =>
          element.tagName === 'A' ? element.getAttribute('href') : null,
        );
        selectedRoute = allowedRoute(href);
        if (
          !selectedRoute ||
          !bootstrap.outputs.some(
            (slot) => slot.kind === 'csv' && !usedOutputs.has(slot.id),
          )
        )
          fail('denied');
      }
      timeout();
      actionsUsed++;
      network.effectActive = !['observe', 'download'].includes(operation.kind);
      const navigates =
        operation.kind === 'navigate' ||
        operation.kind === 'navigateUrl' ||
        operation.kind === 'click';
      network.navigationRouteId = navigates ? selectedRoute?.id : undefined;
      network.navigationRemaining = navigates && selectedRoute ? 1 : 0;
      if (scope.interactive && navigates && selectedRoute)
        admittedNavigationAuthority =
          new URL((selectedRoute.origin ?? scope.origin) + selectedRoute.path)
            .hostname + ':443';
      network.downloadRouteId =
        operation.kind === 'download' ? selectedRoute?.id : undefined;
      const artifacts = [];
      switch (operation.kind) {
        case 'navigate':
        case 'navigateUrl':
          dispatched = true;
          await page.goto(
            (selectedRoute.origin ?? scope.origin) + selectedRoute.path,
            {
              waitUntil: 'load',
              timeout: timeout(),
            },
          );
          if (pendingRedirect) {
            const destination = pendingRedirect;
            pendingRedirect = undefined;
            await page.waitForURL(destination, {
              waitUntil: 'load',
              timeout: timeout(),
            });
          }
          break;
        case 'observe':
          break;
        case 'click': {
          const navigation =
            selectedRoute && !scope.interactive
              ? page.waitForURL(scope.origin + selectedRoute.path, {
                  waitUntil: 'load',
                  timeout: timeout(),
                })
              : undefined;
          navigation?.catch(() => {});
          dispatched = true;
          await handle.click({ timeout: timeout() });
          if (navigation) await navigation;
          break;
        }
        case 'type':
          dispatched = true;
          await handle.fill(operation.text, { timeout: timeout() });
          break;
        case 'selectOption':
          dispatched = true;
          await handle.selectOption(operation.value, { timeout: timeout() });
          break;
        case 'pressKey':
          dispatched = true;
          await handle.press(operation.key, { timeout: timeout() });
          break;
        case 'scroll':
          dispatched = true;
          await page.mouse.wheel(0, operation.deltaY);
          break;
        case 'upload':
          dispatched = true;
          await handle.setInputFiles(
            {
              name: 'selected-input.csv',
              mimeType: 'text/csv',
              buffer: uploadBytes,
            },
            { timeout: timeout() },
          );
          break;
        case 'download': {
          dispatched = true;
          const bounded = await guardedResponse({
            pageId,
            isMainFrame: true,
            channel: 'http',
            method: 'GET',
            url: (selectedRoute.origin ?? scope.origin) + selectedRoute.path,
            navigation: false,
            download: true,
            bodyBytes: 0,
            headerBytes: 0,
          });
          artifacts.push(
            await writeArtifact(bounded.kind ?? 'csv', bounded.body),
          );
          break;
        }
      }
      timeout();
      if (operation.kind === 'observe' && operation.screenshot) {
        const image = await page.screenshot({
          type: 'jpeg',
          quality: 45,
          fullPage: false,
          animations: 'disabled',
          caret: 'hide',
          scale: 'css',
          timeout: timeout(),
        });
        artifacts.push(await writeArtifact('screenshot', image));
      }
      return await observe(artifacts);
    } catch (error) {
      const failure = staticFailure(
        fatal ??
          (shutdown.signal.aborted
            ? new BrowserWorkerError('cancelled')
            : error),
        dispatched,
      );
      if (
        !closing &&
        (dispatched ||
          shutdown.signal.aborted ||
          ['protocol', 'unavailable', 'deadline', 'limitsExceeded'].includes(
            failure.code,
          ))
      )
        retire(failure.code);
      throw failure;
    } finally {
      admittedNavigationAuthority = undefined;
      ownerNavigationActive = false;
      network.effectActive =
        bootstrap.profile?.ownerLogin === true || ownerControl;
      network.navigationRemaining = 0;
      network.navigationRouteId = undefined;
      network.downloadRouteId = undefined;
      expectedNavigation = undefined;
    }
  }
  try {
    timeout();
    const downloadDirectory = windowsPath.join(
      bootstrap.privateRoot,
      'browser-downloads',
    );
    const prepareRuntime =
      dependencies.prepareRuntime ??
      (async () => {
        await checkedFile(
          bootstrap.installation.browserExecutable,
          268435456,
          bootstrap.installation.browserSha256,
        );
        await filesystem.mkdir(downloadDirectory, {
          recursive: false,
          mode: 0o700,
        });
        await realContained(downloadDirectory, bootstrap.privateRoot);
        if (scope.interactive || bootstrap.profile) {
          const profileDirectory = windowsPath.join(
            bootstrap.privateRoot,
            'profile-context',
            'Default',
          );
          await filesystem.mkdir(profileDirectory, {
            recursive: true,
            mode: 0o700,
          });
          await realContained(profileDirectory, bootstrap.privateRoot);
          await filesystem.writeFile(
            windowsPath.join(profileDirectory, 'Preferences'),
            JSON.stringify({
              net: { network_prediction_options: 2 },
              dns_prefetching: { enabled: false },
              default_search_provider: { enabled: false },
              search: { suggest_enabled: false },
              autofill: { profile_enabled: false, credit_card_enabled: false },
              credentials_enable_service: false,
            }),
            { flag: 'wx', mode: 0o600 },
          );
        }
      });
    await prepareRuntime();
    timeout();
    denyProxy = await (dependencies.createDenyProxy ?? createDenyProxy)({
      signal: shutdown.signal,
      onRequest(code = 'denied', metadata) {
        network.requestsUsed++;
        network.blockedRequests++;
        const failure =
          network.requestsUsed > scope.limits.requests
            ? 'limitsExceeded'
            : code;
        // Native Chromium 1248 emits a speculative CONNECT for page.goto before
        // its paused request reaches interception. The deny proxy STILL returns
        // 403 and never forwards any socket. Only that exact approved hostname
        // during an already admitted navigation may continue; all refusals stay
        // charged, and every other fallback retains strict retirement.
        const expectedSpeculation =
          scope.interactive &&
          admittedNavigationAuthority &&
          !fatal &&
          !shutdown.signal.aborted &&
          now() < expiresAtMs &&
          (!ownerNavigationActive ||
            (now() < ownerNavigationExpires && ownerNavigationUses > 0)) &&
          metadata?.method === 'CONNECT' &&
          metadata.authority === admittedNavigationAuthority;
        if (expectedSpeculation && ownerNavigationActive) ownerNavigationUses--;
        if ((actionsUsed > 0 && !expectedSpeculation) || failure !== 'denied')
          retire(failure);
      },
      onFailure() {
        retire('unavailable');
      },
    });
    timeout();
    const launchOptions = {
      executablePath: bootstrap.installation.browserExecutable,
      chromiumSandbox: true,
      headless:
        bootstrap.profile?.ownerLogin || scope.interactive
          ? dependencies.profileHeadlessForTests === true
          : true,
      env: environment,
      downloadsPath: downloadDirectory,
      proxy: { server: denyProxy.server },
      args: [
        '--disable-quic',
        '--force-webrtc-ip-handling-policy=disable_non_proxied_udp',
        '--proxy-bypass-list=<-loopback>',
        '--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1',
        '--disable-background-networking',
        '--disable-preconnect',
        '--dns-prefetch-disable',
      ],
      timeout: Math.min(10000, remaining),
    };
    const contextOptions = {
      serviceWorkers: 'block',
      acceptDownloads: false,
      viewport: { width: 1024, height: 768 },
      permissions: [],
      ignoreHTTPSErrors: false,
      bypassCSP: false,
      ...(scope.publicIpv4Pin && !scope.authenticated && !scope.interactive
        ? { javaScriptEnabled: false }
        : {}),
    };
    if (bootstrap.profile || scope.interactive) {
      context = await chromium.launchPersistentContext(
        windowsPath.join(bootstrap.privateRoot, 'profile-context'),
        { ...launchOptions, ...contextOptions },
      );
      for (const startupPage of context.pages()) await startupPage.close();
      if (bootstrap.profile) {
        try {
          const sealed = JSON.parse(
            await filesystem.readFile(bootstrap.profile.sealedPath, 'utf8'),
          );
          await context.setStorageState(
            openProfileState(bootstrap.profile, sealed),
          );
        } catch (error) {
          if (!(bootstrap.profile.ownerLogin && error.code === 'ENOENT'))
            throw error;
        }
        profileLoaded = true;
      }
    } else {
      browser = await chromium.launch(launchOptions);
      timeout();
      context = await browser.newContext(contextOptions);
    }
    timeout();
    if (scope.interactive && !dependencies.prepareRuntime) {
      const launched = browser ?? context.browser();
      if (!launched || launched.version() !== MAINTAINED_CHROMIUM_VERSION)
        fail('unavailable');
    }
    context.setDefaultTimeout(10000);
    await context.exposeBinding(report, () => retire('denied'));
    if (scope.interactive)
      await context.exposeBinding(ownerNavigation, (source, origin) => {
        if (
          !ownerControl ||
          closing ||
          fatal ||
          shutdown.signal.aborted ||
          now() >= expiresAtMs ||
          source.page !== page ||
          source.frame !== page.mainFrame()
        )
          return false;
        if (
          typeof origin !== 'string' ||
          !scope.interactive.origins.some(
            (approved) => approved.origin === origin,
          )
        )
          return false;
        try {
          publicHostname(origin);
        } catch {
          return false;
        }
        admittedNavigationAuthority = new URL(origin).hostname + ':443';
        ownerNavigationActive = true;
        // This page-supplied origin is an untrusted hint under explicit owner
        // control, not proof of human identity. It can suppress at most three
        // already-denied CONNECT refusals over two seconds; it grants no egress.
        ownerNavigationExpires = Math.min(expiresAtMs, now() + 2000);
        ownerNavigationUses = 3;
        redirectHops = 0;
        return true;
      });
    await context.addInitScript(installDocumentGuards, {
      marker,
      report,
      ...(scope.interactive ? { ownerNavigation } : {}),
    });
    await context.route('**/*', (route) => {
      const work = routeHttp(route);
      routeTasks.add(work);
      work.finally(() => routeTasks.delete(work)).catch(() => {});
      return work;
    });
    await context.routeWebSocket('**/*', async (socket) => {
      network.requestsUsed++;
      network.blockedRequests++;
      try {
        await socket.close({ code: 1008, reason: 'Unavailable' });
      } catch {}
      retire(
        network.requestsUsed > scope.limits.requests
          ? 'limitsExceeded'
          : 'denied',
      );
    });
    context.on('serviceworker', () => retire('denied'));
    context.on('page', (candidate) => {
      if (!expectPage || page) {
        retire('denied');
        void candidate.close().catch(() => {});
        return;
      }
      page = candidate;
      expectPage = false;
      attachPage(candidate);
    });
    expectPage = true;
    const created = await context.newPage();
    if (page !== created || expectPage) fail('unavailable');
    timeout();
    await documentMarker();
    if (bootstrap.profile?.ownerLogin) {
      const login = scope.routes.find(
        (route) =>
          route.id === bootstrap.profile.loginRouteId && route.method === 'GET',
      );
      if (!login) fail('denied');
      network.effectActive = true;
      network.navigationRouteId = login.id;
      network.navigationRemaining = 1;
      expectedNavigation = scope.origin + login.path;
      await page.goto(expectedNavigation, {
        waitUntil: 'domcontentloaded',
        timeout: timeout(),
      });
    }
    finishStartup();
    return {
      contextId,
      pageId,
      counters,
      execute,
      cancel() {
        shutdown.abort();
        void close().catch(() => {});
      },
      async close() {
        clearTimeout(leaseTimer);
        return close();
      },
      diagnostics() {
        return freeze({
          ...privateDiagnostics,
          blockedRequests:
            network.blockedRequests + privateDiagnostics.blockedRequests,
        });
      },
    };
  } catch (error) {
    clearTimeout(leaseTimer);
    finishStartup();
    await close().catch(() => {});
    throw staticFailure(error);
  }
}

/** Protocol pump: no engine creation until a complete admitted Open. */
export async function runProtocol(bootstrap, input, output, createSession) {
  const state = { wireBytes: 0, lastIncomingId: 0, lastOutgoingId: 0 };
  const reader = new FrameReader(state);
  const cancellation = new AbortController();
  let session,
    active = Promise.resolve(),
    terminal,
    publicationStopped = false;
  let closeRequested = false;
  const writes = new Set();
  async function send(message) {
    if (publicationStopped) return;
    const encoded = encodeFrame(message, state);
    const writing = new Promise((resolve, reject) =>
      output.write(encoded, (error) => (error ? reject(error) : resolve())),
    );
    writes.add(writing);
    try {
      await writing;
    } finally {
      writes.delete(writing);
    }
  }
  function stop(error) {
    terminal ??= staticFailure(error);
    publicationStopped = true;
    cancellation.abort();
    session?.cancel();
    input.destroy();
  }
  async function answer(message, callback) {
    try {
      const result = await callback();
      if (closeRequested && message.op !== 'close')
        throw new BrowserWorkerError(
          'cancelled',
          message.op === 'execute' ? 'mayHaveOccurred' : 'notDispatched',
        );
      await send({
        protocol: 1,
        id: message.id,
        op: message.op,
        payload: { ok: true, result },
      });
    } catch (error) {
      const failure = staticFailure(error, error?.effect === 'mayHaveOccurred');
      try {
        await send({
          protocol: 1,
          id: message.id,
          op: message.op,
          payload: {
            ok: false,
            error: { code: failure.code, effect: failure.effect },
            counters: session?.counters() ?? {
              actionsUsed: 0,
              requestsUsed: 0,
              wireBytesUsed: 0,
            },
          },
        });
      } catch (encodingError) {
        stop(encodingError);
      }
    }
  }
  function handle(message) {
    if (message.op === 'open') {
      active = answer(message, async () => {
        validateOpen(bootstrap, message);
        session = await createSession(
          (failure) => stop(failure),
          cancellation.signal,
          message.payload.profileKey,
        );
        if (publicationStopped || closeRequested) {
          await session.close();
          fail('cancelled');
        }
        return {
          contextId: session.contextId,
          pageId: session.pageId,
          counters: session.counters(),
        };
      });
    } else if (message.op === 'execute') {
      active = answer(message, () => session.execute(message));
    } else {
      closeRequested = true;
      session?.cancel();
      const previous = active;
      active = (async () => {
        await previous;
        await answer(message, () => session.close());
      })();
    }
    active.catch((error) => stop(error));
  }
  output.on('error', (error) => stop(error));
  const ended = new Promise((resolve) => {
    input.on('data', (chunk) => {
      if (publicationStopped) return;
      try {
        for (const message of reader.push(chunk)) handle(message);
      } catch (error) {
        stop(error);
      }
    });
    input.on('end', () => {
      try {
        reader.finish();
      } catch (error) {
        terminal ??= staticFailure(error);
      }
      if (stateMetadata(state).pending.size)
        terminal ??= new BrowserWorkerError('cancelled');
      publicationStopped = true;
      cancellation.abort();
      session?.cancel();
      resolve();
    });
    input.on('error', (error) => {
      terminal ??= staticFailure(error);
      publicationStopped = true;
      cancellation.abort();
      session?.cancel();
      resolve();
    });
    input.on('close', () => {
      publicationStopped = true;
      cancellation.abort();
      session?.cancel();
      resolve();
    });
  });
  await ended;
  await active;
  if (session) {
    try {
      await session.close();
    } catch (error) {
      terminal ??= staticFailure(error);
    }
  }
  await Promise.allSettled([...writes]);
  return terminal ? 1 : 0;
}

export async function runCli(options = {}) {
  const argv = options.argv ?? process.argv.slice(2);
  const cwd = options.cwd ?? process.cwd();
  const environment = options.environment ?? process.env;
  const input = options.input ?? process.stdin;
  const output = options.output ?? process.stdout;
  try {
    if (
      process.platform !== 'win32' ||
      ![22, 24].includes(Number(process.versions.node.split('.')[0]))
    )
      fail('unavailable');
    const bootstrapPath = parseCliArguments(argv, cwd);
    const browserEnvironment = validateWorkerEnvironment(environment, cwd);
    const bootstrap = await loadBootstrap(bootstrapPath, cwd);
    const runtime = async (onFatal, signal, profileKey) => {
      const { chromium } = await import('playwright-core');
      const { yaml } = require('playwright-core/lib/utilsBundle');
      const authenticatedBootstrap = bootstrap.profile
        ? {
            ...bootstrap,
            profile: { ...bootstrap.profile, keyHex: profileKey },
          }
        : bootstrap;
      return createMaintainedSession(authenticatedBootstrap, {
        chromium,
        yaml,
        environment: browserEnvironment,
        onFatal,
        signal,
      });
    };
    return await runProtocol(bootstrap, input, output, runtime);
  } catch {
    return 1;
  }
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : '';
const modulePath = fileURLToPath(import.meta.url);
if (
  invokedPath &&
  (process.platform === 'win32'
    ? invokedPath.toLowerCase() === modulePath.toLowerCase()
    : invokedPath === modulePath)
) {
  process.exitCode = await runCli();
}
