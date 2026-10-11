import { expect, test } from '@playwright/test';
import { installConsoleFixture } from './console/fixtures/api';
import { createState } from './console/fixtures/state';
import { about, send } from './console/fixtures/journeys';

for (const width of [1280, 390])
  test(`retained independent agent is not the companion at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
    const state = createState();
    state.agents[1].messages.push({
      id: 'independent_1',
      agentId: 'helper',
      roomId: 'peer:companion:helper',
      role: 'user',
      content: {
        text: 'Retained independent history',
        attachments: [],
        metadata: {},
      },
      createdAtMs: 1,
    });
    const fixture = await installConsoleFixture(page, state);
    await page.goto('/');
    await expect(page.getByPlaceholder('Message Anima…')).toBeVisible();
    await fixture.waitForStream();
    await expect(page.getByText('Retained independent history')).toHaveCount(0);
    const run = await send(page, fixture);
    expect(run.agentId).toBe('companion');
    fixture.finish(run, 'The companion answered.');
    await fixture.emit({ type: 'run.completed', ...about(run), run });
    await expect(
      page.getByText('The companion answered.', { exact: true }),
    ).toBeVisible();
    expect(state.agents[1].messages[0].content.text).toBe(
      'Retained independent history',
    );
    fixture.assertClean();
  });
