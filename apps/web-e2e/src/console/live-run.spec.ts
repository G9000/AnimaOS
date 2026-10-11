import { test, expect } from '@playwright/test';
import { openConsole, send, about } from './fixtures/journeys';

test('accepted run streams UTF-16 deltas, tools and one persisted reply', async ({
  page,
}) => {
  const fixture = await openConsole(page),
    run = await send(page, fixture);
  const stepId = `${run.id}:1`;
  await fixture.emit({
    type: 'step.delta',
    ...about(run),
    stepId,
    offset: 0,
    text: 'Hello 🌏',
  });
  await expect(page.getByText('Hello 🌏', { exact: true })).toBeVisible();
  await fixture.emit({
    type: 'step.delta',
    ...about(run),
    stepId,
    offset: 'Hello 🌏'.length,
    text: ', friend',
  });
  await expect(
    page.getByText('Hello 🌏, friend', { exact: true }),
  ).toBeVisible();
  await fixture.emit({
    type: 'tool.started',
    ...about(run),
    stepId,
    toolCallId: 'call_1',
    name: 'read_file',
    argumentsPreview: '{}',
    argumentsTruncated: false,
  });
  await expect(
    page.getByText('read_file', { exact: true }).first(),
  ).toBeVisible();
  await fixture.emit({
    type: 'tool.finished',
    ...about(run),
    stepId,
    toolCallId: 'call_1',
    name: 'read_file',
    status: 'success',
    durationMs: 12,
    resultPreview: 'read complete',
    truncated: false,
    recovered: false,
  });
  // Persist the actual page before emitting lifecycle, just as a server does.
  fixture.finish(run, 'Your day is planned.');
  await fixture.emit({ type: 'run.completed', ...about(run), run });
  await expect(
    page.getByText('Your day is planned.', { exact: true }),
  ).toHaveCount(1);
  await page.reload();
  await expect(
    page.getByText('Your day is planned.', { exact: true }),
  ).toHaveCount(1);
  await page.getByRole('button', { name: /Used 1 tool/ }).click();
  await expect(page.getByRole('button', { name: /read_file/ })).toBeVisible();
  expect(
    fixture.calls.filter(
      (c) => c.method === 'POST' && c.path.endsWith('/runs'),
    ),
  ).toHaveLength(1);
  fixture.assertClean();
});
