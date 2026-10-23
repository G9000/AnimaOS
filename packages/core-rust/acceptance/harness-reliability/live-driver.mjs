import { readFile, writeFile, mkdir, open, rename } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { spawn, execFileSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, resolve, join } from 'node:path';
import { performance } from 'node:perf_hooks';

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const canonical = (value) => JSON.stringify(Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b))));
const directory = dirname(fileURLToPath(import.meta.url));
const repository = resolve(directory, '../../../..');
const worker = 'app::harness_reliability_tests::live_trace::trace_live_worker';
const comparator = 'app::harness_reliability_tests::live_trace::trace_live_compare';
export function validateArtifact(plan, row, artifact) {
  const trace = artifact?.graded?.trace;
  return artifact?.traceComplete === true && artifact.fixtureFileSha256 === plan.fixtureFileSha256 && artifact.hostPolicy && canonical(artifact.hostPolicy) === canonical(plan.hostPolicy) && trace?.fixtureId === row.fixtureId && trace?.repetition === row.repetition && trace?.variant === row.variant && trace?.settings?.provider === plan.provider && trace?.settings?.model === plan.model && Array.isArray(artifact?.graded?.oracleFailures);
}

export function createPlan({ fixtures, builds, model, repetitions = 2, timeoutMs = 45000 }) {
  if (!Array.isArray(fixtures) || fixtures.length !== 6 || new Set(fixtures).size !== fixtures.length || fixtures.some((id) => !/^[a-z0-9-]{1,80}$/.test(id))) throw new Error('the six unique existing fixture IDs are required');
  if (!Number.isInteger(repetitions) || repetitions < 1 || repetitions > 5) throw new Error('repetitions must be between 1 and 5');
  if (!Number.isInteger(timeoutMs) || timeoutMs < 100 || timeoutMs > 120000) throw new Error('trial timeout must be finite and at most 120000ms');
  if (typeof model !== 'string' || !model.trim() || model.length > 256) throw new Error('an explicit matched model is required');
  for (const variant of ['baseline', 'candidate']) {
    const build = builds?.[variant];
    if (!build?.binary || !/^[a-f0-9]{64}$/.test(build.binarySha256) || !/^[a-f0-9]{64}$/.test(build.sourceSnapshotSha256) || !/^[a-f0-9]{40,64}$/.test(build.sourceCommit)) throw new Error(`${variant} requires binary and source identity`);
  }
  const rows = [];
  for (let repetition = 0; repetition < repetitions; repetition++) {
    // Alternate order without changing the matched settings or retrying a row.
    for (const fixtureId of fixtures) for (const variant of repetition % 2 ? ['candidate', 'baseline'] : ['baseline', 'candidate']) {
      rows.push({ id: `${fixtureId}-${repetition}-${variant}`, fixtureId, repetition, variant, status: 'planned' });
    }
  }
  return { version: 1, provider: 'anthropic', model, repetitions, timeoutMs, fixtures, builds, rows, matrixValidated: false, createdAt: new Date().toISOString(), hostPolicy: { tools: 'isolated-fixture-allowlist-v1', context: 'fresh-runtime-and-workspace-per-trial', maxToolIterations: 12, maxModelAttempts: 16, maxToolCalls: 48, maxRetries: 0, maxInflightOperations: 4, temperature: 0, maxOutputTokensPerStep: 2048, maxInputTokens: 4000000, maxOutputTokens: 262144, maxTotalTokens: 4262144, executionDeadlineMs: 40000 } };
}

async function saveJson(path, value) {
  const temporary = `${path}.next`;
  const file = await open(temporary, 'w');
  try { await file.writeFile(`${JSON.stringify(value, null, 2)}\n`); await file.sync(); }
  finally { await file.close(); }
  await rename(temporary, path);
}

export async function executePlan(plan, output, runTrial) {
  await mkdir(output, { recursive: true });
  await saveJson(join(output, 'plan.json'), plan);
  for (const row of plan.rows) {
    // This driver never resumes uncertain rows or retries an external request.
    if (row.status !== 'planned') throw new Error('an experiment directory cannot be resumed');
    row.status = 'running';
    row.startedAt = new Date().toISOString();
    await saveJson(join(output, 'plan.json'), plan);
    let result;
    try { result = await runTrial(row); }
    catch (error) { result = { status: 'failed', error: error?.code ?? error?.name ?? 'worker failed' }; }
    Object.assign(row, result, { finishedAt: new Date().toISOString() });
    if (row.status === 'completed') {
      const evidence = row.evidence;
      if (!evidence || evidence.fixtureId !== row.fixtureId || evidence.repetition !== row.repetition || evidence.variant !== row.variant || evidence.traceComplete !== true || !Array.isArray(evidence.oracleFailures)) row.status = 'invalid_evidence';
    }
    await saveJson(join(output, 'plan.json'), plan);
    if (row.status !== 'completed') break;
  }
  await saveJson(join(output, 'summary.json'), summarize(plan));
  return plan;
}

export function summarize(plan) {
  const complete = plan.rows.length === plan.fixtures.length * plan.repetitions * 2 && plan.rows.every((row) => row.status === 'completed');
  const identicalBuilds = plan.builds.baseline.binarySha256 === plan.builds.candidate.binarySha256;
  return { planned: plan.rows.length, completed: plan.rows.filter((row) => row.status === 'completed').length, complete, matrixValidated: plan.matrixValidated === true, comparisonAccepted: complete && plan.matrixValidated === true, comparisonKind: identicalBuilds ? 'repeatability-control' : 'matched-build-comparison', improvementClaimAllowed: false, allFixtureOraclesPassed: complete && plan.rows.every((row) => row.evidence.oracleFailures.length === 0), limitation: 'Observed bounded executions only; no automatic quality, cost, promotion or approval claim.' };
}

export async function runProcess(binary, args, { timeoutMs, env, cwd = repository }) {
  const start = performance.now();
  return await new Promise((done) => {
    let output = '', timedOut = false, spawnError;
    const child = spawn(binary, args, { cwd, env, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    const capture = (bytes) => { if (output.length < 2 * 1024 * 1024) output += bytes.toString('utf8').slice(0, 2 * 1024 * 1024 - output.length); };
    child.stdout.on('data', capture); child.stderr.on('data', capture);
    const timer = setTimeout(() => { timedOut = true; child.kill('SIGKILL'); }, timeoutMs);
    child.on('error', (error) => { spawnError = error.code ?? error.name; });
    child.once('close', (code, signal) => { clearTimeout(timer); done({ code, signal, timedOut, spawnError, output, elapsedMs: Math.round(performance.now() - start) }); });
  });
}

function childEnvironment(explicitKey) {
  const env = {};
  for (const key of ['SystemRoot', 'PATH', 'TEMP', 'TMP', 'HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'SSL_CERT_FILE', 'SSL_CERT_DIR']) if (process.env[key]) env[key] = process.env[key];
  return { ...env, ANIMA_RUN_TRACE_LIVE: '1', ANTHROPIC_API_KEY: explicitKey, ANTHROPIC_BASE_URL: 'https://api.anthropic.com', RUST_TEST_THREADS: '1' };
}

async function loadBuild(manifestPath) {
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
  const binary = resolve(dirname(manifestPath), manifest.binary);
  const binarySha256 = sha256(await readFile(binary));
  if (binarySha256 !== manifest.binarySha256) throw new Error('binary changed after its build manifest was recorded');
  return { ...manifest, binary, binarySha256 };
}

// Records exact artifact bytes and the current source snapshot, without claiming
// that this observation proves how an externally built binary was produced.
async function recordBuild(binary, output) {
  const files = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], { cwd: repository }).toString('utf8').split('\0').filter((file) => file && (/\.(rs|toml|lock)$/.test(file) || file === 'rust-toolchain.toml')).sort();
  const hash = createHash('sha256');
  for (const file of files) { hash.update(file); hash.update('\0'); hash.update(await readFile(join(repository, file))); hash.update('\0'); }
  await saveJson(output, { binary: resolve(binary), binarySha256: sha256(await readFile(binary)), sourceSnapshotSha256: hash.digest('hex'), sourceCommit: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repository }).toString('utf8').trim(), recordedAt: new Date().toISOString(), provenance: 'artifact hash plus source snapshot observed at recording; build origin requires the retained build log' });
}

async function main() {
  const args = process.argv.slice(2);
  if (args[0] === '--record-build' && args.length === 3) { await recordBuild(args[1], resolve(args[2])); return; }
  if (args.length !== 1) throw new Error('usage: node live-driver.mjs experiment.json; or --record-build test-binary manifest.json');
  if (process.env.ANIMA_RUN_TRACE_LIVE !== '1' || !process.env.ANTHROPIC_API_KEY?.trim()) throw new Error('explicit ANIMA_RUN_TRACE_LIVE=1 and ANTHROPIC_API_KEY are required; .env is never loaded');
  const key = process.env.ANTHROPIC_API_KEY;
  const sanitize = (text) => text.replaceAll(key, '[REDACTED]');
  const configPath = resolve(args[0]);
  const config = JSON.parse(await readFile(configPath, 'utf8'));
  const fixturesBytes = await readFile(join(directory, '../turn-quality/fixtures.json'));
  const fixtures = JSON.parse(fixturesBytes);
  const builds = { baseline: await loadBuild(resolve(dirname(configPath), config.baselineManifest)), candidate: await loadBuild(resolve(dirname(configPath), config.candidateManifest)) };
  const plan = createPlan({ fixtures: fixtures.map((f) => f.id), builds, model: config.model, repetitions: config.repetitions, timeoutMs: config.timeoutMs });
  plan.fixtureFileSha256 = sha256(fixturesBytes);
  const output = resolve(dirname(configPath), config.output);
  // mkdir without recursive is an exclusivity guard: each experiment is new.
  await mkdir(output);
  const env = childEnvironment(key);
  await executePlan(plan, output, async (row) => {
    const trial = join(output, row.id); await mkdir(trial);
    const binary = plan.builds[row.variant].binary;
    const result = await runProcess(binary, ['--exact', worker, '--ignored', '--nocapture'], { timeoutMs: plan.timeoutMs, env: { ...env, ANIMA_TRACE_FIXTURE: row.fixtureId, ANIMA_TRACE_REPETITION: String(row.repetition), ANIMA_TRACE_VARIANT: row.variant, ANIMA_TRACE_MODEL: plan.model, ANIMA_TRACE_TRIAL_OUTPUT: trial } });
    await writeFile(join(trial, 'worker.log'), sanitize(result.output));
    if (result.timedOut) return { status: 'timed_out', elapsedMs: result.elapsedMs };
    if (result.code !== 0) return { status: 'failed', elapsedMs: result.elapsedMs, exitCode: result.code, signal: result.signal };
    let artifact;
    try { artifact = JSON.parse(await readFile(join(trial, 'grade.json'), 'utf8')); } catch { return { status: 'invalid_evidence' }; }
    const trace = artifact?.graded?.trace;
    const valid = validateArtifact(plan, row, artifact);
    return { status: valid ? 'completed' : 'invalid_evidence', elapsedMs: result.elapsedMs, evidence: { fixtureId: trace?.fixtureId, repetition: trace?.repetition, variant: trace?.variant, traceComplete: valid, oracleFailures: artifact?.graded?.oracleFailures }, artifact: `${row.id}/grade.json` };
  });
  if (summarize(plan).complete) {
    const result = await runProcess(plan.builds.candidate.binary, ['--exact', comparator, '--ignored', '--nocapture'], { timeoutMs: 15000, env: { ...env, ANTHROPIC_API_KEY: '', ANIMA_TRACE_MATRIX: output, ANIMA_TRACE_REPETITIONS: String(plan.repetitions) } });
    await writeFile(join(output, 'comparison.log'), sanitize(result.output));
    plan.matrixValidated = !result.timedOut && result.code === 0;
    await saveJson(join(output, 'plan.json'), plan);
  }
  const report = summarize(plan);
  await saveJson(join(output, 'summary.json'), report);
  console.log(JSON.stringify(report));
  if (!report.comparisonAccepted) process.exitCode = 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main().catch((error) => { console.error(error.message); process.exitCode = 1; });
