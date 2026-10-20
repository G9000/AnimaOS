import { describe, expect, it, vi } from 'vitest';
import type { AutomationMessageMeta } from '@animaOS-SWARM/sdk';

import {
  MAX_TOASTS,
  TOAST_AUTO_DISMISS_MS,
  autoDismisses,
  dismissToast,
  makeToast,
  notifyToast,
  pushToast,
  shouldToast,
  toastKind,
  toastText,
} from './automation-toasts';

function card(
  overrides: Partial<
    Extract<AutomationMessageMeta, { kind: 'automation' }>
  > = {},
): AutomationMessageMeta {
  return {
    kind: 'automation',
    automationId: 'a',
    automationName: 'Call the bank',
    reminder: true,
    body: 'Their cut-off is 3:00 PM.',
    delivery: 'main',
    runId: 'run',
    firedAtMs: Date.UTC(2026, 9, 21, 14, 0),
    ...overrides,
  };
}
const note: AutomationMessageMeta = {
  kind: 'automationNote',
  automationId: 'n',
  automationName: 'Weekly review',
  body: 'text',
  delivery: 'discord',
  destinationLabel: '#home',
  runId: 'r',
  firedAtMs: 0,
};

describe('toast kinds', () => {
  it('stays for a reminder and leaves by itself for a report or a note', () => {
    expect(toastKind(card())).toBe('reminder');
    expect(toastKind(card({ reminder: false }))).toBe('report');
    expect(toastKind(note)).toBe('note');
    expect(TOAST_AUTO_DISMISS_MS).toBe(8_000);
    expect(autoDismisses(makeToast('1', card()))).toBe(false);
    expect(autoDismisses(makeToast('2', card({ reminder: false })))).toBe(true);
    expect(autoDismisses(makeToast('3', note))).toBe(true);
  });
});

describe('the stack', () => {
  const quiet = (id: string) => makeToast(id, card({ reminder: false }));
  const reminder = (id: string) => makeToast(id, card());

  it('puts the newest on top and shows each announcement once', () => {
    let stack = pushToast([], quiet('1'));
    stack = pushToast(stack, quiet('2'));
    stack = pushToast(stack, quiet('1'));
    expect(stack.map((toast) => toast.id)).toEqual(['2', '1']);
  });

  it('holds three at most, dropping the oldest', () => {
    let stack = [] as ReturnType<typeof pushToast>;
    for (const id of ['1', '2', '3', '4']) stack = pushToast(stack, quiet(id));
    expect(MAX_TOASTS).toBe(3);
    expect(stack.map((toast) => toast.id)).toEqual(['4', '3', '2']);
  });

  it('pushes a quiet toast out before a reminder that waits for the owner', () => {
    let stack = pushToast([], reminder('r'));
    for (const id of ['1', '2', '3']) stack = pushToast(stack, quiet(id));
    expect(stack.map((toast) => toast.id)).toEqual(['3', '2', 'r']);
  });

  it('drops the oldest reminder only when every toast is a reminder', () => {
    let stack = [] as ReturnType<typeof pushToast>;
    for (const id of ['r1', 'r2', 'r3', 'r4'])
      stack = pushToast(stack, reminder(id));
    expect(stack.map((toast) => toast.id)).toEqual(['r4', 'r3', 'r2']);
  });

  it('removes a dismissed toast', () => {
    const stack = pushToast(pushToast([], quiet('1')), quiet('2'));
    expect(dismissToast(stack, '2').map((toast) => toast.id)).toEqual(['1']);
  });
});

describe('shouldToast', () => {
  const looking = {
    viewingMainChat: true,
    atBottom: true,
    documentHidden: false,
  };

  it('stays quiet while the owner looks at the bottom of the main chat', () => {
    expect(shouldToast(looking)).toBe(false);
  });

  it('toasts on another page or side thread, scrolled up, or in the background', () => {
    expect(shouldToast({ ...looking, viewingMainChat: false })).toBe(true);
    expect(shouldToast({ ...looking, atBottom: false })).toBe(true);
    expect(shouldToast({ ...looking, documentHidden: true })).toBe(true);
  });
});

describe('toastText', () => {
  it('words a reminder with its time and body', () => {
    const text = toastText(makeToast('1', card()));
    expect(text.title).toBe('Call the bank');
    expect(text.detail).toBe('Their cut-off is 3:00 PM.');
    expect(text.label).toMatch(/^Reminder · /);
    expect(text.action).toBe('Main chat');
  });

  it('words a report as posted in the main chat', () => {
    const text = toastText(
      makeToast('2', card({ reminder: false, automationName: 'Inbox digest' })),
    );
    expect(text.title).toBe('Inbox digest posted');
    expect(text.detail).toMatch(/^In main chat · /);
    expect(text.action).toBe('Show');
  });

  it('words a Discord note, opening the thread only when there is one', () => {
    const text = toastText(makeToast('3', note));
    expect(text.title).toBe('Weekly review sent to Discord');
    expect(text.detail).toMatch(/^#home · /);
    expect(text.action).toBe('Show');
    expect(
      toastText(makeToast('4', { ...note, destinationSessionId: 'discord:1' }))
        .action,
    ).toBe('Open');
  });
});

describe('notifyToast', () => {
  const toast = makeToast('m1', card());

  it('shows a system notification only in the background with permission', () => {
    const create = vi.fn();
    expect(
      notifyToast(toast, { hidden: true, permission: 'granted', create }),
    ).toBe(true);
    expect(create).toHaveBeenCalledWith('Call the bank', {
      body: 'Their cut-off is 3:00 PM.',
      tag: 'm1',
    });
  });

  it('never notifies a visible tab, a denied or undecided permission, or a missing API', () => {
    const create = vi.fn();
    expect(
      notifyToast(toast, { hidden: false, permission: 'granted', create }),
    ).toBe(false);
    expect(
      notifyToast(toast, { hidden: true, permission: 'denied', create }),
    ).toBe(false);
    expect(
      notifyToast(toast, { hidden: true, permission: 'default', create }),
    ).toBe(false);
    expect(
      notifyToast(toast, { hidden: true, permission: undefined, create }),
    ).toBe(false);
    expect(create).not.toHaveBeenCalled();
  });

  it('survives a browser that refuses to construct it', () => {
    const create = vi.fn(() => {
      throw new Error('Illegal constructor');
    });
    expect(
      notifyToast(toast, { hidden: true, permission: 'granted', create }),
    ).toBe(false);
  });
});
