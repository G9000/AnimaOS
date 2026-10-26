import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { VillageApp } from './App';
import * as presence from './presence';
import { VillageApiError, villageApi } from './api';
import type { GameView, Traveler } from './types';

vi.mock('./VillageScene', () => ({
  VillageScene: ({
    onNear,
    paused,
    traveler,
  }: {
    onNear: (near: 'tavi' | 'mira') => void;
    paused: boolean;
    traveler?: Traveler | null;
  }) => (
    <div
      data-testid="scene"
      data-paused={paused}
      data-traveler={traveler?.name}
    >
      <button onClick={() => onNear('tavi')}>Stand by counter</button>
      <button onClick={() => onNear('mira')}>Stand by Mira</button>
    </div>
  ),
}));
afterEach(() => {
  cleanup();
  localStorage.clear();
});

const savedView: GameView = {
  saveId: 'existing-save',
  language: 'ja',
  questStatus: 'pending',
  traveler: {
    name: 'Legacy traveler',
    skinTone: 'fair',
    hairStyle: 'short',
    hairColor: 'chestnut',
    outfit: 'cloak',
    outfitColor: 'moss',
  },
  npcId: 'tavi',
  phase: 'day',
  inventory: [],
  coins: 0,
  breadPreference: null,
  visits: 1,
  memories: [],
  journal: [],
  runtime: {
    mode: 'live',
    engine: 'anima-harness',
    provider: 'chatgpt',
    model: 'configured',
  },
  dialogue: {
    text: 'こんにちは。',
    translation: 'Hello.',
    reading: 'こんにちは',
    expression: 'warm',
    source: 'model',
    suggestions: [
      {
        id: 'collect_order',
        text: 'ミラさんのパンを二つください。',
        translation: 'Two loaves for Mira, please.',
      },
    ],
  },
};

it('names the contextual interaction button for the resident without decorative keys', async () => {
  const api = {
    create: vi.fn().mockResolvedValue(savedView),
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await screen.findByRole('button', { name: /Walk to the shop/ });
  fireEvent.click(screen.getByRole('button', { name: 'Stand by counter' }));
  expect(
    screen.getByRole('button', { name: 'Talk to Tavi', exact: true }),
  ).toBeEnabled();
  fireEvent.click(screen.getByRole('button', { name: 'Stand by Mira' }));
  expect(
    screen.getByRole('button', { name: 'Talk to Mira', exact: true }),
  ).toBeEnabled();
});

it('restores the exact pending request after reopening and retries its original identity', async () => {
  localStorage.setItem('village.save.ja', savedView.saveId);
  const input = {
    requestId: 'interrupted-request',
    message: 'ミラさんのパンを二つください。',
  };
  localStorage.setItem(
    `village.pending.${savedView.saveId}`,
    JSON.stringify({ id: input.requestId, kind: 'turn', input }),
  );
  const api = {
    create: vi.fn(),
    restore: vi.fn().mockResolvedValue(savedView),
    visit: vi.fn(),
    turn: vi.fn().mockResolvedValue(savedView),
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await waitFor(() =>
    expect(screen.getByRole('alert')).toHaveTextContent('interrupted'),
  );
  fireEvent.click(
    screen.getByRole('button', { name: 'Retry my last message' }),
  );
  await waitFor(() =>
    expect(api.turn).toHaveBeenCalledWith(savedView.saveId, input),
  );
  await waitFor(() =>
    expect(
      localStorage.getItem(`village.pending.${savedView.saveId}`),
    ).toBeNull(),
  );
  expect(api.visit).not.toHaveBeenCalled();
});

it('persists the pending payload before a failed network request for safe recovery', async () => {
  localStorage.setItem('village.save.ja', savedView.saveId);
  const api = {
    create: vi.fn(),
    restore: vi.fn().mockResolvedValue(savedView),
    visit: vi.fn().mockResolvedValue(savedView),
    turn: vi.fn().mockRejectedValue(new Error('Generation interrupted')),
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await waitFor(() =>
    expect(
      screen.getByRole('button', { name: /Walk to the shop/ }),
    ).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Stand by counter' }));
  fireEvent.click(screen.getByRole('button', { name: /Talk to Tavi/ }));
  const field = await screen.findByRole('textbox', { name: 'Your reply' });
  await waitFor(() => expect(field).toBeEnabled());
  fireEvent.change(field, { target: { value: 'ミラさんの注文です。' } });
  fireEvent.submit(field.closest('form')!);
  await waitFor(() =>
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Generation interrupted',
    ),
  );
  const pending = JSON.parse(
    localStorage.getItem(`village.pending.${savedView.saveId}`) ?? 'null',
  );
  expect(pending?.input.message).toBe('ミラさんの注文です。');
  expect(pending?.id).toBe(pending?.input.requestId);
  expect(field).toBeDisabled();
  fireEvent.change(field, { target: { value: '別の返事です。' } });
  fireEvent.submit(field.closest('form')!);
  expect(api.turn).toHaveBeenCalledTimes(1);
  expect(
    JSON.parse(
      localStorage.getItem(`village.pending.${savedView.saveId}`) ?? 'null',
    )?.id,
  ).toBe(pending.id);
});

it('keeps the original retry identity when a recovery request itself fails', async () => {
  localStorage.setItem('village.save.ja', savedView.saveId);
  const input = {
    requestId: 'original-request',
    message: 'ミラさんのパンを二つください。',
  };
  localStorage.setItem(
    `village.pending.${savedView.saveId}`,
    JSON.stringify({ id: input.requestId, kind: 'turn', input }),
  );
  const turn = vi
    .fn()
    .mockRejectedValueOnce(new Error('Recovery connection interrupted'))
    .mockResolvedValue(savedView);
  const api = {
    create: vi.fn(),
    restore: vi.fn().mockResolvedValue(savedView),
    visit: vi.fn(),
    turn,
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await screen.findByRole('alert');
  fireEvent.click(screen.getByRole('button', { name: /Use prepared reply/ }));
  await waitFor(() =>
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Recovery connection interrupted',
    ),
  );
  fireEvent.click(
    screen.getByRole('button', { name: 'Retry my last message' }),
  );
  await waitFor(() =>
    expect(turn).toHaveBeenLastCalledWith(savedView.saveId, input),
  );
});

it('restores the latest committed state after switching languages away and back', async () => {
  localStorage.setItem('village.save.ja', savedView.saveId);
  localStorage.setItem('village.save.zh', 'zh-save');
  const collected = {
    ...savedView,
    questStatus: 'collected' as const,
    breadPreference: 'honey' as const,
    coins: 20,
  };
  const chinese = {
    ...savedView,
    saveId: 'zh-save',
    language: 'zh' as const,
    traveler: null,
  };
  localStorage.setItem(
    'village.appearance.zh-save',
    JSON.stringify({ id: 'stale-zh-appearance', traveler: savedView.traveler }),
  );
  let updated = false;
  const restore = vi.fn(async (id: string) =>
    id === 'zh-save' ? chinese : updated ? collected : savedView,
  );
  const api = {
    create: vi.fn(),
    restore,
    visit: vi.fn().mockResolvedValue(savedView),
    turn: vi.fn(async () => {
      updated = true;
      return collected;
    }),
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await waitFor(() =>
    expect(
      screen.getByRole('button', { name: /Walk to the shop/ }),
    ).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Stand by counter' }));
  fireEvent.click(screen.getByRole('button', { name: /Talk to Tavi/ }));
  const field = await screen.findByRole('textbox', { name: 'Your reply' });
  await waitFor(() => expect(field).toBeEnabled());
  fireEvent.change(field, {
    target: { value: 'ミラさんのパンを二つください。' },
  });
  fireEvent.submit(field.closest('form')!);
  await screen.findByText(
    'The bread is ready. Mira is waiting by the supper table.',
  );
  fireEvent.click(screen.getByRole('button', { name: 'Leave conversation' }));
  fireEvent.click(screen.getByRole('button', { name: /中文/ }));
  await screen.findByText('Find Tavi at the village shop.');
  expect(screen.getByTestId('scene')).toHaveAttribute('data-paused', 'false');
  expect(localStorage.getItem('village.appearance.zh-save')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: /日本語/ }));
  await screen.findByText('Mira’s order is in your basket.');
  expect(
    restore.mock.calls.filter(([id]) => id === savedView.saveId),
  ).toHaveLength(2);
});

it('does not allow replies longer than the server character limit', async () => {
  localStorage.setItem('village.save.ja', savedView.saveId);
  const api = {
    create: vi.fn(),
    restore: vi.fn().mockResolvedValue(savedView),
    visit: vi.fn().mockResolvedValue(savedView),
    turn: vi.fn(),
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await waitFor(() =>
    expect(
      screen.getByRole('button', { name: /Walk to the shop/ }),
    ).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Stand by counter' }));
  fireEvent.click(screen.getByRole('button', { name: /Talk to Tavi/ }));
  const field = await screen.findByRole('textbox', { name: 'Your reply' });
  await waitFor(() => expect(field).toBeEnabled());
  expect(field).toHaveAttribute('maxlength', '1000');
  fireEvent.change(field, { target: { value: 'a'.repeat(1001) } });
  fireEvent.submit(field.closest('form')!);
  expect(api.turn).not.toHaveBeenCalled();
  expect(
    localStorage.getItem(`village.pending.${savedView.saveId}`),
  ).toBeNull();
});

it('releases a definitely rejected request so a corrected reply can be sent', async () => {
  localStorage.setItem('village.save.ja', savedView.saveId);
  const turn = vi
    .fn()
    .mockRejectedValueOnce(
      new VillageApiError(400, 'The reply was rejected before admission'),
    )
    .mockResolvedValue(savedView);
  const api = {
    create: vi.fn(),
    restore: vi.fn().mockResolvedValue(savedView),
    visit: vi.fn().mockResolvedValue(savedView),
    turn,
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await waitFor(() =>
    expect(
      screen.getByRole('button', { name: /Walk to the shop/ }),
    ).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Stand by counter' }));
  fireEvent.click(screen.getByRole('button', { name: /Talk to Tavi/ }));
  const field = await screen.findByRole('textbox', { name: 'Your reply' });
  await waitFor(() => expect(field).toBeEnabled());
  fireEvent.change(field, { target: { value: 'パンはありますか？' } });
  fireEvent.submit(field.closest('form')!);
  await screen.findByRole('alert');
  expect(field).toBeEnabled();
  expect(
    localStorage.getItem(`village.pending.${savedView.saveId}`),
  ).toBeNull();
  fireEvent.change(field, { target: { value: 'ミラさんの注文です。' } });
  fireEvent.submit(field.closest('form')!);
  await waitFor(() => expect(turn).toHaveBeenCalledTimes(2));
});

it('preserves a saved identity when the server is temporarily unavailable', async () => {
  localStorage.setItem('village.save.ja', 'existing-save');
  const api = {
    create: vi.fn(),
    restore: vi
      .fn()
      .mockRejectedValue(new Error('The game host is unavailable')),
    visit: vi.fn(),
    turn: vi.fn(),
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await waitFor(() =>
    expect(screen.getByRole('alert')).toHaveTextContent(
      'The game host is unavailable',
    ),
  );
  expect(localStorage.getItem('village.save.ja')).toBe('existing-save');
  expect(api.create).not.toHaveBeenCalled();
});

it('starts a fresh save without a traveler directly in exploration', async () => {
  const api = {
    create: vi.fn().mockResolvedValue({ ...savedView, traveler: null }),
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await waitFor(() =>
    expect(screen.getByTestId('scene')).toHaveAttribute('data-paused', 'false'),
  );
  expect(
    screen.getByRole('button', { name: /Walk to the shop/ }),
  ).toBeEnabled();
  expect(
    screen.queryByRole('textbox', { name: 'Your name' }),
  ).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: /中文/ })).toBeEnabled();
  fireEvent.click(screen.getByRole('button', { name: 'Open phrase journal' }));
  expect(screen.getByTestId('scene')).toHaveAttribute('data-paused', 'true');
  fireEvent.click(screen.getByRole('button', { name: 'Close phrase journal' }));
  expect(screen.getByTestId('scene')).toHaveAttribute('data-paused', 'false');
});

it('recovers an interrupted Mira turn with a legacy null traveler and preserves its speaker on retry', async () => {
  localStorage.setItem('village.save.ja', savedView.saveId);
  const input = {
    requestId: 'mira-pending',
    message: 'パンを持ってきました。',
    npcId: 'mira',
  };
  localStorage.setItem(
    `village.pending.${savedView.saveId}`,
    JSON.stringify({ id: input.requestId, kind: 'turn', npcId: 'mira', input }),
  );
  const api = {
    create: vi.fn(),
    restore: vi.fn().mockResolvedValue({ ...savedView, traveler: null }),
    visit: vi.fn(),
    turn: vi
      .fn()
      .mockResolvedValue({ ...savedView, traveler: null, npcId: 'mira' }),
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await screen.findByRole('region', { name: 'Conversation with Mira' });
  expect(
    screen.queryByRole('textbox', { name: 'Your name' }),
  ).not.toBeInTheDocument();
  fireEvent.click(
    screen.getByRole('button', { name: 'Retry my last message' }),
  );
  await waitFor(() =>
    expect(api.turn).toHaveBeenCalledWith(savedView.saveId, input),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Leave conversation' }));
  expect(
    await screen.findByRole('button', { name: /Walk to the shop/ }),
  ).toBeEnabled();
  expect(screen.getByTestId('scene')).toHaveAttribute('data-paused', 'false');
});

it('routes a visit and its reply to Mira then shows delivery and evening return objectives', async () => {
  const collected: GameView = {
    ...savedView,
    questStatus: 'collected',
    inventory: [{ id: 'mira_loaves', name: 'Mira’s bread', quantity: 2 }],
    coins: 20,
  };
  const mira = { ...collected, npcId: 'mira' as const };
  const delivered: GameView = {
    ...mira,
    questStatus: 'delivered',
    phase: 'evening',
    inventory: [],
  };
  const api = {
    create: vi.fn().mockResolvedValue(collected),
    restore: vi.fn(),
    visit: vi.fn().mockResolvedValue(mira),
    turn: vi.fn().mockResolvedValue(delivered),
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await screen.findByRole('button', { name: /Walk to Mira/ });
  expect(screen.getByTitle('Mira’s bread order')).toHaveTextContent('2');
  fireEvent.click(screen.getByRole('button', { name: 'Stand by Mira' }));
  fireEvent.click(screen.getByRole('button', { name: /Talk to Mira/ }));
  await waitFor(() =>
    expect(api.visit).toHaveBeenCalledWith(
      savedView.saveId,
      expect.any(String),
      'mira',
    ),
  );
  const field = await screen.findByRole('textbox', { name: 'Your reply' });
  await waitFor(() => expect(field).toBeEnabled());
  fireEvent.change(field, { target: { value: 'パンを持ってきました。' } });
  fireEvent.submit(field.closest('form')!);
  await waitFor(() =>
    expect(api.turn).toHaveBeenCalledWith(
      savedView.saveId,
      expect.objectContaining({ npcId: 'mira' }),
    ),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Leave conversation' }));
  await screen.findByText('Return to Tavi for an evening conversation.');
  expect(
    screen.getByTitle('An empty place for something good'),
  ).toHaveTextContent('·');
  expect(screen.getByText('The lanterns are lit')).toBeInTheDocument();
});

it('keeps saved chapter progress without an appearance editor or scene override', async () => {
  const complete: GameView = {
    ...savedView,
    questStatus: 'delivered',
    phase: 'complete',
    coins: 20,
  };
  const api = {
    create: vi.fn().mockResolvedValue(complete),
    restore: vi.fn(),
    visit: vi.fn(),
    turn: vi.fn(),
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await screen.findByText('A place at the table');
  expect(
    screen.queryByRole('button', { name: 'Edit traveler appearance' }),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole('textbox', { name: 'Your name' }),
  ).not.toBeInTheDocument();
  expect(screen.getByTestId('scene')).not.toHaveAttribute('data-traveler');
  expect(screen.getByTestId('scene')).toHaveAttribute('data-paused', 'false');
  expect(screen.queryByText('Legacy traveler')).not.toBeInTheDocument();
  expect(screen.getByText('20')).toBeInTheDocument();
  expect(
    screen.getByText(
      'Your village day is complete. Stay, explore, and talk a little longer.',
    ),
  ).toBeInTheDocument();
});

it('keeps legacy pending turns with Tavi even if the restored snapshot last showed Mira', async () => {
  localStorage.setItem('village.save.ja', savedView.saveId);
  const input = { requestId: 'legacy-request', message: 'こんにちは。' };
  localStorage.setItem(
    `village.pending.${savedView.saveId}`,
    JSON.stringify({ id: input.requestId, kind: 'turn', input }),
  );
  const api = {
    restore: vi.fn().mockResolvedValue({ ...savedView, npcId: 'mira' }),
    turn: vi.fn().mockResolvedValue(savedView),
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await screen.findByRole('region', { name: 'Conversation with Tavi' });
  fireEvent.click(
    screen.getByRole('button', { name: 'Retry my last message' }),
  );
  await waitFor(() =>
    expect(api.turn).toHaveBeenCalledWith(savedView.saveId, input),
  );
});

it.each([
  ['collected', 'day'],
  ['delivered', 'evening'],
  ['delivered', 'complete'],
] as const)(
  'offers a conversation recovery instead of another collection after %s in %s',
  async (questStatus, phase) => {
    localStorage.setItem('village.save.ja', savedView.saveId);
    localStorage.setItem(
      `village.pending.${savedView.saveId}`,
      JSON.stringify({ id: 'return-to-tavi', kind: 'visit', npcId: 'tavi' }),
    );
    const snapshot: GameView = {
      ...savedView,
      npcId: 'mira',
      questStatus,
      phase,
    };
    const api = {
      restore: vi.fn().mockResolvedValue(snapshot),
      turn: vi.fn().mockResolvedValue({ ...snapshot, npcId: 'tavi' }),
    } as unknown as typeof villageApi;
    render(<VillageApp api={api} />);
    await screen.findByRole('region', { name: 'Conversation with Tavi' });
    expect(
      screen.queryAllByRole('button', { name: /Two loaves for Mira/ }),
    ).toHaveLength(0);
    expect(
      screen.getByRole('button', {
        name: /Use prepared reply: I'm sure it'll be delicious/,
      }),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole('button', {
        name: 'Use prepared reply: How is the new recipe going?',
      }),
    );
    await waitFor(() =>
      expect(api.turn).toHaveBeenCalledWith(
        savedView.saveId,
        expect.objectContaining({
          npcId: 'tavi',
          recoveryOf: 'return-to-tavi',
          choiceId: 'ask_recipe',
        }),
      ),
    );
  },
);

it('allows exploration while preserving unfinished conversation recovery after leaving its panel', async () => {
  localStorage.setItem('village.save.ja', savedView.saveId);
  localStorage.setItem(
    `village.pending.${savedView.saveId}`,
    JSON.stringify({ id: 'old-visit', kind: 'visit', npcId: 'mira' }),
  );
  const api = {
    restore: vi.fn().mockResolvedValue({ ...savedView, traveler: null }),
    visit: vi.fn(),
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  await screen.findByRole('region', { name: 'Conversation with Mira' });
  fireEvent.click(screen.getByRole('button', { name: 'Leave conversation' }));
  expect(
    screen.queryByRole('textbox', { name: 'Your name' }),
  ).not.toBeInTheDocument();
  expect(screen.getByTestId('scene')).toHaveAttribute('data-paused', 'false');
  fireEvent.click(
    screen.getByRole('button', { name: 'Resume interrupted conversation' }),
  );
  expect(
    screen.getByRole('region', { name: 'Conversation with Mira' }),
  ).toBeInTheDocument();
});

it('discards stale appearance metadata without trapping a legacy save or resetting progress', async () => {
  localStorage.setItem('village.save.ja', savedView.saveId);
  localStorage.setItem(
    `village.appearance.${savedView.saveId}`,
    JSON.stringify({
      id: 'lost-appearance-request',
      traveler: savedView.traveler,
    }),
  );
  const legacy = {
    ...savedView,
    traveler: null,
    questStatus: 'collected' as const,
    coins: 20,
    inventory: [{ id: 'mira_loaves', name: 'Mira’s bread', quantity: 2 }],
  };
  const api = {
    restore: vi.fn().mockResolvedValue(legacy),
    visit: vi.fn().mockResolvedValue({ ...legacy, npcId: 'mira' }),
  } as unknown as typeof villageApi;
  render(<VillageApp api={api} />);
  expect(
    await screen.findByRole('button', { name: /Walk to Mira/ }),
  ).toBeEnabled();
  expect(screen.getByTestId('scene')).toHaveAttribute('data-paused', 'false');
  expect(screen.getByTitle('Mira’s bread order')).toHaveTextContent('2');
  expect(
    localStorage.getItem(`village.appearance.${savedView.saveId}`),
  ).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Stand by Mira' }));
  fireEvent.click(screen.getByRole('button', { name: 'Talk to Mira' }));
  await screen.findByRole('region', { name: 'Conversation with Mira' });
  expect(api.visit).toHaveBeenCalledWith(
    savedView.saveId,
    expect.any(String),
    'mira',
  );
});


it('shows the resident activity terminal reason alongside retained supper replies', async () => {
  const stop = vi.fn();
  const connection = vi.spyOn(presence, 'connectVillagePresence').mockImplementation((_save, update) => {
    update({ interactionId: 'supper-1', sourceEventId: 'delivery', complete: false, status: 'budget_exhausted', terminalReason: 'The supper conversation reached its time limit.', replies: [{eventId:'tavi-reply',npcId:'tavi',text:'パンです。',translation:'Here is the bread.',reading:'Pan desu.'}] });
    return stop;
  });
  const create = vi.spyOn(villageApi, 'create').mockResolvedValue(savedView);
  try {
    render(<VillageApp />);
    fireEvent.click(await screen.findByRole('button', { name: 'Open phrase journal' }));
    expect(await screen.findByText('The supper conversation reached its time limit.')).toBeVisible();
    expect(screen.getByText('Here is the bread.')).toBeVisible();
  } finally {
    connection.mockRestore();
    create.mockRestore();
  }
});
