import { test, expect } from '@playwright/test';
import type { Approval } from '@animaos/sdk';
import { openConsole, send, about } from './fixtures/journeys';

for (const decision of ['Allow once', 'Deny'])
  test(`pending approval: ${decision} settles once`, async ({ page }) => {
    const fixture = await openConsole(page),
      run = await send(page, fixture);
    const approval: Approval = {
      id: 'approval_1',
      agentId: run.agentId,
      sessionId: run.sessionId,
      runId: run.id,
      toolCallId: 'call_1',
      tool: 'bash',
      class: 'exec',
      arguments: '{"command":"git status"}',
      argumentsTruncated: false,
      suggestedMatcher: { kind: 'command_prefix', value: 'git status' },
      matcherKinds: ['command_prefix', 'any'],
      createdAtMs: Date.now(),
      expiresAtMs: Date.now() + 1800000,
      status: 'pending',
      revision: 1,
      resolution: null,
    };
    fixture.requestApproval(approval);
    await fixture.emit({
      type: 'tool.started',
      ...about(run),
      stepId: `${run.id}:1`,
      toolCallId: 'call_1',
      name: 'bash',
      argumentsPreview: approval.arguments,
      argumentsTruncated: false,
    });
    await fixture.emit({ type: 'run.awaiting_approval', ...about(run), run });
    await fixture.emit({ type: 'approval.requested', ...about(run), approval });
    await page.getByRole('button', { name: decision, exact: true }).click();
    await expect
      .poll(
        () => fixture.calls.filter((c) => c.path.endsWith('/decision')).length,
      )
      .toBe(1);
    await fixture.emit({ type: 'approval.resolved', ...about(run), approval });
    await fixture.emit({
      type: 'tool.finished',
      ...about(run),
      stepId: `${run.id}:1`,
      toolCallId: 'call_1',
      name: 'bash',
      status: decision === 'Deny' ? 'error' : 'success',
      durationMs: 1,
      resultPreview: decision === 'Deny' ? 'Denied by owner' : 'Clean tree',
      truncated: false,
      recovered: false,
    });
    fixture.finish(run, 'Approval handled.');
    await fixture.emit({ type: 'run.completed', ...about(run), run });
    await expect(
      page.getByText('Approval handled.', { exact: true }),
    ).toBeVisible();
    fixture.assertClean();
  });
test('Stop preserves partial reply and reload never replays a run', async ({
  page,
}) => {
  const fixture = await openConsole(page),
    run = await send(page, fixture);
  await fixture.emit({
    type: 'step.delta',
    ...about(run),
    stepId: `${run.id}:1`,
    offset: 0,
    text: 'A partial answer',
  });
  await expect(
    page.getByText('A partial answer', { exact: true }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Stop', exact: true }).click();
  await expect.poll(() => run.status).toBe('cancelled');
  fixture.persistPartial(run, 'A partial answer');
  await fixture.emit({ type: 'run.cancelled', ...about(run), run });
  await expect(
    page.getByText('Stopped', { exact: true }).first(),
  ).toBeVisible();
  await page.reload();
  await expect(
    page.getByText('A partial answer', { exact: true }),
  ).toBeVisible();
  expect(fixture.calls.filter((c) => c.path.endsWith('/stop'))).toHaveLength(1);
  expect(
    fixture.calls.filter(
      (c) => c.method === 'POST' && c.path.endsWith('/runs'),
    ),
  ).toHaveLength(1);
  fixture.assertClean();
});
