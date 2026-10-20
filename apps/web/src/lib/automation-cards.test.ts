import { describe, expect, it } from 'vitest';

import { automationFixture } from '../test/automations';
import {
  destinationChips,
  discordChannelLabel,
  findAutomationMessage,
  groupAutomations,
  reminderNeedsYou,
  reminderState,
} from './automation-cards';

const FIRED = 1_000_000;
const card = { firedAtMs: FIRED };

function reminder(overrides: Parameters<typeof automationFixture>[1] = {}) {
  return automationFixture('rem', {
    trigger: { type: 'once', atMs: FIRED },
    enabled: false,
    lastFiredAtMs: FIRED,
    ...overrides,
  });
}

describe('reminderState', () => {
  it('is due once fired, switched off, and not marked done', () => {
    const due = reminder();
    expect(reminderState(due, card)).toBe('due');
    expect(reminderNeedsYou(due)).toBe(true);
  });

  it('is snoozed while re-armed after having fired', () => {
    const snoozed = reminder({ enabled: true, nextDueAtMs: FIRED + 900_000 });
    expect(reminderState(snoozed, card)).toBe('snoozed');
    expect(reminderNeedsYou(snoozed)).toBe(false);
  });

  it('is done as soon as it has a done time, whatever else it says', () => {
    expect(reminderState(reminder({ doneAtMs: FIRED + 5 }), card)).toBe('done');
    expect(
      reminderState(reminder({ doneAtMs: FIRED + 5, enabled: true }), card),
    ).toBe('done');
    expect(reminderNeedsYou(reminder({ doneAtMs: FIRED + 5 }))).toBe(false);
  });

  it('is gone when the record no longer exists, with no actions to offer', () => {
    expect(reminderState(undefined, card)).toBe('gone');
  });

  it('is gone for a record that is not a fired one-time reminder', () => {
    expect(reminderState(automationFixture('every'), card)).toBe('gone');
    expect(reminderState(reminder({ lastFiredAtMs: null }), card)).toBe('gone');
  });

  it('hands the actions to the newest card when a snooze ran out', () => {
    const again = reminder({ lastFiredAtMs: FIRED + 15 * 60_000 });
    expect(reminderState(again, card)).toBe('earlier');
    expect(reminderState(again, { firedAtMs: FIRED + 15 * 60_000 })).toBe(
      'due',
    );
    // A fired time a few seconds off is still the same firing.
    expect(
      reminderState(reminder({ lastFiredAtMs: FIRED + 4_000 }), card),
    ).toBe('due');
  });
});

describe('groupAutomations', () => {
  const upcomingLater = automationFixture('later', {
    trigger: { type: 'once', atMs: 300 },
    nextDueAtMs: 300,
  });
  const upcomingSoon = automationFixture('soon', {
    trigger: { type: 'once', atMs: 100 },
    nextDueAtMs: 100,
  });
  const repeating = automationFixture('daily', {
    trigger: { type: 'daily', hour: 9, minute: 0, timeZone: 'UTC' },
  });
  const event = automationFixture('event', {
    trigger: { type: 'event', topic: 'files.changed' },
  });
  const waiting = reminder({ id: 'waiting' });
  const snoozed = reminder({
    id: 'snoozed',
    enabled: true,
    nextDueAtMs: FIRED + 1,
  });
  const doneEarly = reminder({ id: 'done-early', doneAtMs: 10 });
  const doneLate = reminder({ id: 'done-late', doneAtMs: 20 });
  const interrupted = automationFixture('interrupted', {
    enabled: false,
    lastOutcome: {
      status: 'error',
      occurredAtMs: 5,
      errorCode: 'schedule_run_interrupted',
    },
  });

  it('puts one-time, unfired, enabled automations in Upcoming, soonest first', () => {
    const groups = groupAutomations([upcomingLater, upcomingSoon]);
    expect(groups.upcoming.map((item) => item.id)).toEqual(['soon', 'later']);
  });

  it('puts everything that is not one-time in Repeating', () => {
    const groups = groupAutomations([repeating, event, upcomingSoon]);
    expect(groups.repeating.map((item) => item.id)).toEqual(['daily', 'event']);
  });

  it('puts one-time reminders that fired in Done, latest activity first, waiting and snoozed ones too', () => {
    const groups = groupAutomations([
      doneEarly,
      waiting,
      snoozed,
      doneLate,
      upcomingSoon,
    ]);
    // The two that fired at FIRED are the latest activity; the done ones
    // follow by when they were marked done.
    expect(groups.done.map((item) => item.id)).toEqual([
      'waiting',
      'snoozed',
      'done-late',
      'done-early',
    ]);
    expect(groups.upcoming.map((item) => item.id)).toEqual(['soon']);
  });

  it('keeps an interrupted run in Needs review wherever it would sit', () => {
    const groups = groupAutomations([repeating, interrupted]);
    expect(groups.needsReview.map((item) => item.id)).toEqual(['interrupted']);
    expect(groups.repeating.map((item) => item.id)).toEqual(['daily']);
  });
});

describe('destinationChips', () => {
  const context = {
    telegramConnectorId: 'tg-1',
    discordDestinations: [{ connectorId: 'dc-1', label: 'Nova · #home' }],
  };

  it('shows the main chat for the workspace target', () => {
    expect(
      destinationChips(automationFixture('a'), context).map((c) => c.kind),
    ).toEqual(['main']);
  });

  it('adds Telegram for the Telegram connector', () => {
    const telegram = automationFixture('b', {
      target: { type: 'connector', connectorId: 'tg-1' },
    });
    expect(destinationChips(telegram, context).map((c) => c.kind)).toEqual([
      'main',
      'telegram',
    ]);
  });

  it('names the Discord channel and notes the main chat', () => {
    const discord = automationFixture('c', {
      target: { type: 'connector', connectorId: 'dc-1' },
    });
    expect(destinationChips(discord, context)).toEqual([
      { kind: 'discord', label: '#home' },
      { kind: 'note' },
    ]);
  });

  it('keeps the main chat for a connector it cannot name', () => {
    const unknown = automationFixture('d', {
      target: { type: 'connector', connectorId: 'gone' },
    });
    expect(destinationChips(unknown, context).map((c) => c.kind)).toEqual([
      'main',
      'connector',
    ]);
  });

  it('reads the channel off a bot label', () => {
    expect(discordChannelLabel('Nova · #home')).toBe('#home');
    expect(discordChannelLabel('#home')).toBe('#home');
  });
});

describe('findAutomationMessage', () => {
  const run = (id: string, runId: string) => ({
    id,
    metadata: {
      kind: 'automation',
      automationId: 'a',
      runId,
      firedAtMs: 1,
    },
  });
  const messages = [
    run('m1', 'r1'),
    { id: 'm2', metadata: { kind: 'chat' } },
    run('m3', 'r2'),
  ];

  it('finds the message of a run', () => {
    expect(
      findAutomationMessage(messages, { automationId: 'a', runId: 'r1' }),
    ).toBe('m1');
  });

  it('finds the newest message of an automation without a run', () => {
    expect(findAutomationMessage(messages, { automationId: 'a' })).toBe('m3');
  });

  it('answers null when it is not among the loaded messages', () => {
    expect(
      findAutomationMessage(messages, { automationId: 'a', runId: 'r9' }),
    ).toBeNull();
    expect(findAutomationMessage([], { automationId: 'a' })).toBeNull();
  });
});
