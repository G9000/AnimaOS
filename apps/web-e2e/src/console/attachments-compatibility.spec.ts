import { test, expect } from '@playwright/test';
import { openConsole } from './fixtures/journeys';
import { installConsoleFixture } from './fixtures/api';

test('attachment-only send retains upload IDs and owner Blob thumbnail request', async ({
  page,
}) => {
  const fixture = await openConsole(page);
  await page.getByLabel('Choose attachments').setInputFiles({
    name: 'pixel.png',
    mimeType: 'image/png',
    buffer: Buffer.from(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a9uoAAAAASUVORK5CYII=',
      'base64',
    ),
  });
  await expect(
    page.getByRole('button', { name: 'Send', exact: true }),
  ).toBeEnabled();
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(() => fixture.state.runs.length).toBe(1);
  expect(fixture.state.runs[0].input).toMatchObject({
    text: '',
    attachmentIds: ['att_1'],
  });
  await expect
    .poll(() =>
      fixture.calls.some(
        (c) =>
          c.method === 'GET' &&
          c.path === '/api/agents/companion/attachments/att_1',
      ),
    )
    .toBe(true);
  fixture.assertClean();
});
test('status 404 alone shows Update the daemon and retains a draft on reload', async ({
  page,
}) => {
  const fixture = await installConsoleFixture(page);
  fixture.setRouteFailure('GET', '/api/status', 404);
  await page.addInitScript(() =>
    sessionStorage.setItem('animaos.draft.companion/home', 'A saved draft'),
  );
  await page.goto('/');
  await expect(
    page.getByText('Update the daemon', { exact: false }).first(),
  ).toBeVisible();
  await expect(page.getByPlaceholder('Message Anima…')).toHaveValue(
    'A saved draft',
  );
  await expect(
    page.getByRole('button', { name: 'Send', exact: true }),
  ).toBeDisabled();
  fixture.setRouteFailure('GET', '/api/status', null);
  await page.reload();
  await expect(page.getByPlaceholder('Message Anima…')).toHaveValue(
    'A saved draft',
  );
  await expect(
    page.getByRole('button', { name: 'Send', exact: true }),
  ).toBeEnabled();
  fixture.assertClean();
});
