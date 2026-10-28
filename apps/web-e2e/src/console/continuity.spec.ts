import { expect, test, type Page } from '@playwright/test';
import type {
  WorkflowAttemptEvidence,
  WorkflowResumeInput,
  WorkflowRun,
} from '@animaos/sdk';
import { installConsoleFixture } from './fixtures/api';
import { AGENT, createState } from './fixtures/state';

function savedWorkflow(uncertain = false): WorkflowRun {
  return {
    id: 'saved-workflow', agentId: AGENT, title: 'Prepare the launch report',
    requestKey: 'saved-launch', revision: 4, paused: false,
    status: uncertain ? 'needs_review' : 'awaiting_approval',
    currentStep: uncertain ? 0 : 1,
    steps: [
      { id: 'report', title: 'Prepare report', kind: 'task', prompt: 'Use selected evidence only.',
        status: uncertain ? 'needs_review' : 'completed', jobId: 'job-saved', attempt: 1,
        attempts: ['job-saved'], result: uncertain ? null : 'Saved report <script> remains inert.' },
      { id: 'review', title: 'Review the report', kind: 'approval', prompt: 'Review before continuing.',
        status: uncertain ? 'pending' : 'awaiting_approval', jobId: null, attempt: 0, attempts: [], result: null },
      { id: 'follow-up', title: 'Prepare follow-up', kind: 'task', prompt: 'Wait for the review checkpoint.',
        status: 'pending', jobId: null, attempt: 0, attempts: [], result: null },
    ],
    decisions: [], createdAtMs: 1, updatedAtMs: 2, error: uncertain ? 'Execution interrupted; check saved effects.' : null,
  };
}

async function continuityFixture(page: Page, uncertain = false) {
  const state = createState();
  // This workflow belongs to one agent; the base fixture also retains an older helper.
  state.agents = state.agents.filter((agent) => agent.state.id === AGENT);
  const fixture = await installConsoleFixture(page, state, { chatSuite: {} });
  const workflow = savedWorkflow(uncertain);
  const decisions: WorkflowResumeInput[] = [];
  const inspections: string[] = [];
  const unexpected: string[] = [];
  let staleEvidence = false;
  const base = `/api/agents/${AGENT}/workflows`;
  await page.route(`**/api/agents/${AGENT}/tasks`, (route) =>
    route.fulfill({ json: { tasks: [], revision: 'saved-task-list' } }));
  await page.route(`**${base}**`, async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (request.method() === 'GET' && path === base)
      return route.fulfill({ json: { workflows: [workflow] } });
    if (request.method() === 'POST' && path === `${base}/${workflow.id}/resume` && !uncertain) {
      const input = request.postDataJSON() as WorkflowResumeInput;
      expect(input).toEqual({ revision: workflow.revision, action: workflow.paused ? 'continue' : 'pause' });
      decisions.push(input);
      workflow.paused = !workflow.paused;
      workflow.decisions.push({ stepIndex: workflow.currentStep, request: input, decidedAtMs: 10 + decisions.length });
      workflow.revision += 1;
      workflow.updatedAtMs += 1;
      return route.fulfill({ json: workflow });
    }
    if (request.method() === 'GET' && path === `${base}/${workflow.id}/steps/report/attempts/job-saved`) {
      inspections.push(path);
      const evidence: WorkflowAttemptEvidence = {
        workflowId: workflow.id, workflowRevision: workflow.revision + (staleEvidence ? 1 : 0),
        stepId: 'report', jobId: 'job-saved', stepAttempt: 1, status: 'needs_review',
        startedAtMs: 1, finishedAtMs: 2, result: null, error: 'Interrupted',
        resultTruncated: false, evidenceState: 'retained',
        run: { id: 'saved-run', sessionId: 'saved-session', status: 'interrupted', evidence: {
          actionReceipts: [{ toolCallId: 'saved-delivery', name: 'send_message', status: 'started',
            startedAtMs: 1, finishedAtMs: null, durationMs: null, argumentsPreview: 'Prepared launch update',
            argumentsTruncated: false, resultPreview: null, resultTruncated: false, durability: 'persistent' }],
          unreceiptedTools: [], omittedUnreceiptedToolCount: 0, checkpoint: null,
        } },
      };
      return route.fulfill({ json: evidence });
    }
    unexpected.push(`${request.method()} ${path}`);
    return route.fulfill({ status: 501, json: { error: 'Unexpected continuity fixture operation' } });
  });
  return { fixture, workflow, decisions, inspections, makeEvidenceStale: () => { staleEvidence = true; },
    assertClean: () => { expect(unexpected).toEqual([]); fixture.assertClean(); } };
}

async function openWorkflows(page: Page) {
  await page.goto('/#/work');
  const work = page.getByRole('region', { name: 'Work', exact: true });
  await work.getByRole('tab', { name: /^Workflows/ }).click();
  return work.getByRole('article', { name: 'Prepare the launch report', exact: true });
}

async function expectNoWorkOverflow(page: Page) {
  await expect.poll(() => page.evaluate(() => {
    const work = document.querySelector<HTMLElement>('.work-page');
    return {
      page: document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1,
      work: work !== null && work.scrollWidth <= work.clientWidth + 1,
    };
  })).toEqual({ page: true, work: true });
}

for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
  test.describe(`saved work at ${viewport.width}px`, () => {
    test.use({ viewport });

    test('pause survives reload and continuing leaves the approval checkpoint waiting', async ({ page }) => {
      const control = await continuityFixture(page);
      let card = await openWorkflows(page);
      await control.fixture.waitForStream();
      await expect(card.getByText('1 of 3 steps finished · 2 remaining', { exact: true })).toBeVisible();
      await expect(card.getByText('Saved report <script> remains inert.', { exact: true })).toBeVisible();
      await card.getByRole('button', { name: 'Pause work', exact: true }).click();
      await expect(card.getByText('Paused. No new task will start. A task already started may finish.', { exact: true })).toBeVisible();
      await expect(card.getByRole('button', { name: 'Approve checkpoint', exact: true })).toBeDisabled();
      await expectNoWorkOverflow(page);

      await page.reload();
      const work = page.getByRole('region', { name: 'Work', exact: true });
      await work.getByRole('tab', { name: /^Workflows/ }).click();
      card = work.getByRole('article', { name: 'Prepare the launch report', exact: true });
      await expect(card.getByRole('button', { name: 'Continue work', exact: true })).toBeEnabled();
      await expect(card.getByRole('button', { name: 'Approve checkpoint', exact: true })).toBeDisabled();
      expect(control.decisions).toEqual([{ revision: 4, action: 'pause' }]);
      await card.getByRole('button', { name: 'Continue work', exact: true }).click();
      await expect(card.getByRole('button', { name: 'Approve checkpoint', exact: true })).toBeEnabled();
      await expect(card.getByText('Awaiting approval', { exact: true }).first()).toBeVisible();
      await expect(card.getByText('1 of 3 steps finished · 2 remaining', { exact: true })).toBeVisible();
      await card.getByText('Saved owner decisions', { exact: true }).click();
      await expect(card.getByText(/Revision 4: pause/)).toBeVisible();
      await expect(card.getByText(/Revision 5: continue/)).toBeVisible();
      expect(control.decisions).toEqual([{ revision: 4, action: 'pause' }, { revision: 5, action: 'continue' }]);
      expect(control.workflow.steps[2]).toMatchObject({ status: 'pending', jobId: null, attempts: [] });
      await expectNoWorkOverflow(page);
      control.assertClean();
    });

    test('saved unknown effects stay read-only and stale evidence blocks retry', async ({ page }) => {
      const control = await continuityFixture(page, true);
      const card = await openWorkflows(page);
      await control.fixture.waitForStream();
      const retry = card.getByRole('button', { name: 'Retry task', exact: true });
      await expect(retry).toBeDisabled();
      await card.getByRole('button', { name: 'Inspect saved attempt 1', exact: true }).click();
      await expect(card.getByRole('region', { name: 'Saved evidence', exact: true })).toContainText('send_message · Outcome unknown');
      await expect(card.getByText('Saved evidence may be incomplete. Check effects before continuing.', { exact: true })).toBeVisible();
      await expect(retry).toBeDisabled();
      await card.getByRole('checkbox', { name: /I have reviewed the uncertain effects/ }).check();
      await expect(retry).toBeEnabled();
      control.makeEvidenceStale();
      await card.getByRole('button', { name: 'Inspect saved attempt 1', exact: true }).click();
      await expect(card.getByRole('alert')).toHaveText('The workflow changed. Refresh before deciding.');
      await expect(retry).toBeDisabled();
      expect(control.inspections).toHaveLength(2);
      expect(control.decisions).toEqual([]);
      await expectNoWorkOverflow(page);
      control.assertClean();
    });
  });
}
