import { expect, test, type Page, type Route } from '@playwright/test';
import type {
  RunOutcome,
  RegressionSuite,
  TaskRegressionConfiguration,
  TaskRegressionReport,
  TaskRegressionSuiteSummary,
} from '@animaos/sdk';
import { installConsoleFixture } from './fixtures/api';
import { AGENT, ORIGIN, createState, queuedRun, session } from './fixtures/state';

// Browser wiring evidence only. All assets, API replies, reports and streams are
// supplied locally. This file never starts a daemon, provider, or native worker.
test.use({ serviceWorkers: 'block' });

function isFeatureEndpoint(path: string): boolean {
  return /^\/api\/agents\/[^/]+\/runs\/[^/]+\/outcome(?:\/|$)/.test(path) ||
    /^\/api\/task-regression(?:s|-suites|-configs)(?:\/|$)/.test(path);
}

async function prepareConsole(page: Page, outcome?: RunOutcome) {
  const state = createState();
  state.agents = state.agents.slice(0, 1);
  const chat = {
    ...session('chat:outcome-browser', 'Outcome browser fixture'),
    mainChat: true,
    clearGeneration: 0,
    messageCount: outcome ? 2 : 0,
  };
  state.sessions = [chat];
  state.messages[chat.id] = [];
  if (outcome) {
    const run = {
      ...queuedRun(chat.id, { text: 'Prepare the fixture report', attachmentIds: [], skill: null }, 'fixture-completed'),
      id: outcome.rootRunId,
      status: 'completed' as const,
      startedAtMs: 10,
      finishedAtMs: 20,
      replyMessageId: 'fixture-outcome-reply',
      outcome,
    };
    state.runs = [run];
    state.messages[chat.id] = [
      { id: 'fixture-outcome-prompt', role: 'user', text: run.input.text, attachments: [], metadata: { runId: run.id }, createdAtMs: 10 },
      { id: run.replyMessageId, role: 'assistant', text: 'The report is ready for your review.', attachments: [], metadata: { runId: run.id }, createdAtMs: 20 },
    ];
  }
  // realStatic/realMemory/realWorkspaceFiles remain unset. The shared fallback
  // fulfills contained dist files, answers API calls, and aborts other origins.
  const fixture = await installConsoleFixture(page, state, { chatSuite: {} });
  const violations: string[] = [];
  const record = (route: Route) => {
    const request = route.request();
    const url = new URL(request.url());
    expect(url.origin).toBe(ORIGIN);
    const body = request.postData() ? request.postDataJSON() as Record<string, unknown> : {};
    const call = {
      method: request.method(), path: decodeURIComponent(url.pathname), body,
      key: request.headers()['idempotency-key'] ?? null, headers: request.headers(),
    };
    fixture.calls.push(call);
    return call;
  };
  const reject = async (route: Route) => {
    violations.push(`${route.request().method()} ${new URL(route.request().url()).pathname}`);
    await route.fulfill({ status: 501, json: { error: 'Unimplemented offline acceptance request' } });
  };
  // Profile mounts the existing personality summary alongside the new entry.
  // Supply that unrelated read locally instead of touching a real profile.
  await page.route((url) => url.pathname === `/api/agents/${AGENT}/personality`, async (route) => {
    const call = record(route);
    if (call.method !== 'GET') return reject(route);
    await route.fulfill({ json: { source: 'agent_config', path: null, content: 'Synthetic browser personality.', revision: '0'.repeat(64), maxChars: 16_384 } });
  });
  const open = async () => {
    await page.goto('/');
    await expect(page.getByPlaceholder('Message Anima…')).toBeEditable();
    await fixture.waitForStream();
  };
  const assertClean = () => {
    expect(violations).toEqual([]);
    fixture.assertClean();
  };
  return { state, chat, fixture, record, reject, open, assertClean };
}

for (const [name, viewport] of [
  ['desktop', { width: 1280, height: 900 }],
  ['mobile', { width: 390, height: 844 }],
] as const) {
  test(`criteria stay opt-in and accompany only an explicit send on ${name}`, async ({ page }, testInfo) => {
    await page.setViewportSize(viewport);
    const { state, chat, fixture, record, open, assertClean } = await prepareConsole(page);
    const submitted: Record<string, unknown>[] = [];
    await page.route((url) => /^\/api\/agents\/companion\/sessions\/[^/]+\/runs$/.test(url.pathname), async (route) => {
      if (route.request().method() !== 'POST') return route.fallback();
      const call = record(route);
      expect(call.path).toBe(`/api/agents/${AGENT}/sessions/${chat.id}/runs`);
      expect(call.key).toBeTruthy();
      submitted.push(call.body);
      const run = {
        ...queuedRun(chat.id, { text: String(call.body.text), attachmentIds: [], skill: null }, call.key!),
        id: 'fixture-queued-outcome',
      };
      state.runs.push(run);
      chat.activeRuns = 1;
      await route.fulfill({ status: 202, json: { run } });
    });
    await open();
    await expect(page.getByRole('navigation', { name: 'Chat controls' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: /Change how it works/ })).toHaveCount(0);
    await expect(page.locator('summary').filter({ hasText: /^Success criteria$/ })).toHaveCount(0);
    expect(fixture.calls.some((call) => isFeatureEndpoint(call.path))).toBe(false);

    // Exercise the real settings route and checkbox rather than injecting the
    // preference through localStorage or calling the component's implementation.
    await page.evaluate(() => { location.hash = '#/appearance'; });
    await page.getByRole('checkbox', { name: /Advanced input/ }).check();
    await page.getByRole('button', { name: 'Open companion chat' }).click();
    await page.locator('summary').filter({ hasText: /^Success criteria$/ }).click();
    await page.getByRole('checkbox', { name: 'Check this result' }).check();
    await page.getByPlaceholder('Message Anima…').fill('Prepare a summary in the workspace.');
    await page.getByRole('button', { name: 'Send', exact: true }).click();
    await expect(page.getByRole('alert')).toContainText('Enter the text the result must contain.');
    expect(submitted).toEqual([]);

    await page.getByRole('combobox', { name: 'Check', exact: true }).selectOption('file_text');
    await page.getByRole('textbox', { name: 'Workspace file', exact: true }).fill('reports/summary.txt');
    await page.getByRole('textbox', { name: 'Required text', exact: true }).fill('Evidence checked');
    await page.getByRole('combobox', { name: 'Maximum automatic corrections', exact: true }).selectOption('1');
    await page.getByRole('checkbox', { name: 'Require my acceptance' }).check();
    expect(submitted).toEqual([]);
    await page.screenshot({ path: testInfo.outputPath(`criteria-ready-${name}.png`), fullPage: true, animations: 'disabled' });
    await page.getByRole('button', { name: 'Send', exact: true }).click();
    await expect.poll(() => submitted.length).toBe(1);
    expect(submitted[0]).toEqual({
      text: 'Prepare a summary in the workspace.', mode: 'queue', timeZone: expect.any(String),
      outcome: { criteria: [{ id: 'result', kind: 'file_text', path: 'reports/summary.txt', contains: 'Evidence checked' }], maxCorrections: 1, acceptance: 'owner_required' },
    });
    expect(String(submitted[0].timeZone)).not.toBe('');
    expect(fixture.calls.filter((call) => call.path.endsWith('/outcome/accept'))).toEqual([]);
    assertClean();
  });
}

test('meeting criteria waits for a separate revision-bound owner acceptance', async ({ page }, testInfo) => {
  const outcome: RunOutcome = {
    definition: { criteria: [{ kind: 'file_text', id: 'result', path: 'report.txt', contains: 'Evidence checked' }], maxCorrections: 1, acceptance: 'owner_required' },
    revision: 2, rootRunId: 'fixture-outcome-run', taskSha256: '1'.repeat(64), sourceSha256: '2'.repeat(64),
    modelSha256: '3'.repeat(64), workspaceSha256: '4'.repeat(64), terminalReplySha256: '5'.repeat(64),
    iterations: [{ ordinal: 1, bindingSha256: '6'.repeat(64), semanticSha256: '7'.repeat(64), replySha256: '5'.repeat(64), observations: [{ criterionId: 'result', verdict: 'met', subjectSha256: '8'.repeat(64), detail: 'Public file requirement met on observed bytes' }] }],
    consumedCorrections: 0, status: 'awaiting_owner', reportSha256: '9'.repeat(64), acceptedReportSha256: null, repairPaths: [],
  };
  const { state, fixture, record, reject, open, assertClean } = await prepareConsole(page, outcome);
  const acceptance: Record<string, unknown>[] = [];
  await page.route((url) => /^\/api\/agents\/[^/]+\/runs\/[^/]+\/outcome(?:\/|$)/.test(url.pathname), async (route) => {
    const call = record(route);
    if (call.method !== 'POST' || call.path !== `/api/agents/${AGENT}/runs/${outcome.rootRunId}/outcome/accept`) return reject(route);
    acceptance.push(call.body);
    expect(call.body).toEqual({ revision: 2, reportSha256: outcome.reportSha256 });
    const accepted: RunOutcome = { ...outcome, revision: 3, status: 'accepted', acceptedReportSha256: outcome.reportSha256 };
    state.runs[0].outcome = accepted;
    await route.fulfill({ json: { outcome: accepted } });
  });
  await open();
  const status = page.locator('details').filter({ has: page.locator(':scope > summary').filter({ hasText: /^Success criteria need your acceptance/ }) });
  await expect(status).toBeVisible();
  expect(acceptance).toEqual([]);
  expect(fixture.calls.filter((call) => call.path.includes('/outcome'))).toEqual([]);
  await status.locator(':scope > summary').click();
  await expect(status).toContainText('Meeting criteria is separate from your acceptance.');
  await expect(status.getByRole('button', { name: 'Accept this report' })).toBeEnabled();
  expect(acceptance).toEqual([]);
  await status.getByRole('button', { name: 'Accept this report' }).click();
  await expect.poll(() => acceptance.length).toBe(1);
  await expect(page.locator('summary').filter({ hasText: /^Success criteria accepted by you/ })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Accept this report' })).toHaveCount(0);
  expect(fixture.calls.filter((call) => call.method === 'POST' && call.path.endsWith('/runs'))).toEqual([]);
  await page.screenshot({ path: testInfo.outputPath('owner-accepted-report.png'), fullPage: true, animations: 'disabled' });
  assertClean();
});

test('saved-task form keeps the oracle private and dispatches only on explicit Run', async ({ page }, testInfo) => {
  const { fixture, record, reject, open, assertClean } = await prepareConsole(page);
  const configuration: TaskRegressionConfiguration = { agentId: AGENT, configDigest: 'a'.repeat(64), provider: 'openai', model: 'test-model', system: null };
  const suites: TaskRegressionSuiteSummary[] = [];
  const definitions: RegressionSuite[] = [];
  const comparisons: Record<string, unknown>[] = [];
  const oracleCanary = 'PRIVATE_REPLY_ORACLE_248';
  fixture.expectHttpRefusal(`${ORIGIN}/api/task-regressions`);
  await page.route((url) => url.pathname.startsWith('/api/task-regression'), async (route) => {
    const call = record(route);
    if (call.path === `/api/task-regression-configs/${AGENT}` && call.method === 'GET') return route.fulfill({ json: { configuration } });
    if (call.path === '/api/task-regression-suites' && call.method === 'GET') return route.fulfill({ json: { suites } });
    if (call.path === '/api/task-regression-suites' && call.method === 'POST') {
      const suite = call.body as unknown as RegressionSuite;
      definitions.push(suite);
      const summary = { id: 'suite_browser', name: suite.name, digest: 'b'.repeat(64), fixtureCount: 1, createdAtMs: 10 };
      suites.push(summary);
      return route.fulfill({ status: 201, json: { suite: summary } });
    }
    if (call.path === '/api/task-regressions' && call.method === 'POST') {
      comparisons.push(call.body);
      // An injected ambiguous HTTP failure tests explicit retry/key retention;
      // neither request crosses this browser route or starts any real work.
      if (comparisons.length === 1) return route.fulfill({ status: 503, json: { error: 'Offline injected comparison failure' } });
      const pin = { agentId: AGENT, configDigest: configuration.configDigest, effectiveConfigDigest: 'c'.repeat(64), provider: 'openai', model: 'test-model', toolPolicy: 'files' as const };
      const report: TaskRegressionReport = {
        id: 'comparison_browser', status: 'inconclusive', suite: suites[0], baseline: pin, candidate: pin, repetitions: 1,
        plannedTrials: 2, completedTrials: 0, createdAtMs: 10, finishedAtMs: 11, retiredAtMs: 11,
        evidenceScope: 'runtimePlumbing', evaluatorDigest: 'd'.repeat(64), toolImplementationDigest: 'e'.repeat(64), hostSourceDigest: 'f'.repeat(64), planDigest: '0'.repeat(64),
        comparison: null, observations: [], usageComplete: false, limitations: ['Offline browser fixture; no model was contacted.'], error: null,
      };
      return route.fulfill({ status: 202, json: { report } });
    }
    return reject(route);
  });
  await open();
  expect(fixture.calls.filter((call) => call.path.startsWith('/api/task-regression'))).toEqual([]);
  await page.evaluate(() => { location.hash = '#/profile'; });
  const entry = page.getByRole('button', { name: 'Compare on saved tasks', exact: true });
  await expect(entry).toHaveAttribute('aria-expanded', 'false');
  expect(fixture.calls.filter((call) => call.path.startsWith('/api/task-regression'))).toEqual([]);
  await entry.click();
  const panel = page.getByRole('region', { name: 'Saved task comparisons' });
  await expect(panel.getByRole('textbox', { name: 'Candidate model', exact: true })).toHaveValue('test-model');
  await panel.getByRole('button', { name: 'Create task suite' }).click();
  await expect(panel.getByLabel('Synthetic suite JSON')).toBeHidden();
  await panel.getByRole('textbox', { name: 'Suite name', exact: true }).fill('Private expected total');
  await panel.getByRole('textbox', { name: 'Task instructions', exact: true }).fill('Read numbers.txt. Write their sum as JSON in total.txt and give a short completion reply.');
  await panel.locator('summary').filter({ hasText: /^Sample input \(optional\)$/ }).click();
  await panel.getByRole('textbox', { name: 'Input file name', exact: true }).fill('numbers.txt');
  await panel.getByRole('textbox', { name: 'Input file contents', exact: true }).fill('2\n3\n');
  await panel.getByRole('textbox', { name: 'Expected result file', exact: true }).fill('total.txt');
  await panel.getByRole('textbox', { name: 'Expected result contents', exact: true }).fill('{"total":5}');
  await panel.getByRole('textbox', { name: 'Expected reply', exact: true }).fill(oracleCanary);
  expect(comparisons).toEqual([]);
  await page.screenshot({ path: testInfo.outputPath('simple-saved-task-form.png'), fullPage: true, animations: 'disabled' });
  await panel.getByRole('button', { name: 'Save task suite', exact: true }).click();
  await expect.poll(() => definitions.length).toBe(1);
  expect(definitions[0]).toEqual({ name: 'Private expected total', fixtures: [{ id: 'write-result', actor: { prompt: 'Read numbers.txt. Write their sum as JSON in total.txt and give a short completion reply.', initialFiles: { 'numbers.txt': '2\n3\n' }, readableFiles: ['numbers.txt'], writableFiles: ['total.txt'], mockEffects: false }, oracle: { expectedFiles: { 'numbers.txt': '2\n3\n', 'total.txt': '{"total":5}' }, expectedOutput: oracleCanary, expectedMockEffects: [] } }] });
  expect(JSON.stringify(definitions[0].fixtures[0].actor)).not.toContain(oracleCanary);
  expect(JSON.stringify(definitions[0].fixtures[0].actor)).not.toContain('{"total":5}');
  expect(comparisons).toEqual([]);
  await expect(panel).toContainText('Task instructions and sample inputs are sent to the selected providers and may use paid credits.');
  const run = panel.getByRole('button', { name: 'Run comparison (uses selected models)', exact: true });
  await run.click();
  await expect(panel.getByRole('alert')).toBeVisible();
  expect(comparisons).toHaveLength(1);
  await run.click();
  await expect.poll(() => comparisons.length).toBe(2);
  expect(comparisons[1]).toEqual(comparisons[0]);
  expect(comparisons[0]).toEqual({ suiteId: 'suite_browser', suiteDigest: 'b'.repeat(64), baseline: { agentId: AGENT, configDigest: configuration.configDigest }, candidate: { agentId: AGENT, configDigest: configuration.configDigest, provider: 'openai', model: 'test-model', toolPolicy: 'files' }, repetitions: 1, requestKey: expect.any(String) });
  expect(String(comparisons[0].requestKey)).not.toBe('');
  expect(JSON.stringify(comparisons)).not.toContain(oracleCanary);
  expect(JSON.stringify(comparisons)).not.toContain('{"total":5}');
  await expect(panel.getByRole('article', { name: 'Task comparison report' })).toContainText('Complete task matrix unavailable; no task pass conclusion.');
  expect(fixture.calls.filter((call) => ['POST', 'PATCH'].includes(call.method) && call.path === `/api/agents/${AGENT}`)).toEqual([]);
  assertClean();
});
