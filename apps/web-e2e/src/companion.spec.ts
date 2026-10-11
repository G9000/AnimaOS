import { expect, test } from '@playwright/test';
import { installConsoleFixture } from './console/fixtures/api';
import { createState } from './console/fixtures/state';
import { about, openConsole, send } from './console/fixtures/journeys';

for (const viewport of [
  { width: 1280, height: 900 },
  { width: 390, height: 844 },
]) {
  test(`companion session chat and settings at ${viewport.width}px`, async ({
    page,
  }) => {
    await page.setViewportSize(viewport);
    const fixture = await openConsole(page),
      run = await send(page, fixture);
    fixture.finish(run, 'What matters most today?');
    await fixture.emit({ type: 'run.completed', ...about(run), run });
    await expect(
      page.getByText('What matters most today?', { exact: true }),
    ).toBeVisible();
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await expect(page.getByRole('dialog')).toBeVisible();
    await page.getByRole('button', { name: 'Close settings' }).click();
    fixture.assertClean();
  });
}
test('first run creates one companion without a team wizard', async ({
  page,
}) => {
  const fixture = await installConsoleFixture(page, createState(true));
  await page.goto('/');
  await expect(
    page.getByRole('heading', { name: 'Set up your companion' }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Start chatting' }).click();
  await expect(page.getByPlaceholder('Message Anima…')).toBeVisible();
  expect(fixture.state.agents).toHaveLength(1);
  fixture.assertClean();
});
