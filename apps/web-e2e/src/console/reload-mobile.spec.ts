import { test, expect } from '@playwright/test';
import { openConsole, send, about } from './fixtures/journeys';

test('reload and reconnect rejoin retained run without duplicate sends or deltas', async ({
  page,
}) => {
  const fixture = await openConsole(page),
    run = await send(page, fixture);
  const stepId = `${run.id}:1`;
  fixture.setSnapshot([
    { run, stepId, text: 'Hello 🌏', textOffset: 0, tools: [] },
  ]);
  await fixture.emit({
    type: 'step.delta',
    ...about(run),
    stepId,
    offset: 0,
    text: 'Hello 🌏',
  });
  await expect(page.getByText('Hello 🌏', { exact: true })).toBeVisible();
  const key = run.idempotencyKey;
  await page.reload();
  await expect(page.getByText('Hello 🌏', { exact: true })).toBeVisible();
  await expect.poll(fixture.connections).toBe(2);
  await fixture.disconnectStream();
  await expect(
    page.getByText('Reconnecting', { exact: false }).first(),
  ).toBeVisible();
  await expect.poll(fixture.connections).toBe(3);
  await fixture.waitForStream();
  await fixture.emit({
    type: 'step.delta',
    ...about(run),
    stepId,
    offset: 'Hello 🌏'.length,
    text: ' again',
  });
  await expect(page.getByText('Hello 🌏 again', { exact: true })).toBeVisible();
  fixture.finish(run, 'Final answer once.');
  await fixture.emit({ type: 'run.completed', ...about(run), run });
  await expect(
    page.getByText('Final answer once.', { exact: true }),
  ).toHaveCount(1);
  const sends = fixture.calls.filter(
    (c) => c.method === 'POST' && c.path.endsWith('/runs'),
  );
  expect(sends).toHaveLength(1);
  expect(sends[0].key).toBe(key);
  fixture.assertClean();
});
test('mobile sessions drawer selects a retained session and preserves home draft', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const fixture = await openConsole(page);
  await page.getByPlaceholder('Message Anima…').fill('Keep my draft');
  await page
    .getByRole('button', { name: 'Open sessions', exact: true })
    .click();
  await expect(
    page.getByRole('button', { name: 'Close sessions' }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Close sessions' }).click();
  await expect(page.getByPlaceholder('Message Anima…')).toHaveValue(
    'Keep my draft',
  );
  const run = await send(page, fixture, 'Mobile message');
  fixture.finish(run, 'Mobile response');
  await fixture.emit({ type: 'run.completed', ...about(run), run });
  await expect(
    page.getByText('Mobile response', { exact: true }),
  ).toBeVisible();
  await page
    .getByRole('button', { name: 'Open sessions', exact: true })
    .click();
  await page.getByRole('button', { name: 'New chat', exact: true }).click();
  await page.getByPlaceholder('Message Anima…').fill('Another draft');
  await page
    .getByRole('button', { name: 'Open sessions', exact: true })
    .click();
  await page
    .getByRole('button', { name: 'Mobile message', exact: true })
    .click();
  await expect(
    page.getByText('Mobile response', { exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  fixture.assertClean();
});
