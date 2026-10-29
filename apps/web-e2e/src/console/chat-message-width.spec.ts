import { expect, test } from '@playwright/test';
import { installConsoleFixture } from './fixtures/api';
import { createState, session } from './fixtures/state';

test('assistant replies use the full conversation width', async ({ page }) => {
  const state = createState();
  const chat = { ...session('chat:width', 'Conversation'), mainChat: true, messageCount: 1 };
  state.sessions = [chat];
  state.messages[chat.id] = [{
    id: 'answer', role: 'assistant', text: 'A reply that can use the full conversation width.',
    attachments: [], metadata: {}, createdAtMs: 1,
  }];
  await installConsoleFixture(page, state, { chatSuite: {} });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  const reply = page.locator('.chat-turn.is-agent .scenic-agent-text');
  await expect(reply).toBeVisible();
  const widths = await reply.evaluate((element) => ({
    reply: element.getBoundingClientRect().width,
    turn: element.parentElement!.getBoundingClientRect().width,
  }));
  expect(widths.reply).toBeCloseTo(widths.turn, 0);
});

test('the plus menu offers attachment and stays aligned with the composer', async ({ page }) => {
  const state = createState();
  const chat = { ...session('chat:options', 'Conversation'), mainChat: true, messageCount: 0 };
  state.sessions = [chat];
  state.messages[chat.id] = [];
  await installConsoleFixture(page, state, { chatSuite: {} });
  await page.goto('/');
  const trigger = page.getByRole('button', { name: 'More input options' });
  await trigger.click();
  await expect(page.getByRole('button', { name: 'Attach files' })).toBeVisible();
  await expect(page.getByText('Spoken replies')).toHaveCount(0);
  await expect(page.getByText('Browser task')).toHaveCount(0);
  const menu = await page.locator('.composer-tray').boundingBox();
  const button = await trigger.boundingBox();
  expect(Math.abs(menu!.x - button!.x)).toBeLessThan(24);
  const chooser = page.waitForEvent('filechooser');
  await page.getByRole('button', { name: 'Attach files' }).click();
  await (await chooser).setFiles({ name: 'hello.txt', mimeType: 'text/plain', buffer: Buffer.from('hello') });
  await expect(page.getByRole('button', { name: 'Remove pixel.png' })).toBeVisible();
});
test('the chat continues behind a transparent navigation bar without a day label', async ({ page }) => {
  const state = createState();
  const chat = { ...session('chat:seamless', 'Conversation'), mainChat: true, messageCount: 1 };
  state.sessions = [chat];
  state.messages[chat.id] = [{
    id: 'answer', role: 'assistant', text: 'A conversation reply.',
    attachments: [], metadata: {}, createdAtMs: Date.now() - 86_400_000,
  }];
  await installConsoleFixture(page, state, { chatSuite: {} });
  await page.goto('/');
  await expect(page.getByText('A conversation reply.', { exact: true })).toBeVisible();
  const bar = page.locator('.scenic-topbar');
  await expect(bar.locator('.header-context-day')).toHaveCount(0);
  expect(await bar.evaluate((element) => getComputedStyle(element).backgroundColor)).toBe('rgba(0, 0, 0, 0)');
  expect(await page.locator('.studio-message-scroller').evaluate((element) => getComputedStyle(element).maskImage)).toBe('none');
});
