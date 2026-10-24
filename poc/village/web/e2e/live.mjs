import { chromium } from 'playwright-core';
import { access, mkdir, readdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { villageDefaults } from '../../tools/config.js';

const root = fileURLToPath(new URL('../..', import.meta.url));

let executablePath = chromium.executablePath();
if (
  !(await access(executablePath)
    .then(() => true)
    .catch(() => false))
) {
  const cache = resolve(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  const installed = (await readdir(cache))
    .filter((name) => /^chromium-\d+$/.test(name))
    .sort((a, b) => Number(b.split('-')[1]) - Number(a.split('-')[1]));
  executablePath = resolve(cache, installed[0], 'chrome-win64/chrome.exe');
}
const browser = await chromium.launch({ executablePath, headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
page.setDefaultTimeout(20000);
const output = resolve(
  root,
  process.env.VILLAGE_TEST_OUTPUT ?? 'test-output/live',
);
await mkdir(output, { recursive: true });
const errors = [];
const evidence = [];
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
const responseFor = (suffix) =>
  page.waitForResponse(
    (response) =>
      response.url().endsWith(suffix) && response.request().method() === 'POST',
    { timeout: 120000 },
  );
async function result(response, step, expectedStatus = 200) {
  const data = await response.json();
  assert.equal(response.status(), expectedStatus, JSON.stringify(data));
  evidence.push({
    step,
    npcId: data.npcId,
    phase: data.phase,
    source: data.dialogue.source,
    questStatus: data.questStatus,
    preference: data.breadPreference,
    text: data.dialogue.text,
  });
  return data;
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
async function walkAndTalk(npc = 'Tavi', walk = /Walk to the shop/) {
  await page.getByRole('button', { name: walk }).click();
  const reply = responseFor('/visit');
  await page
    .getByRole('button', { name: 'Talk to ' + npc, exact: true })
    .click();
  const data = await result(await reply, 'visit-' + npc);
  assert.equal(data.npcId, npc.toLowerCase());
  await page.getByRole('dialog', { name: npc, exact: true }).waitFor();
  return data;
}
async function say(message) {
  await page.getByRole('textbox', { name: 'Your reply' }).fill(message);
  const reply = responseFor('/turns');
  await page.getByRole('button', { name: 'Send reply' }).click();
  return result(await reply, 'turn');
}
async function leave() {
  await page.getByRole('button', { name: 'Leave conversation' }).click();
}
async function restore() {
  const reply = page.waitForResponse(
    (response) =>
      /\/saves\/[^/]+$/.test(new URL(response.url()).pathname) &&
      response.request().method() === 'GET',
  );
  await page.reload();
  return (await reply).json();
}
try {
  const japaneseCreated = responseFor('/saves');
  await page.goto(
    process.env.VILLAGE_URL ?? `http://127.0.0.1:${villageDefaults.webPort}`,
  );
  const fresh = await result(await japaneseCreated, 'create-ja', 201);
  assert.equal(fresh.language, 'ja');
  assert.equal(fresh.traveler, null);
  assert.equal(fresh.questStatus, 'pending');
  await noCreator();
  const first = await walkAndTalk();
  assert.equal(first.traveler, null);
  assert.equal(first.runtime.mode, 'live');
  assert.equal(first.runtime.engine, 'anima-harness');
  assert.equal(first.dialogue.source, 'model');
  const collected = await say('ミラさんのパンを二つ、受け取りに来ました。');
  assert.equal(collected.questStatus, 'collected');
  assert.equal(
    collected.inventory.find((item) => item.id === 'mira_loaves')?.quantity,
    2,
  );
  assert.equal(collected.coins, 20);
  const preferred = await say('はちみつパンが好きです。');
  assert.equal(preferred.breadPreference, 'honey');
  await page.screenshot({ path: resolve(output, 'live-tavi.png') });
  await leave();
  const persisted = await restore();
  assert.equal(persisted.saveId, first.saveId);
  assert.equal(persisted.questStatus, 'collected');
  assert.equal(persisted.traveler, null);
  assert.equal(persisted.coins, 20);
  assert.equal(
    persisted.inventory.find((item) => item.id === 'mira_loaves')?.quantity,
    2,
  );
  await noCreator();
  const mira = await walkAndTalk('Mira', /Walk to Mira/);
  assert.equal(mira.dialogue.source, 'model');
  assert.ok(
    !mira.memories.some((memory) => /honey|はちみつ|蜂蜜/i.test(memory)),
    'Mira must not inherit Tavi’s private preference memory',
  );
  await page.screenshot({ path: resolve(output, 'live-mira.png') });
  const delivered = await say('ミラさん、パンを二つ届けに来ました。');
  assert.equal(delivered.questStatus, 'delivered');
  assert.equal(delivered.phase, 'evening');
  assert.equal(delivered.inventory.length, 0);
  assert.equal(delivered.coins, 20);
  await leave();
  await page.getByText('Return to Tavi for an evening conversation.').waitFor();
  await page.screenshot({ path: resolve(output, 'live-evening.png') });
  const callback = await walkAndTalk('Tavi', /^Return to Tavi/);
  assert.equal(callback.phase, 'complete');
  assert.equal(callback.dialogue.source, 'reviewed');
  assert.match(callback.dialogue.translation, /honey/i);
  await page.screenshot({ path: resolve(output, 'live-callback.png') });
  await leave();
  const complete = await restore();
  assert.equal(complete.phase, 'complete');
  assert.equal(complete.traveler, null);
  assert.equal(complete.questStatus, 'delivered');
  assert.equal(complete.coins, 20);
  assert.deepEqual(complete.inventory, []);
  await noCreator();
  const chineseCreated = responseFor('/saves');
  await page.getByRole('button', { name: /中文/ }).click();
  const chinese = await result(await chineseCreated, 'create-zh', 201);
  assert.notEqual(chinese.saveId, first.saveId);
  assert.equal(chinese.language, 'zh');
  assert.equal(chinese.traveler, null);
  assert.equal(chinese.phase, 'day');
  assert.equal(chinese.questStatus, 'pending');
  await page.getByText('Find Tavi at the village shop.').waitFor();
  await noCreator();
  const chineseVisit = await walkAndTalk();
  assert.equal(chineseVisit.runtime.mode, 'live');
  assert.equal(chineseVisit.runtime.engine, 'anima-harness');
  assert.equal(chineseVisit.dialogue.source, 'model');
  assert.equal(chineseVisit.language, 'zh');
  await leave();
  const japaneseRestored = page.waitForResponse(
    (response) =>
      response.url().endsWith('/saves/' + first.saveId) &&
      response.request().method() === 'GET',
  );
  await page.getByRole('button', { name: /日本語/ }).click();
  const japanese = await (await japaneseRestored).json();
  assert.equal(japanese.phase, 'complete');
  assert.equal(japanese.questStatus, 'delivered');
  assert.equal(japanese.breadPreference, 'honey');
  assert.equal(japanese.traveler, null);
  assert.equal(japanese.coins, 20);
  assert.deepEqual(japanese.inventory, []);
  await page
    .getByText(
      'Your village day is complete. Stay, explore, and talk a little longer.',
    )
    .waitFor();
  await page.screenshot({ path: resolve(output, 'restored-village.png') });
  await noCreator();
  assert.deepEqual(appearanceRequests, []);
  assert.deepEqual(errors, []);
  console.log(
    JSON.stringify(
      {
        passed: true,
        saveId: first.saveId,
        evidence,
        restored: {
          phase: japanese.phase,
          questStatus: japanese.questStatus,
          traveler: japanese.traveler,
        },
        errors,
      },
      null,
      2,
    ),
  );
} catch (error) {
  await page.screenshot({ path: resolve(output, 'failure.png') });
  console.log(
    JSON.stringify(
      {
        evidence,
        errors,
        body: (await page.locator('body').innerText()).slice(-2500),
      },
      null,
      2,
    ),
  );
  throw error;
} finally {
  await browser.close();
}
