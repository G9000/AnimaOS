import { describe, expect, it } from 'vitest';

import { automationMessageMeta } from './index.js';

const card = {
  kind: 'automation',
  automationId: 'schedule-1',
  automationName: 'Call the bank',
  reminder: true,
  body: 'Their cut-off is 3:00 PM.',
  delivery: 'telegram',
  runId: 'run_1',
  firedAtMs: 1_000,
};
const note = {
  kind: 'automationNote',
  automationId: 'schedule-2',
  automationName: 'Weekly review',
  body: 'The full review',
  delivery: 'discord',
  destinationLabel: '#home',
  destinationSessionId: 'discord:1',
  destinationMessageId: 'msg-9',
  runId: 'run_2',
  firedAtMs: 2_000,
};

describe('automationMessageMeta', () => {
  it('reads a card from a session message or a chat message', () => {
    const expected = { ...card };
    expect(automationMessageMeta({ metadata: card })).toEqual(expected);
    expect(automationMessageMeta({ content: { metadata: card } })).toEqual(
      expected,
    );
  });

  it('reads a Discord note with its destination', () => {
    expect(automationMessageMeta({ metadata: note })).toEqual(note);
  });

  it('omits an absent destination session and message', () => {
    const { destinationSessionId, destinationMessageId, ...bare } = note;
    void destinationSessionId;
    void destinationMessageId;
    const parsed = automationMessageMeta({ metadata: bare });
    expect(parsed).toEqual(bare);
    expect(parsed).not.toHaveProperty('destinationSessionId');
  });

  it('treats other delivery values on a card as the main chat', () => {
    expect(
      automationMessageMeta({ metadata: { ...card, delivery: 'carrier-pigeon' } }),
    ).toMatchObject({ delivery: 'main' });
    expect(
      automationMessageMeta({ metadata: { ...card, reminder: 'yes' } }),
    ).toMatchObject({ reminder: false });
  });

  it('returns null for other messages and malformed metadata', () => {
    expect(automationMessageMeta({})).toBeNull();
    expect(automationMessageMeta({ metadata: null })).toBeNull();
    expect(automationMessageMeta({ metadata: { kind: 'chat' } })).toBeNull();
    expect(
      automationMessageMeta({ metadata: { ...card, runId: undefined } }),
    ).toBeNull();
    expect(
      automationMessageMeta({ metadata: { ...card, automationId: '' } }),
    ).toBeNull();
    expect(
      automationMessageMeta({ metadata: { ...card, firedAtMs: 'soon' } }),
    ).toBeNull();
    expect(
      automationMessageMeta({ metadata: { ...note, delivery: 'telegram' } }),
    ).toBeNull();
    expect(automationMessageMeta({ metadata: [card] })).toBeNull();
    // The copy of a Discord post in its own channel is a plain message.
    expect(
      automationMessageMeta({
        metadata: {
          kind: 'automationPost',
          automationId: 'schedule-2',
          automationName: 'Weekly review',
          runId: 'run_2',
          firedAtMs: 2_000,
        },
      }),
    ).toBeNull();
  });
});
