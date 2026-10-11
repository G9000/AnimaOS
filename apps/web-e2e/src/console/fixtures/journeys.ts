import { expect, type Page } from '@playwright/test';
import type { Run } from '@animaOS-SWARM/sdk';
import { installConsoleFixture } from './api';
export async function openConsole(page: Page) {
  const fixture = await installConsoleFixture(page);
  await page.goto('/');
  await expect(page.getByPlaceholder('Message Anima…')).toBeVisible();
  await fixture.waitForStream();
  return fixture;
}
export async function send(
  page: Page,
  fixture: Awaited<ReturnType<typeof installConsoleFixture>>,
  text = 'Plan my day',
) {
  await page.getByPlaceholder('Message Anima…').fill(text);
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(() => fixture.state.runs.length).toBe(1);
  const run = fixture.state.runs[0];
  await expect(page.getByText('Queued', { exact: true })).toBeVisible();
  fixture.start(run);
  await fixture.emit({ type: 'run.started', ...about(run), run });
  return run;
}
export function about(run: Run) {
  return {
    agentId: run.agentId,
    sessionId: run.sessionId,
    runId: run.id,
    seq: 0,
    at: 12,
  };
}
