import { chromium } from 'playwright-core';
import { access, mkdir, readdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { villageDefaults } from '../../tools/config.js';

const root = fileURLToPath(new URL('../..', import.meta.url));

const cache = resolve(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
let executablePath = chromium.executablePath();
if (
  !(await access(executablePath)
    .then(() => true)
    .catch(() => false))
) {
  const installed = (await readdir(cache))
    .filter((name) => /^chromium-\d+$/.test(name))
    .sort((a, b) => Number(b.split('-')[1]) - Number(a.split('-')[1]));
  executablePath = resolve(cache, installed[0], 'chrome-win64/chrome.exe');
}
const browser = await chromium.launch({ executablePath, headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
const errors = [];
const requests = [];
const appearanceRequests = [];
page.on('request', (request) => {
  if (request.url().endsWith('/traveler'))
    appearanceRequests.push(request.url());
});
page.on('pageerror', (error) => errors.push(error.message));
page.on('console', (entry) => {
  if (
    entry.type() === 'error' ||
    (entry.type() === 'warning' &&
      /THREE.*(?:deprecated|removed)/.test(entry.text()))
  )
    errors.push(entry.text());
});
const output = resolve(
  root,
  process.env.VILLAGE_TEST_OUTPUT ?? 'test-output/visual',
);
await mkdir(output, { recursive: true });
let view = {
  saveId: 'visual-fixture-ja',
  language: 'ja',
  questStatus: 'pending',
  traveler: null,
  npcId: 'tavi',
  phase: 'day',
  inventory: [],
  coins: 0,
  breadPreference: null,
  visits: 1,
  memories: [],
  journal: [],
  runtime: {
    mode: 'reviewed',
    engine: 'anima-harness',
    provider: 'visual-fixture',
    model: 'none',
  },
};
const saves = new Map([
  [view.saveId, view],
  [
    'visual-fixture-zh',
    { ...view, saveId: 'visual-fixture-zh', language: 'zh' },
  ],
]);
// Existing pre-MC saves can have no traveler and an unresolved creator request.
await page.addInitScript(() => {
  if (localStorage.getItem('village.e2e.seeded')) return;
  localStorage.setItem('village.e2e.seeded', 'true');
  localStorage.setItem('village.language', 'ja');
  localStorage.setItem('village.save.ja', 'visual-fixture-ja');
  localStorage.setItem(
    'village.appearance.visual-fixture-ja',
    JSON.stringify({
      id: 'old-appearance-request',
      traveler: {
        name: 'Old traveler',
        skinTone: 'deep',
        hairStyle: 'braid',
        hairColor: 'auburn',
        outfit: 'tunic',
        outfitColor: 'berry',
      },
    }),
  );
});
function dialogue() {
  const mira = view.npcId === 'mira';
  return {
    text: mira
      ? 'パンを二つ、ありがとう！村の夕ごはんを楽しみにしていてね。'
      : view.phase === 'complete'
        ? 'ミラに届けてくれて、ありがとう！はちみつのパンも用意したよ。'
        : 'いらっしゃい。焼きたてのパンはいかがですか？',
    translation: mira
      ? 'Thank you for both loaves! I hope you enjoy the village supper.'
      : view.phase === 'complete'
        ? 'Thank you for delivering the bread to Mira! I saved your favorite honey bread too.'
        : 'Welcome. Would you like some freshly baked bread?',
    reading: mira
      ? 'Pan o futatsu, arigatō!'
      : 'Irasshai. Yakit ate no pan wa ikaga desu ka?',
    expression: mira || view.phase === 'complete' ? 'happy' : 'warm',
    source: 'reviewed',
    suggestions: mira
      ? [
          {
            id: 'deliver_order',
            text: 'ミラさん、パンを二つ届けに来ました。',
            translation: 'Mira, I brought your two loaves.',
          },
          {
            id: 'ask_supper',
            text: '夕ごはんは何ですか。',
            translation: 'What’s for supper?',
          },
        ]
      : [
          {
            id: 'collect_order',
            text: 'ミラさんのパンを二つください。',
            translation: 'Two loaves for Mira, please.',
          },
          {
            id: 'ask_recipe',
            text: '新しいパンはどんな味ですか？',
            translation: 'What is the new bread like?',
          },
        ],
  };
}
await page.route('**/api/village/**', async (route) => {
  const request = route.request();
  const body = request.method() === 'POST' ? request.postDataJSON() : null;
  if (body) requests.push(body);
  const saveId = /\/saves\/([^/]+)/.exec(new URL(request.url()).pathname)?.[1];
  view = saves.get(saveId ?? `visual-fixture-${body.language}`);
  assert.ok(view, 'Every request must address its own language save');
  if (request.url().endsWith('/visit')) {
    view.npcId = body.npcId ?? 'tavi';
    view.visits++;
    if (view.npcId === 'tavi' && view.phase === 'evening')
      view.phase = 'complete';
  } else if (request.url().endsWith('/turns')) {
    view.npcId = body.npcId ?? 'tavi';
    if (body.choiceId === 'collect_order' && view.questStatus === 'pending') {
      view.questStatus = 'collected';
      view.coins = 20;
      view.inventory = [
        { id: 'mira_loaves', name: 'Mira’s two loaves', quantity: 2 },
      ];
    } else if (
      body.choiceId === 'deliver_order' &&
      view.npcId === 'mira' &&
      view.questStatus === 'collected'
    ) {
      view.questStatus = 'delivered';
      view.phase = 'evening';
      view.inventory = [];
    }
  }
  await route.fulfill({ json: { ...view, dialogue: dialogue() } });
});
async function talk(npc, walk) {
  await page.getByRole('button', { name: walk }).click();
  const button = page.getByRole('button', {
    name: 'Talk to ' + npc,
    exact: true,
  });
  await button.waitFor({ timeout: 15000 });
  await button.click();
  await page.getByRole('dialog', { name: npc, exact: true }).waitFor();
}
async function choose(text) {
  await page.getByRole('button', { name: text }).click();
  await page.getByRole('button', { name: 'Send reply' }).click();
}
async function shot(name) {
  await page.screenshot({ path: resolve(output, name) });
}
async function noCreator() {
  assert.equal(
    await page.getByRole('textbox', { name: 'Your name' }).count(),
    0,
  );
  assert.equal(
    await page
      .getByRole('button', { name: 'Edit traveler appearance' })
      .count(),
    0,
  );
  assert.equal(await page.locator('.character-creator').count(), 0);
}
try {
  await page.goto(
    process.env.VILLAGE_URL ?? `http://127.0.0.1:${villageDefaults.webPort}`,
  );
  await page.getByText('Reviewed dialogue · progress saved').waitFor();
  await noCreator();
  assert.equal(
    await page.evaluate(() =>
      localStorage.getItem('village.appearance.visual-fixture-ja'),
    ),
    null,
  );
  assert.equal(view.traveler, null);
  await shot('exploration.png');
  await talk('Tavi', /Walk to the shop/);
  await shot('conversation.png');
  await choose(/Two loaves for Mira/);
  await page
    .getByText('The bread is ready. Mira is waiting by the supper table.')
    .waitFor();
  await page.getByRole('button', { name: 'Leave conversation' }).click();
  assert.equal(await page.getByTitle('Mira’s bread order').innerText(), '2');
  await talk('Mira', /Walk to Mira/);
  await shot('mira-conversation.png');
  await choose(/I brought your two loaves/);
  await page
    .getByText(
      'The bread has arrived. Return to Tavi for an evening conversation.',
    )
    .waitFor();
  await page.getByRole('button', { name: 'Leave conversation' }).click();
  await page.getByText('Return to Tavi for an evening conversation.').waitFor();
  assert.equal(await page.getByTitle('Mira’s bread order').count(), 0);
  assert.equal(
    await page.getByTitle('An empty place for something good').innerText(),
    '·',
  );
  await shot('evening.png');
  await talk('Tavi', /^Return to Tavi/);
  await page
    .getByText('A place at the table, and time for another conversation.')
    .waitFor();
  await shot('evening-callback.png');
  await page.getByRole('button', { name: 'Leave conversation' }).click();
  await page
    .getByRole('heading', { name: 'A place at the table', exact: true })
    .waitFor();
  await page.reload();
  await page
    .getByRole('heading', { name: 'A place at the table', exact: true })
    .waitFor();
  await noCreator();
  assert.equal(view.phase, 'complete');
  assert.equal(view.coins, 20);
  assert.deepEqual(view.inventory, []);
  await shot('completed-village.png');
  await page.evaluate((saveId) => {
    const input = {
      requestId: 'interrupted-mira-turn',
      message: '夕ごはんは何ですか。',
      npcId: 'mira',
    };
    localStorage.setItem(
      'village.pending.' + saveId,
      JSON.stringify({
        id: input.requestId,
        kind: 'turn',
        npcId: 'mira',
        input,
      }),
    );
  }, view.saveId);
  await page.reload();
  await page.getByRole('region', { name: 'Conversation with Mira' }).waitFor();
  await page.getByRole('alert').filter({ hasText: 'interrupted' }).waitFor();
  await page.getByRole('button', { name: /Use prepared reply:/ }).click();
  await page.waitForFunction(
    (saveId) => localStorage.getItem('village.pending.' + saveId) === null,
    view.saveId,
  );
  assert.equal(requests.at(-1).recoveryOf, 'interrupted-mira-turn');
  assert.equal(requests.at(-1).npcId, 'mira');
  assert.equal(requests.at(-1).choiceId, 'ask_supper');
  assert.notEqual(requests.at(-1).requestId, 'interrupted-mira-turn');
  await shot('reviewed-mira-recovery.png');
  await page.getByRole('button', { name: 'Leave conversation' }).click();
  await page.getByRole('button', { name: /中文/ }).click();
  await page.waitForFunction(
    () => localStorage.getItem('village.save.zh') === 'visual-fixture-zh',
  );
  await page.getByText('Find Tavi at the village shop.').waitFor();
  assert.equal(view.language, 'zh');
  assert.equal(view.phase, 'day');
  assert.equal(view.questStatus, 'pending');
  assert.equal(view.traveler, null);
  await noCreator();
  await page.getByRole('button', { name: /日本語/ }).click();
  await page
    .getByRole('heading', { name: 'A place at the table', exact: true })
    .waitFor();
  assert.equal(view.saveId, 'visual-fixture-ja');
  assert.equal(view.questStatus, 'delivered');
  assert.equal(view.coins, 20);
  assert.deepEqual(view.inventory, []);
  await page.setViewportSize({ width: 390, height: 844 });
  await shot('mobile-village.png');
  await noCreator();
  assert.deepEqual(appearanceRequests, []);
  assert.deepEqual(errors, []);
  console.log(
    JSON.stringify({
      passed: true,
      output,
      phase: view.phase,
      traveler: view.traveler,
      errors,
    }),
  );
} catch (error) {
  await shot('failure.png');
  console.log(
    JSON.stringify({
      errors,
      body: (await page.locator('body').innerText()).slice(0, 2500),
    }),
  );
  throw error;
} finally {
  await browser.close();
}
