#!/usr/bin/env node
/**
 * Real-provider acceptance, deliberately split into explicit phases.
 * No phase repairs learned skills, copies owner state, prints credentials, or
 * stops an unowned process. `prepare` copies a binary, not an owner profile.
 *
 * node tools/skill-learning-acceptance.mjs prepare --binary <built daemon>
 * node tools/skill-learning-acceptance.mjs start
 * node tools/skill-learning-acceptance.mjs setup
 * node tools/skill-learning-acceptance.mjs baseline
 * node tools/skill-learning-acceptance.mjs correction
 * node tools/skill-learning-acceptance.mjs review
 * node tools/skill-learning-acceptance.mjs fresh
 * node tools/skill-learning-acceptance.mjs checkpoint
 * node tools/skill-learning-acceptance.mjs stop
 * node tools/skill-learning-acceptance.mjs restart
 * node tools/skill-learning-acceptance.mjs reload
 * node tools/skill-learning-acceptance.mjs undo
 * node tools/skill-learning-acceptance.mjs stop
 * node tools/skill-learning-acceptance.mjs proof
 *
 * `inspect` is read-only. `wait baseline|correction|fresh` resumes bounded
 * observation of an already submitted run. A submit phase never retries an
 * uncertain admission. `self-test` uses synthetic bytes and no daemon/model.
 */
import assert from 'node:assert/strict';
import { createHash, randomBytes } from 'node:crypto';
import { spawn, execFile } from 'node:child_process';
import fs from 'node:fs/promises';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

const exec = promisify(execFile);
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const phase = args[0] ?? 'help';
function option(name, fallback) {
  const i = args.indexOf(`--${name}`);
  return i < 0 ? fallback : args[i + 1];
}
const root = path.resolve(repo, option('root', '.tmp/skill-learning-live-2026-10-21'));
const rootBase = path.resolve(repo, '.tmp');
const fixtureRoot = path.join(repo, 'packages/core-rust/acceptance/skill-learning');
const port = 18089;
const origin = `http://127.0.0.1:${port}`;
const slug = 'ledger-export';
const allowedTools = ['load_skill', 'read_file', 'write_file'];
const stateFile = path.join(root, 'case.json');
const tokenFile = path.join(root, 'private-admin-token');
const canonicalAgent = {
  name: 'Skill learning acceptance',
  provider: 'moonshot',
  model: 'kimi-k2-0905-preview',
  system: 'Use the applicable owner-approved skill for a filesystem task. Read it with load_skill before relying on its procedure. Work only on the requested task and output file. Use only the tools granted to you. Report completion only after reading the output back. Do not write skills or memory files.',
  tools: allowedTools,
  settings: {
    temperature: 0,
    maxTokens: 2200,
    timeoutMs: 120000,
    maxRetries: 0,
    maxToolIterations: 6,
    agentFilesProfileEnabled: false,
  },
  fallbacks: [],
};
let state;
let secrets = [];
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
function guardRoot() {
  const relative = path.relative(rootBase, root);
  assert(relative && !relative.startsWith('..') && !path.isAbsolute(relative), 'Acceptance root must be a dedicated child of repo/.tmp');
}
function clean(value) {
  let text = typeof value === 'string' ? value : JSON.stringify(value, null, 2);
  for (const secret of secrets) if (secret) text = text.split(secret).join('[REDACTED]');
  return text;
}
function report(value) { console.log(clean(value)); }
async function save(name, value) {
  await fs.writeFile(path.join(root, name), clean(value) + '\n', { encoding: 'utf8', mode: 0o600 });
}
async function read(name) { return JSON.parse(await fs.readFile(path.join(root, name), 'utf8')); }
async function saveState() { await save('case.json', state); }
async function exists(file) { try { await fs.access(file); return true; } catch { return false; } }
async function loadEnv() {
  // Only the selected provider credential is forwarded; no env file is copied.
  const text = await fs.readFile(path.join(repo, '.env'), 'utf8');
  const values = {};
  for (const line of text.split(/\r?\n/)) {
    const m = line.match(/^\s*(?:export\s+)?([A-Z][A-Z0-9_]*)\s*=\s*(.*?)\s*$/);
    if (!m) continue;
    let value = m[2];
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) value = value.slice(1, -1);
    else value = value.replace(/\s+#.*$/, '').trim();
    values[m[1]] = value;
    if (/KEY|TOKEN|SECRET|PASSWORD/.test(m[1]) && value) secrets.push(value);
  }
  const name = state?.provider === 'anthropic' ? 'ANTHROPIC_API_KEY' : 'MOONSHOT_API_KEY';
  assert(values[name], `${name} must be configured in the repository .env`);
  return { [name]: values[name] };
}
async function load() {
  guardRoot();
  state = await read('case.json');
  assert.equal(state.schema, 1);
  assert.equal(state.root, root);
  secrets.push((await fs.readFile(tokenFile, 'utf8')).trim());
  await loadEnv();
}
async function api(method, route, body, headers = {}) {
  const token = (await fs.readFile(tokenFile, 'utf8')).trim();
  const response = await fetch(origin + route, {
    method,
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json', ...headers },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(15000),
  });
  const text = await response.text();
  let result;
  try { result = JSON.parse(text); } catch { throw new Error(`${method} ${route}: non-JSON HTTP ${response.status}`); }
  assert(response.ok, clean(`${method} ${route}: HTTP ${response.status}: ${text.slice(0, 1200)}`));
  return result;
}
async function processIdentity(pid) {
  assert(Number.isSafeInteger(pid) && pid > 0);
  if (process.platform === 'win32') {
    let stdout;
    try {
      ({ stdout } = await exec('cscript.exe', ['//Nologo', path.join(fixtureRoot, 'process-identity.js'), String(pid)], { windowsHide: true, timeout: 10000 }));
    } catch (error) {
      throw new Error(`Windows owned-PID probe failed: ${(error.stdout || error.stderr || error.message).trim().slice(0, 600)}. Windows Script Host may require execution outside the filesystem sandbox.`);
    }
    if (!stdout.trim()) return null;
    const [foundPid, executable, created] = stdout.trim().split('|');
    assert(executable && created && Number(foundPid) === pid, 'Windows process identity is unavailable');
    return { pid, path: executable, created };
  }
  try {
    const target = await fs.readlink(`/proc/${pid}/exe`);
    const fields = (await fs.readFile(`/proc/${pid}/stat`, 'utf8')).replace(/^.*\) /, '').split(' ');
    return { pid, path: target, created: fields[19] };
  } catch (error) { if (error.code === 'ENOENT') return null; throw error; }
}
function comparablePath(value) {
  // Rust canonicalize returns the Windows extended-length spelling.
  if (value.startsWith('\\\\?\\UNC\\')) value = '\\\\' + value.slice(8);
  else if (value.startsWith('\\\\?\\')) value = value.slice(4);
  return path.resolve(value).toLowerCase();
}
function samePath(a, b) { return comparablePath(a) === comparablePath(b); }
async function owned() {
  const record = state.processes?.findLast(p => !p.stopped);
  assert(record, 'No live owned-process record');
  const actual = await processIdentity(record.pid);
  assert(actual && actual.created === record.created && samePath(actual.path, state.binary), 'PID identity differs; refusing to use or stop it');
  return record;
}
async function assertLive() {
  await owned();
  const workspace = await api('GET', '/api/workspace');
  if (state.setup) assert(samePath(workspace.workspace.rootPath, state.workspace), 'Daemon workspace differs from acceptance root');
}
async function portFree() {
  await new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once('error', () => reject(new Error(`Port ${port} is occupied; no process was stopped`)));
    server.listen(port, '127.0.0.1', () => server.close(resolve));
  });
}
function parseFixture(csv) {
  const lines = csv.trimEnd().split(/\r?\n/);
  assert.equal(lines.shift(), 'id,kind,amount_cents');
  const ids = new Set();
  let sales = 0, refunds = 0;
  for (const line of lines) {
    const [id, kind, amount, extra] = line.split(',');
    assert(id && !ids.has(id) && ['sale', 'refund'].includes(kind) && /^\d+$/.test(amount) && extra === undefined, 'Invalid acceptance fixture');
    ids.add(id);
    const cents = Number(amount);
    assert(Number.isSafeInteger(cents) && cents > 0);
    if (kind === 'sale') sales += cents; else refunds += cents;
  }
  return { row_count: lines.length, gross_sales_cents: sales, refund_cents: refunds, net_sales_cents: sales - refunds };
}
function expectedBytes(fixture, incorrect = false) {
  const value = parseFixture(fixture);
  if (incorrect) value.net_sales_cents = value.gross_sales_cents + value.refund_cents;
  return Buffer.from(JSON.stringify(value), 'utf8');
}
async function oracle(fixture, output, incorrect = false) {
  const input = await fs.readFile(path.join(state.workspace, fixture), 'utf8');
  const actual = await fs.readFile(path.join(state.workspace, output));
  const expected = expectedBytes(input, incorrect);
  assert(actual.equals(expected), `Exact output bytes differ for ${output}; expected ${expected.toString()}, got ${actual.toString().slice(0, 500)}`);
  return { output, sha256: sha(actual), bytes: actual.length, value: JSON.parse(actual.toString()), expectedMatches: true, deliberatelyIncorrectBaseline: incorrect };
}
async function strict(agentId) {
  await api('PUT', `/api/agents/${agentId}/autonomy`, { mode: 'strict', smartCheck: false });
  const policy = await api('PATCH', `/api/agents/${agentId}/learning-policy`, { autoEnabled: false });
  assert.equal(policy.autoEnabled, false);
}
async function setup() {
  await assertLive();
  assert(!state.setup, 'Already set up');
  const agents = (await api('GET', '/api/agents')).agents;
  assert.equal(agents.length, 0, 'Isolated daemon must begin without agents');
  await api('PUT', '/api/workspace', { rootPath: state.workspace, companyName: 'Skill learning acceptance', mission: 'Isolated synthetic reconciliation acceptance', values: [] });
  const config = { ...canonicalAgent, provider: state.provider, model: state.model };
  const result = await api('POST', '/api/agents', config);
  state.agentId = result.agent.state.id;
  assert(state.agentId);
  const actual = result.agent.state.config;
  assert.deepEqual(actual.tools.map(tool => tool.name), allowedTools, 'Canonical tool grants differ');
  assert.equal(actual.provider, state.provider);
  assert.equal(actual.model, state.model);
  await strict(state.agentId);
  const body = await fs.readFile(path.join(fixtureRoot, 'SKILL.baseline.md'), 'utf8');
  const saved = await api('PUT', `/api/skills/${slug}`, {
    name: 'Ledger reconciliation export',
    description: 'Prepare a reconciliation export JSON from a ledger CSV fixture containing sale and refund rows with integer cent amounts.',
    body,
    enabled: true,
    files: [{ path: 'references/export-format.txt', content: 'Compact JSON; keys row_count, gross_sales_cents, refund_cents, net_sales_cents; integer cents; no newline.\n' }],
  });
  state.originalHash = saved.skill.approvedHash;
  state.config = config;
  state.actualConfig = actual;
  state.setup = true;
  await save('baseline-skill.json', await api('GET', `/api/skills/${slug}`));
  await saveState();
  report({ setup: true, agentId: state.agentId, provider: state.provider, model: state.model, originalHash: state.originalHash });
}
async function emptyFresh(agentId) {
  const memories = await api('GET', `/api/agents/${agentId}/memories/recent?limit=100`);
  const core = await api('GET', `/api/agents/${agentId}/core-memory`);
  assert.equal(memories.memories.length, 0, 'Fresh agent has canonical memory');
  assert.equal(core.user.entries.length + core.notes.entries.length, 0, 'Fresh agent has core memory');
  return { canonicalMemories: 0, coreEntries: 0 };
}
async function submit(which) {
  await assertLive();
  assert(state.setup);
  assert(!state[which], `${which} already attempted; inspect or wait, never resubmit`);
  let agentId = state.agentId;
  let sessionId;
  let text;
  let skill;
  if (which === 'baseline') {
    skill = slug; // Explicit context plus the model's native load observation.
    text = "Prepare the reconciliation export for 'fixtures/baseline.csv'.";
  } else if (which === 'correction') {
    assert(state.baseline?.verified, 'Baseline must first prove the declared incorrect total');
    sessionId = state.baseline.sessionId;
    await api('PUT', `/api/agents/${agentId}/autonomy`, { mode: 'free', smartCheck: false });
    await api('PATCH', `/api/agents/${agentId}/learning-policy`, { autoEnabled: true });
    text = "The previous reconciliation export is incorrect. Refunds in this CSV are positive integer cents and must be subtracted, never added to sales. Correct the reconciliation export for 'fixtures/baseline.csv': read the applicable skill and input, sum sale rows into gross_sales_cents, sum refund rows into refund_cents, calculate net_sales_cents = gross_sales_cents - refund_cents, write the same compact export schema to outputs/baseline.json, then read it back and compare every value. Only change outputs/baseline.json; do not edit skill or memory files. Retain this correction as a reusable procedure through the runtime's normal background learning review.";
  } else {
    assert(state.learnedHash && state.review?.verified && !state.undo, 'A verified automatic revision is required before the fresh task');
    const config = { ...state.config, name: state.config.name + ' fresh' };
    const result = await api('POST', '/api/agents', config);
    agentId = result.agent.state.id;
    const actual = result.agent.state.config;
    const primary = { ...state.actualConfig, name: actual.name };
    assert.deepEqual(actual, primary, 'Fresh agent primary config or tool grants differ');
    state.freshAgentId = agentId;
    await strict(agentId);
    const empty = await emptyFresh(agentId);
    await save('fresh-isolation.json', { agentId, ...empty, config: actual, equivalentPrimaryConfig: true, noMemoryTools: true, noSkillInUserPrompt: true });
    text = "Prepare the reconciliation export for 'fixtures/fresh.csv'.";
    assert(!text.includes(slug) && !/subtract|net_sales|refund/.test(text));
  }
  if (!sessionId) sessionId = (await api('POST', `/api/agents/${agentId}/sessions`, { title: `Skill learning ${which}` })).session.id;
  const messages = await api('GET', `/api/agents/${agentId}/sessions/${encodeURIComponent(sessionId)}/messages`);
  if (which !== 'correction') assert.equal(messages.messages.length, 0, 'New session is not empty');
  state[which] = { attempted: true, agentId, sessionId, text, skill: skill ?? null };
  await saveState(); // Persist attempt before any uncertain admission.
  const result = await api('POST', `/api/agents/${agentId}/sessions/${encodeURIComponent(sessionId)}/runs`, { mode: 'queue', text, ...(skill ? { skill } : {}) }, { 'Idempotency-Key': `skill-learning-${which}-${sessionId}` });
  state[which].runId = result.run.id;
  assert(state[which].runId);
  await saveState();
  report({ submitted: which, agentId, sessionId, runId: result.run.id });
  await waitRun(which);
}
async function approveDeclaredWrite(trial, output) {
  const approvals = (await api('GET', `/api/approvals?agentId=${encodeURIComponent(trial.agentId)}`)).approvals;
  for (const approval of approvals) {
    assert.equal(approval.runId, trial.runId, 'Unrelated approval found in isolated agent');
    assert.equal(approval.tool, 'write_file', 'Only the declared output write is preauthorized');
    assert(!approval.argumentsTruncated);
    const value = JSON.parse(approval.arguments);
    assert.equal(typeof value.file_path, 'string');
    const target = path.resolve(state.workspace, value.file_path);
    assert(samePath(target, path.join(state.workspace, output)), 'Approval targets a file outside the declared output');
    await api('POST', `/api/approvals/${approval.id}/decision`, { decision: 'allow_once', revision: approval.revision, note: 'Authorized isolated acceptance output only' });
    await save(`approval-${approval.id}.json`, { id: approval.id, runId: trial.runId, tool: approval.tool, target, decision: 'allow_once' });
  }
}
async function waitRun(which) {
  const trial = state[which];
  assert(trial?.runId, 'Admission outcome unknown: inspect that exact session; do not submit again');
  const output = which === 'fresh' ? 'outputs/fresh.json' : 'outputs/baseline.json';
  const deadline = Date.now() + 55000;
  do {
    const run = (await api('GET', `/api/agents/${trial.agentId}/runs/${trial.runId}`)).run;
    if (run.status === 'awaiting_approval') await approveDeclaredWrite(trial, output);
    if (['completed', 'failed', 'cancelled', 'interrupted'].includes(run.status)) return verifyRun(which, run);
    await sleep(1000);
  } while (Date.now() < deadline);
  report({ pending: which, runId: trial.runId, next: `node tools/skill-learning-acceptance.mjs wait ${which}`, observationSeconds: 55 });
}
async function skillLearning() { return api('GET', `/api/skills/${slug}/learning`); }
function experiences(report) {
  assert(Array.isArray(report.experiences), 'Learning endpoint must return experiences');
  return report.experiences;
}
async function verifyRun(which, run) {
  const trial = state[which];
  const messages = await api('GET', `/api/agents/${trial.agentId}/sessions/${encodeURIComponent(trial.sessionId)}/messages`);
  await save(`${which}-capture.json`, { run, messages });
  assert.equal(run.status, 'completed', `${which} run did not complete; inspect capture, do not resubmit`);
  assert(!run.error && !run.stop);
  const receipts = run.actionReceipts;
  assert(receipts.length && receipts.every(r => allowedTools.includes(r.name) && r.status === 'succeeded' && r.durability === 'persistent'), 'Unexpected, unsuccessful, or non-durable foreground tool');
  assert(receipts.some(r => r.name === 'load_skill') && receipts.some(r => r.name === 'read_file') && receipts.some(r => r.name === 'write_file'), 'Task must natively load, read, write and read back');
  const tools = messages.messages.filter(m => m.role === 'tool' && m.metadata?.runId === run.id);
  assert.equal(tools.length, receipts.length, 'Tool messages and receipts differ');
  assert.deepEqual(tools.map(m => m.metadata.toolCallId), receipts.map(r => r.toolCallId), 'Tool messages do not match receipt identities');
  const detail = await api('GET', `/api/skills/${slug}`);
  // For the corrective run the automatic revision may already be active.
  const loadedBody = which === 'fresh' ? detail.file.body : (await read('baseline-skill.json')).file.body;
  assert(tools.some(m => m.text.includes(loadedBody)), 'No actual native tool result contains the expected approved skill body');
  const output = which === 'fresh' ? 'outputs/fresh.json' : 'outputs/baseline.json';
  const fixture = which === 'fresh' ? 'fixtures/fresh.csv' : 'fixtures/baseline.csv';
  const calls = receipts.map(receipt => {
    assert(!receipt.argumentsTruncated, 'Acceptance tool arguments must be whole');
    return { ...receipt, args: JSON.parse(receipt.argumentsPreview) };
  });
  const writes = calls.filter(call => call.name === 'write_file');
  assert(writes.length && writes.every(call => typeof call.args.file_path === 'string' && samePath(path.resolve(state.workspace, call.args.file_path), path.join(state.workspace, output))), 'Foreground wrote outside the declared export');
  const lastWrite = calls.findLastIndex(call => call.name === 'write_file');
  const readback = calls.findIndex((call, index) => index > lastWrite && call.name === 'read_file' && typeof call.args.file_path === 'string' && samePath(path.resolve(state.workspace, call.args.file_path), path.join(state.workspace, output)));
  assert(readback > lastWrite, 'No independent output read after the final write');
  const result = await oracle(fixture, output, which === 'baseline');
  assert(tools[readback].text.includes(JSON.stringify(result.value)), 'Native readback did not return exact export JSON');
  const version = which === 'fresh' ? state.learnedHash : state.originalHash;
  const learning = await skillLearning();
  const observation = experiences(learning).find(e => e.agentId === trial.agentId && e.runId === trial.runId && e.sessionId === trial.sessionId && e.version === version);
  assert(observation, 'Missing exact loaded package hash and run/session lineage');
  assert.equal(observation.status, 'completed');
  assert.equal(observation.verifiedEvidence ?? null, null, 'Completion must not independently attest success');
  if (which === 'fresh') {
    assert.equal(run.input.skill, null, 'Fresh run supplied explicit skill context');
    const users = messages.messages.filter(m => m.role === 'user');
    assert.equal(users.length, 1);
    assert.equal(users[0].text, trial.text);
    const freshReplies = messages.messages.filter(m => m.role === 'assistant' && m.id === run.replyMessageId);
    assert.equal(freshReplies.length, 1);
    const afterMemory = await api('GET', `/api/agents/${trial.agentId}/memories/recent?limit=100`);
    const afterCore = await api('GET', `/api/agents/${trial.agentId}/core-memory`);
    assert.equal(afterCore.user.entries.length + afterCore.notes.entries.length, 0, 'Fresh task acquired core-memory instructions');
    assert(afterMemory.memories.every(memory => memory.agentId === trial.agentId
      && memory.type === 'task_result'
      && memory.tags?.includes('runtime')
      && memory.tags?.includes('task-result')
      && memory.createdAt >= run.finishedAtMs
      && memory.content === freshReplies[0].text), 'Fresh agent acquired memory other than its own post-completion output journal');
    await save('fresh-isolation-after.json', {
      preRunCanonicalMemories: 0, coreEntries: 0, alternateLearningInjected: false,
      postCompletionOutputJournals: afterMemory.memories.map(memory => ({ id: memory.id, type: memory.type, createdAt: memory.createdAt })),
      runFinishedAtMs: run.finishedAtMs,
    });
  }
  const feedback = {
    agentId: trial.agentId, sessionId: trial.sessionId, runId: trial.runId, version,
    corrected: which === 'baseline',
    verifiedEvidence: which === 'baseline' ? null : `Independent acceptance oracle matched exact JSON bytes for ${fixture}; SHA256 ${result.sha256}.`,
  };
  await api('POST', `/api/skills/${slug}/feedback`, feedback);
  trial.verified = true;
  trial.output = result;
  trial.version = version;
  await save(`${which}-proof.json`, { ...trial, output: result, observation, receiptNames: receipts.map(r => r.name), usage: run.usage, explicitSkillContext: run.input.skill, ownerFeedback: feedback });
  await saveState();
  report({ phase: which, verified: true, ...result, loadedVersion: version });
}
async function review() {
  await assertLive();
  assert(state.correction?.verified, 'Corrective run must pass the independent oracle');
  const deadline = Date.now() + 55000;
  let reviews, source, drafts, draft, detail, ledger, entry;
  do {
    reviews = (await api('GET', `/api/agents/${state.agentId}/learning-reviews`)).reviews;
    source = reviews.find(r => r.sourceRunId === state.correction.runId);
    if (source && !['queued', 'running'].includes(source.state)) {
      if (source.state !== 'completed') break;
      // Review completion can be durable before the background skill handoff
      // finishes. Observe that same handoff; never request another review/run.
      drafts = (await api('GET', '/api/skill-drafts?status=decided')).drafts;
      draft = drafts.find(d => d.slug === slug && d.learningSource?.reviewId === source.id && d.learningSource?.sourceRunId === state.correction.runId);
      if (draft?.status === 'approved') {
        detail = await api('GET', `/api/skills/${slug}`);
        ledger = (await api('GET', '/api/skill-ledger')).entries;
        entry = ledger.find(e => e.skillSlug === slug && e.contentHash === detail.skill?.approvedHash && e.runId === state.correction.runId);
        if (detail.skill?.approvedHash !== state.originalHash && detail.skill?.approvedHash === detail.file?.hash && entry && !entry.revertedAt) break;
      }
    }
    await sleep(1000);
  } while (Date.now() < deadline);
  await save('review-capture.json', { reviews, source, drafts, draft, detail, ledger, entry });
  if (!source || ['queued', 'running'].includes(source.state)) {
    report({ reviewPending: true, next: 'node tools/skill-learning-acceptance.mjs review', observationSeconds: 55 });
    return;
  }
  assert.equal(source.state, 'completed', 'Background review failed; no manual repair or fallback proposal is allowed');
  assert.equal(source.trigger, 'automatic');
  assert(draft, 'Completed review produced no source-linked automatic skill draft within the bounded handoff wait');
  assert.equal(draft.status, 'approved');
  assert.equal(draft.decidedBy, 'autonomy:free');
  assert.equal(draft.baseHash, state.originalHash);
  assert(detail, 'Automatic skill handoff did not activate within the bounded wait');
  assert.equal(detail.skill.status, 'active');
  assert.equal(detail.skill.approvedHash, detail.file.hash);
  assert.notEqual(detail.skill.approvedHash, state.originalHash);
  const original = await read('baseline-skill.json');
  assert.deepEqual(detail.file.files, original.file.files, 'Supporting files changed during learning');
  assert(entry && !entry.revertedAt, 'Automatic revision has no durable undo ledger');
  assert.equal(entry.previousContentHash, state.originalHash);
  state.learnedHash = detail.skill.approvedHash;
  state.review = { verified: true, reviewId: source.id, draftId: draft.id, ledgerId: entry.id };
  await save('learned-skill.json', detail);
  await save('review-proof.json', { review: source, draft, entry, learnedHash: state.learnedHash, supportingFilesPreserved: true });
  await saveState();
  report({ automaticRevision: true, originalHash: state.originalHash, learnedHash: state.learnedHash, ...state.review });
}
async function quiet() {
  const status = await api('GET', '/api/status');
  assert.equal(status.runs.running, 0);
  assert.equal(status.runs.queued, 0);
  assert.equal(status.storage.history.pendingFlush, 0);
  assert.equal(status.connectors.length, 0);
  for (const id of [state.agentId, state.freshAgentId].filter(Boolean)) {
    const policy = await api('GET', `/api/agents/${id}/learning-policy`);
    assert.equal(policy.physicalReviewerActive, false);
  }
}
async function snapshot() {
  await quiet();
  return {
    skill: await api('GET', `/api/skills/${slug}`),
    learning: await skillLearning(),
    reviews: await api('GET', `/api/agents/${state.agentId}/learning-reviews`),
    ledger: await api('GET', '/api/skill-ledger'),
    drafts: await api('GET', '/api/skill-drafts?status=decided'),
    agents: await api('GET', '/api/agents'),
    memorySha256: await exists(path.join(root, 'memory.json')) ? sha(await fs.readFile(path.join(root, 'memory.json'))) : null,
  };
}
async function start(restart = false) {
  assert(!state.processes?.some(p => !p.stopped), 'An owned process is already active; inspect it first');
  if (restart) assert(state.checkpoint && !state.restarted, 'Restart requires a checkpoint and may run once');
  await portFree();
  const credential = await loadEnv();
  const token = (await fs.readFile(tokenFile, 'utf8')).trim();
  const env = {};
  for (const key of ['SystemRoot', 'WINDIR', 'PATH', 'PATHEXT', 'COMSPEC', 'NUMBER_OF_PROCESSORS', 'PROCESSOR_ARCHITECTURE']) if (process.env[key]) env[key] = process.env[key];
  Object.assign(env, credential, {
    HOME: path.join(root, 'home'), USERPROFILE: path.join(root, 'home'),
    APPDATA: path.join(root, 'home/AppData/Roaming'), LOCALAPPDATA: path.join(root, 'home/AppData/Local'),
    TEMP: path.join(root, 'tmp'), TMP: path.join(root, 'tmp'),
    ANIMAOS_RS_HOST: '127.0.0.1', ANIMAOS_RS_PORT: String(port),
    ANIMAOS_RS_PERSISTENCE_MODE: 'memory',
    ANIMAOS_RS_MEMORY_EMBEDDINGS: 'local', // Isolated acceptance needs no model download.
    ANIMAOS_RS_CONTROL_PLANE_FILE: path.join(root, 'control-plane.json'),
    ANIMAOS_RS_MEMORY_FILE: path.join(root, 'memory.json'),
    ANIMAOS_RS_HISTORY_SQLITE_FILE: path.join(root, 'history.sqlite'),
    ANIMAOS_RS_KEYCHAIN_NAMESPACE: state.keychainNamespace,
    ANIMAOS_WORKSPACE_ROOT: state.workspace,
    ANIMA_LOCAL_ADMIN_TOKEN: token,
    RUST_LOG: 'warn',
  });
  const child = spawn(state.binary, [], { cwd: state.workspace, env, detached: true, windowsHide: true, stdio: 'ignore' });
  let spawnError;
  child.once('error', error => { spawnError = error; });
  await new Promise(resolve => child.once('spawn', resolve).once('error', resolve));
  assert(!spawnError, `Daemon spawn failed: ${spawnError?.code}`);
  child.unref();
  const identity = await processIdentity(child.pid);
  assert(identity && samePath(identity.path, state.binary));
  state.processes ??= [];
  state.processes.push({ ...identity, stopped: false, binarySha256: sha(await fs.readFile(state.binary)) });
  if (restart) state.restarted = true;
  await saveState();
  const deadline = Date.now() + 20000;
  do {
    try { await api('GET', '/api/status'); report({ started: true, restarted: restart, pid: child.pid, port, isolatedRoot: root }); return; } catch { await sleep(500); }
  } while (Date.now() < deadline);
  throw new Error('Owned daemon did not become ready in 20 seconds; inspect the owned PID, never stop an owner daemon');
}
async function stop() {
  const record = await owned();
  await quiet();
  // Exact executable path + creation time checked immediately before kill.
  await owned();
  process.kill(record.pid, 'SIGTERM');
  const deadline = Date.now() + 10000;
  do { if (!await processIdentity(record.pid)) break; await sleep(200); } while (Date.now() < deadline);
  assert(!await processIdentity(record.pid), 'Owned daemon did not stop in 10 seconds');
  record.stopped = true;
  record.stoppedAt = new Date().toISOString();
  await saveState();
  await portFree();
  report({ stoppedOwnedPid: record.pid });
}
async function prepare() {
  guardRoot();
  assert(!await exists(root), 'Acceptance root already exists; use the existing case or a new --root; no overwrite or cleanup');
  const source = option('binary');
  assert(source, 'prepare requires --binary pointing to the freshly built daemon');
  const sourceBinary = path.resolve(repo, source);
  assert((await fs.stat(sourceBinary)).isFile(), 'Source daemon binary must be an existing file');
  const provider = option('provider', 'moonshot');
  assert(['moonshot', 'anthropic'].includes(provider));
  const model = provider === 'moonshot' ? 'kimi-k2-0905-preview' : 'claude-sonnet-4-6';
  const catalog = await fs.readFile(path.join(repo, 'apps/web/src/lib/types.ts'), 'utf8');
  assert(catalog.includes(model), 'Chosen model must exist in the source model catalog');
  state = { provider };
  await loadEnv();
  await fs.mkdir(root, { recursive: true });
  const binary = path.join(root, process.platform === 'win32' ? 'anima-daemon.exe' : 'anima-daemon');
  const workspace = path.join(root, 'workspace');
  for (const name of ['workspace/fixtures', 'workspace/outputs', 'home/AppData/Roaming', 'home/AppData/Local', 'tmp']) await fs.mkdir(path.join(root, name), { recursive: true });
  await fs.copyFile(sourceBinary, binary, fs.constants.COPYFILE_EXCL);
  if (process.platform !== 'win32') await fs.chmod(binary, 0o700);
  for (const name of ['baseline.csv', 'fresh.csv']) await fs.copyFile(path.join(fixtureRoot, name), path.join(workspace, 'fixtures', name));
  await fs.writeFile(tokenFile, randomBytes(32).toString('hex'), { flag: 'wx', mode: 0o600 });
  state = { schema: 1, root, workspace, binary, sourceBinary, binarySha256: sha(await fs.readFile(binary)), keychainNamespace: `sl-acceptance-${sha(root).slice(0, 12)}`, provider, model, createdAt: new Date().toISOString(), processes: [] };
  await saveState();
  report({ prepared: true, root, provider, model, binarySha256: state.binarySha256, noDaemonStarted: true });
}
async function selfTest() {
  const baseline = await fs.readFile(path.join(fixtureRoot, 'baseline.csv'), 'utf8');
  const fresh = await fs.readFile(path.join(fixtureRoot, 'fresh.csv'), 'utf8');
  assert.equal(expectedBytes(baseline).toString(), '{"row_count":4,"gross_sales_cents":14500,"refund_cents":2000,"net_sales_cents":12500}');
  assert.equal(expectedBytes(baseline, true).toString(), '{"row_count":4,"gross_sales_cents":14500,"refund_cents":2000,"net_sales_cents":16500}');
  assert.equal(expectedBytes(fresh).toString(), '{"row_count":5,"gross_sales_cents":17000,"refund_cents":2025,"net_sales_cents":14975}');
  assert.throws(() => parseFixture('id,kind,amount_cents\nx,sale,-1\n'));
  assert.throws(() => parseFixture('id,kind,amount_cents\nx,sale,1\nx,refund,1\n'));
  const identity = await processIdentity(process.pid);
  assert(identity && samePath(identity.path, process.execPath), 'Owned-PID identity check failed');
  secrets = ['synthetic-secret-value'];
  assert(!clean({ message: 'synthetic-secret-value' }).includes('synthetic-secret-value'));
  report({ selfTestPassed: true, fixtureOracles: 3, invalidFixturesRejected: 2, redactionVerified: true, processIdentityVerified: true, providerCalls: 0 });
}
async function main() {
  if (phase === 'help') { report('Phases: prepare --binary PATH [--provider moonshot|anthropic], start, setup, baseline, correction, review, fresh, wait baseline|correction|fresh, inspect, checkpoint, stop, restart, reload, undo, proof, self-test. Optional --root must remain inside repo/.tmp. Only baseline/correction/fresh submit model calls; review observes the automatic reviewer.'); return; }
  if (phase === 'self-test') return selfTest();
  if (phase === 'prepare') return prepare();
  await load();
  if (phase === 'start') return start();
  if (phase === 'restart') return start(true);
  if (phase === 'stop') return stop();
  if (phase === 'setup') return setup();
  if (['baseline', 'correction', 'fresh'].includes(phase)) return submit(phase);
  if (phase === 'wait') { await assertLive(); assert(['baseline', 'correction', 'fresh'].includes(args[1])); return waitRun(args[1]); }
  if (phase === 'review') return review();
  if (phase === 'inspect') {
    await assertLive();
    const result = { state, status: await api('GET', '/api/status'), reviews: state.agentId ? await api('GET', `/api/agents/${state.agentId}/learning-reviews`) : null, learning: state.setup ? await skillLearning() : null };
    await save('inspection.json', result);
    report({ inspection: path.join(root, 'inspection.json'), runs: result.status.runs, reviews: result.reviews?.reviews.map(r => ({ id: r.id, state: r.state, sourceRunId: r.sourceRunId })) });
    return;
  }
  if (phase === 'checkpoint') {
    await assertLive(); assert(state.fresh?.verified && !state.checkpoint);
    await api('PATCH', `/api/agents/${state.agentId}/learning-policy`, { autoEnabled: false });
    await save('checkpoint.json', await snapshot());
    state.checkpoint = true; await saveState(); report({ checkpoint: true }); return;
  }
  if (phase === 'reload') {
    await assertLive(); assert(state.restarted && !state.undo);
    assert.deepEqual(await snapshot(), await read('checkpoint.json'), 'Restart changed settled skill/evidence/approval/undo/memory state');
    state.reloadVerified = true; await saveState(); await save('reload-proof.json', { exactReload: true }); report({ exactReload: true }); return;
  }
  if (phase === 'undo') {
    await assertLive(); assert(state.reloadVerified && !state.undo);
    const result = await api('POST', `/api/skills/${slug}/revert-last`, {});
    assert.equal(result.removed, false); assert.equal(result.skill.approvedHash, state.originalHash);
    const detail = await api('GET', `/api/skills/${slug}`);
    const baseline = await read('baseline-skill.json');
    assert.equal(detail.file.body, baseline.file.body); assert.deepEqual(detail.file.files, baseline.file.files);
    const entry = (await api('GET', '/api/skill-ledger')).entries.find(e => e.id === state.review.ledgerId);
    assert(entry.revertedAt);
    state.undo = { verified: true, restoredHash: detail.file.hash };
    await saveState(); await save('undo-proof.json', { ...state.undo, entry, supportingFilesPreserved: true }); report(state.undo); return;
  }
  if (phase === 'proof') {
    assert(state.baseline?.verified && state.correction?.verified && state.review?.verified && state.fresh?.verified && state.reloadVerified && state.undo?.verified, 'Required acceptance phases are incomplete');
    assert(state.processes.length === 2 && state.processes.every(p => p.stopped), 'Every owned process must be stopped');
    for (const record of state.processes) { const current = await processIdentity(record.pid); assert(!current || current.created !== record.created, 'Owned process remains alive'); }
    await portFree();
    const proof = { liveProviderAcceptance: true, provider: state.provider, model: state.model, binarySha256: state.binarySha256, baselineIncorrectNet: state.baseline.output.value.net_sales_cents, correctedNet: state.correction.output.value.net_sales_cents, freshCorrectNet: state.fresh.output.value.net_sales_cents, originalHash: state.originalHash, learnedHash: state.learnedHash, freshActualLoadedHash: state.fresh.version, independentExactOutput: true, freshEmptyAgentAndSession: true, foregroundTools: allowedTools, explicitAndNativeLoadCovered: true, automaticReview: state.review, exactRestartPersistence: true, undo: state.undo, allOwnedProcessesStopped: true };
    await save('acceptance-proof.json', proof); report(proof); return;
  }
  throw new Error(`Unknown phase: ${phase}`);
}
main().catch(error => { console.error(clean({ failed: phase, message: error.message, noAutomaticResubmission: true })); process.exitCode = 1; });
