import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DaemonHttpError } from '@animaOS-SWARM/sdk';

import { COMPANION_UNREACHABLE } from '../lib/approvals';
import { daemon } from '../lib/daemon-api';
import {
  DELETE_ENTITY_PROMPT,
  DELETE_FACT_PROMPT,
  DELETE_MEMORY_PROMPT,
  ENTITIES_EMPTY,
  FACTS_EMPTY,
  FACT_EDIT_HINT,
  MEMORY_EMPTY,
  MEMORY_GONE,
  MEMORY_SEARCH_EMPTY,
  REPLACED_LABEL,
} from '../lib/memory';
import {
  entityFixture,
  factFixture,
  memoryFixture,
  relationshipFixture,
} from '../test/memory';
import { MemoryPage } from './MemoryPage';

const ZWSP = String.fromCodePoint(0x200b);
const HIDDEN_MARKER = '⟨U+200B⟩';
// A tag character is one the daemon refuses; a zero-width space is not.
const TAG_A = String.fromCodePoint(0xe0041);
const TAG_MARKER = '⟨U+E0041⟩';
const HIDDEN_REFUSAL =
  'Memory text must not contain invisible tag or direction-override characters';

function renderPage(props: { online?: boolean } = {}) {
  return render(
    <MemoryPage agentId="agent-main" online={props.online ?? true} epoch={1} />,
  );
}

const deleted = {
  id: 'm1',
  removedRelationships: 0,
  updatedRelationships: 0,
  updatedFacts: 0,
};

beforeEach(() => {
  vi.spyOn(daemon, 'recentMemories').mockResolvedValue([]);
  vi.spyOn(daemon, 'searchMemories').mockResolvedValue([]);
  vi.spyOn(daemon, 'listFacts').mockResolvedValue([]);
  vi.spyOn(daemon, 'getMemoryViews').mockResolvedValue({
    version: 0,
    viewDigest: 'empty',
    userProfile: [],
    agentNotes: [],
    unavailable: [],
  });
  vi.spyOn(daemon, 'memoryViewCandidates').mockResolvedValue({
    candidates: [],
    hasMore: false,
  });
  vi.spyOn(daemon, 'selectMemoryViews').mockResolvedValue({
    version: 0,
    viewDigest: 'empty',
    userProfile: [],
    agentNotes: [],
    unavailable: [],
  });
  vi.spyOn(daemon, 'correctMemoryViewSource').mockResolvedValue({
    version: 0,
    viewDigest: 'empty',
    userProfile: [],
    agentNotes: [],
    unavailable: [],
  });
  vi.spyOn(daemon, 'listMemoryEntities').mockResolvedValue([]);
  vi.spyOn(daemon, 'listMemoryRelationships').mockResolvedValue([]);
  vi.spyOn(daemon, 'updateMemory').mockResolvedValue(memoryFixture('m1'));
  vi.spyOn(daemon, 'deleteMemory').mockResolvedValue(deleted);
  vi.spyOn(daemon, 'replaceFact').mockResolvedValue({
    fact: factFixture('f2'),
    superseded: factFixture('f1', { status: 'superseded' }),
  });
  vi.spyOn(daemon, 'deleteFact').mockResolvedValue({ id: 'f1' });
  vi.spyOn(daemon, 'deleteMemoryEntity').mockResolvedValue({
    kind: 'external',
    id: 'e1',
    removedRelationships: 0,
    removedFacts: 0,
  });
  vi.spyOn(daemon, 'traceMemory').mockResolvedValue({
    memory: memoryFixture('m1'),
    relationships: [],
    entities: [],
  });
});

afterEach(() => {
  vi.restoreAllMocks();
  sessionStorage.clear();
});

describe('MemoryPage: M78 context freshness', () => {
  function deferNextBatch(memories: ReturnType<typeof memoryFixture>[]) {
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    vi.mocked(daemon.recentMemories).mockImplementationOnce(() =>
      pending.then(() => memories),
    );
    vi.mocked(daemon.listFacts).mockImplementationOnce(() =>
      pending.then(() => []),
    );
    vi.mocked(daemon.listMemoryEntities).mockImplementationOnce(() =>
      pending.then(() => []),
    );
    vi.mocked(daemon.listMemoryRelationships).mockImplementationOnce(() =>
      pending.then(() => []),
    );
    return release;
  }

  function expectReadBatches(count: number) {
    for (const read of [
      daemon.recentMemories,
      daemon.listFacts,
      daemon.listMemoryEntities,
      daemon.listMemoryRelationships,
    ]) {
      expect(read).toHaveBeenCalledTimes(count);
    }
  }

  function expectNoEditableRows() {
    expect(
      screen.queryByRole('button', { name: 'Edit', exact: true }),
    ).toBeNull();
    expect(
      screen.queryByRole('button', { name: 'Delete', exact: true }),
    ).toBeNull();
    expect(screen.queryByRole('form', { name: 'Edit memory' })).toBeNull();
  }

  it('m78 hides old editable rows until the fresh reconnect batch settles', async () => {
    vi.mocked(daemon.recentMemories).mockResolvedValueOnce([
      memoryFixture('before-reconnect', { content: 'Old editable memory' }),
    ]);
    const page = renderPage();
    expect(await screen.findByText('Old editable memory')).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Edit', exact: true }),
    ).toBeEnabled();
    expect(
      screen.getByRole('button', { name: 'Delete', exact: true }),
    ).toBeEnabled();
    expectReadBatches(1);
    const releaseFresh = deferNextBatch([
      memoryFixture('after-reconnect', { content: 'Fresh reconnect memory' }),
    ]);
    try {
      page.rerender(
        <MemoryPage agentId="agent-main" online={false} epoch={1} />,
      );
      expect(screen.getByText(COMPANION_UNREACHABLE)).toBeVisible();
      expect(screen.queryByText('Old editable memory')).toBeNull();
      expectNoEditableRows();
      expectReadBatches(1);

      // The same agent and epoch must still have a new online context.
      page.rerender(<MemoryPage agentId="agent-main" online epoch={1} />);
      expectReadBatches(2);
      expect(screen.queryByText('Old editable memory')).toBeNull();
      expectNoEditableRows();
      expect(screen.getByText('Loading memory…')).toBeVisible();

      await act(async () => releaseFresh());
      expect(await screen.findByText('Fresh reconnect memory')).toBeVisible();
      expect(screen.queryByText('Old editable memory')).toBeNull();
      expect(screen.queryByText('Loading memory…')).toBeNull();
      expect(
        screen.getByRole('button', { name: 'Edit', exact: true }),
      ).toBeEnabled();
      expectReadBatches(2);
      expect(daemon.updateMemory).not.toHaveBeenCalled();
      expect(daemon.deleteMemory).not.toHaveBeenCalled();
    } finally {
      await act(async () => releaseFresh());
    }
  });

  it('m78 hides the previous agent rows while new reads wait and ignores its late refresh', async () => {
    vi.mocked(daemon.recentMemories).mockResolvedValueOnce([
      memoryFixture('old-agent-memory', { content: 'Previous agent memory' }),
    ]);
    const page = render(<MemoryPage agentId="agent-old" online epoch={1} />);
    expect(await screen.findByText('Previous agent memory')).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Edit', exact: true }),
    ).toBeEnabled();
    const releaseOld = deferNextBatch([
      memoryFixture('late-old-memory', {
        content: 'Late previous agent memory',
      }),
    ]);
    const releaseNew = deferNextBatch([
      memoryFixture('new-agent-memory', { content: 'Current agent memory' }),
    ]);
    try {
      fireEvent.click(
        screen.getByRole('button', { name: 'Refresh', exact: true }),
      );
      expectReadBatches(2);
      expect(daemon.recentMemories).toHaveBeenLastCalledWith('agent-old', 200);
      page.rerender(<MemoryPage agentId="agent-new" online epoch={1} />);
      expectReadBatches(3);
      expect(daemon.recentMemories).toHaveBeenLastCalledWith('agent-new', 200);
      expect(screen.queryByText('Previous agent memory')).toBeNull();
      expectNoEditableRows();
      expect(screen.getByText('Loading memory…')).toBeVisible();

      await act(async () => releaseNew());
      expect(await screen.findByText('Current agent memory')).toBeVisible();
      await act(async () => releaseOld());
      expect(screen.getByText('Current agent memory')).toBeVisible();
      expect(screen.queryByText('Previous agent memory')).toBeNull();
      expect(screen.queryByText('Late previous agent memory')).toBeNull();
      expect(
        screen.getAllByRole('button', { name: 'Edit', exact: true }),
      ).toHaveLength(1);
      expect(
        screen.getAllByRole('button', { name: 'Delete', exact: true }),
      ).toHaveLength(1);
      expectReadBatches(3);
      expect(daemon.updateMemory).not.toHaveBeenCalled();
      expect(daemon.deleteMemory).not.toHaveBeenCalled();
    } finally {
      await act(async () => {
        releaseOld();
        releaseNew();
      });
    }
  });
});

describe('MemoryPage: memories', () => {
  it('shows the empty state when nothing is remembered', async () => {
    renderPage();
    expect(await screen.findByText(MEMORY_EMPTY)).toBeVisible();
    expect(
      screen.getByRole('heading', { name: 'What your companion remembers' }),
    ).toBeVisible();
  });

  it('lists memories with type, importance, tags, and date', async () => {
    vi.mocked(daemon.recentMemories).mockResolvedValue([
      memoryFixture('m1', {
        type: 'reflection',
        content: 'Likes green tea',
        importance: 0.8,
        tags: ['drink', 'habit'],
      }),
    ]);
    renderPage();

    const item = (await screen.findByText('Likes green tea')).closest(
      'li',
    ) as HTMLElement;
    expect(within(item).getByText('Reflection')).toBeVisible();
    expect(within(item).getByText('High importance')).toBeVisible();
    expect(within(item).getByText('drink')).toBeVisible();
    expect(within(item).getByText('habit')).toBeVisible();
    expect(item.querySelector('time')).not.toBeNull();
  });

  it('filters by type and sorts by importance', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.recentMemories).mockResolvedValue([
      memoryFixture('a', {
        content: 'Alpha',
        type: 'fact',
        importance: 0.2,
        createdAt: 3,
      }),
      memoryFixture('b', {
        content: 'Bravo',
        type: 'observation',
        importance: 0.9,
        createdAt: 1,
      }),
      memoryFixture('c', {
        content: 'Charlie',
        type: 'fact',
        importance: 0.6,
        createdAt: 2,
      }),
    ]);
    renderPage();
    await screen.findByText('Alpha');
    const order = () =>
      screen
        .getAllByRole('listitem')
        .map((item) => item.querySelector('.memory-content')?.textContent)
        .filter(Boolean);
    // Newest first by default.
    expect(order()).toEqual(['Alpha', 'Charlie', 'Bravo']);

    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Sort memories' }),
      'importance',
    );
    expect(order()).toEqual(['Bravo', 'Charlie', 'Alpha']);

    const types = screen.getByRole('group', { name: 'Type' });
    await user.click(within(types).getByRole('button', { name: 'Fact' }));
    expect(within(types).getByRole('button', { name: 'Fact' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(order()).toEqual(['Charlie', 'Alpha']);
  });

  it('searching shows matches and an empty search returns to the recent list', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.recentMemories).mockResolvedValue([
      memoryFixture('m1', { content: 'Recent one' }),
    ]);
    vi.mocked(daemon.searchMemories).mockImplementation(async (query) =>
      query === 'tea'
        ? [{ ...memoryFixture('m2', { content: 'Likes tea' }), score: 0.9 }]
        : [],
    );
    renderPage();
    await screen.findByText('Recent one');

    const box = screen.getByRole('searchbox', { name: 'Search memories' });
    await user.type(box, ' tea {Enter}');
    expect(await screen.findByText('Likes tea')).toBeVisible();
    expect(daemon.searchMemories).toHaveBeenCalledWith(
      'tea',
      'agent-main',
      200,
    );
    // Most relevant is offered while a search is active.
    expect(
      within(screen.getByRole('combobox', { name: 'Sort memories' })).getByRole(
        'option',
        { name: 'Most relevant' },
      ),
    ).toBeInTheDocument();

    await user.clear(box);
    await user.type(box, 'zzz');
    await user.click(screen.getByRole('button', { name: 'Search' }));
    expect(await screen.findByText(MEMORY_SEARCH_EMPTY)).toBeVisible();

    await user.click(screen.getByRole('button', { name: 'Clear' }));
    expect(await screen.findByText('Recent one')).toBeVisible();
    expect(
      screen.queryByRole('option', { name: 'Most relevant' }),
    ).not.toBeInTheDocument();

    // Submitting nothing is the recent list too.
    await user.type(box, 'tea{Enter}');
    await screen.findByText('Likes tea');
    await user.clear(box);
    await user.click(screen.getByRole('button', { name: 'Search' }));
    expect(await screen.findByText('Recent one')).toBeVisible();
  });

  describe('editing and deleting', () => {
    beforeEach(() => {
      vi.mocked(daemon.recentMemories).mockResolvedValue([
        memoryFixture('m1', {
          content: 'Likes tea',
          importance: 0.5,
          tags: ['drink'],
        }),
      ]);
    });

    async function openEditor(user: ReturnType<typeof userEvent.setup>) {
      renderPage();
      await screen.findByText('Likes tea');
      await user.click(screen.getByRole('button', { name: 'Edit' }));
      return screen.getByRole('form', { name: 'Edit memory' });
    }

    it('editing saves only the changed fields and closes', async () => {
      const user = userEvent.setup();
      const form = await openEditor(user);
      const save = within(form).getByRole('button', { name: 'Save' });
      expect(save).toBeDisabled();

      const text = within(form).getByRole('textbox', { name: 'Memory text' });
      await user.clear(text);
      await user.type(text, '  Likes coffee  ');
      await user.click(save);
      await waitFor(() =>
        expect(daemon.updateMemory).toHaveBeenCalledWith('m1', {
          content: 'Likes coffee',
        }),
      );
      await waitFor(() =>
        expect(
          screen.queryByRole('form', { name: 'Edit memory' }),
        ).not.toBeInTheDocument(),
      );

      await user.click(screen.getByRole('button', { name: 'Edit' }));
      const second = screen.getByRole('form', { name: 'Edit memory' });
      await user.clear(within(second).getByRole('textbox', { name: 'Tags' }));
      await user.click(within(second).getByRole('button', { name: 'Save' }));
      await waitFor(() =>
        expect(daemon.updateMemory).toHaveBeenLastCalledWith('m1', {
          tags: null,
        }),
      );
      await waitFor(() =>
        expect(
          screen.queryByRole('form', { name: 'Edit memory' }),
        ).not.toBeInTheDocument(),
      );

      await user.click(screen.getByRole('button', { name: 'Edit' }));
      const third = screen.getByRole('form', { name: 'Edit memory' });
      fireEvent.change(
        within(third).getByRole('slider', { name: 'Importance' }),
        {
          target: { value: '0.8' },
        },
      );
      expect(within(third).getByText('0.80')).toBeVisible();
      await user.click(within(third).getByRole('button', { name: 'Save' }));
      await waitFor(() =>
        expect(daemon.updateMemory).toHaveBeenLastCalledWith('m1', {
          importance: 0.8,
        }),
      );
    });

    it('editing cannot save unchanged or oversized text', async () => {
      const user = userEvent.setup();
      const form = await openEditor(user);
      const text = within(form).getByRole('textbox', { name: 'Memory text' });
      fireEvent.change(text, { target: { value: 'a'.repeat(8001) } });
      expect(within(form).getByRole('button', { name: 'Save' })).toBeDisabled();
      expect(screen.getByRole('alert')).toHaveTextContent('8,001 / 8,000');

      fireEvent.change(text, { target: { value: 'Likes tea' } });
      expect(within(form).getByRole('button', { name: 'Save' })).toBeDisabled();
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    });

    it('cancelling an edit discards it', async () => {
      const user = userEvent.setup();
      const form = await openEditor(user);
      await user.type(
        within(form).getByRole('textbox', { name: 'Memory text' }),
        ' more',
      );
      await user.click(within(form).getByRole('button', { name: 'Cancel' }));
      expect(daemon.updateMemory).not.toHaveBeenCalled();
      expect(screen.getByText('Likes tea')).toBeVisible();
      expect(
        screen.queryByRole('form', { name: 'Edit memory' }),
      ).not.toBeInTheDocument();
    });

    it('a failed save keeps the form open with the daemon’s message', async () => {
      const user = userEvent.setup();
      vi.mocked(daemon.updateMemory).mockRejectedValue(
        new DaemonHttpError(400, { error: HIDDEN_REFUSAL }),
      );
      const form = await openEditor(user);
      await user.type(
        within(form).getByRole('textbox', { name: 'Memory text' }),
        ' more',
      );
      await user.click(within(form).getByRole('button', { name: 'Save' }));

      expect(await screen.findByRole('alert')).toHaveTextContent(
        HIDDEN_REFUSAL,
      );
      expect(screen.getByRole('form', { name: 'Edit memory' })).toBeVisible();
      expect(within(form).getByRole('button', { name: 'Save' })).toBeEnabled();
    });

    it('deleting asks first, Keep cancels, and confirming deletes', async () => {
      const user = userEvent.setup();
      renderPage();
      await screen.findByText('Likes tea');

      await user.click(screen.getByRole('button', { name: 'Delete' }));
      expect(screen.getByText(DELETE_MEMORY_PROMPT)).toBeVisible();
      await user.click(screen.getByRole('button', { name: 'Keep' }));
      expect(screen.queryByText(DELETE_MEMORY_PROMPT)).not.toBeInTheDocument();
      expect(daemon.deleteMemory).not.toHaveBeenCalled();

      await user.click(screen.getByRole('button', { name: 'Delete' }));
      await user.keyboard('{Escape}');
      expect(screen.queryByText(DELETE_MEMORY_PROMPT)).not.toBeInTheDocument();

      await user.click(screen.getByRole('button', { name: 'Delete' }));
      vi.mocked(daemon.recentMemories).mockResolvedValue([]);
      await user.click(screen.getByRole('button', { name: 'Delete memory' }));
      await waitFor(() =>
        expect(daemon.deleteMemory).toHaveBeenCalledWith('m1', 'Likes tea'),
      );
      expect(await screen.findByText(MEMORY_EMPTY)).toBeVisible();
      await waitFor(() =>
        expect(screen.getByRole('heading', { name: 'Memories' })).toHaveFocus(),
      );
    });

    it('deletes only the memory text that was reviewed before a background refresh', async () => {
      const user = userEvent.setup();
      const reviewed = memoryFixture('m1', { content: 'Reviewed wording' });
      const refreshed = memoryFixture('m1', { content: 'Changed wording' });
      vi.mocked(daemon.recentMemories)
        .mockResolvedValueOnce([reviewed])
        .mockResolvedValue([refreshed]);
      renderPage();

      await screen.findByText('Reviewed wording');
      await user.click(screen.getByRole('button', { name: 'Delete' }));
      expect(screen.getByText(DELETE_MEMORY_PROMPT)).toBeVisible();

      await user.click(screen.getByRole('button', { name: 'Refresh' }));
      expect(await screen.findByText('Reviewed wording')).toBeVisible();
      expect(screen.queryByText('Changed wording')).not.toBeInTheDocument();

      await user.click(screen.getByRole('button', { name: 'Delete memory' }));
      await waitFor(() =>
        expect(daemon.deleteMemory).toHaveBeenCalledWith(
          'm1',
          'Reviewed wording',
        ),
      );
    });

    it('a stale deletion refreshes the row and requires a new confirmation', async () => {
      const user = userEvent.setup();
      const reviewed = memoryFixture('m1', { content: 'Reviewed wording' });
      const refreshed = memoryFixture('m1', { content: 'Changed wording' });
      vi.mocked(daemon.recentMemories)
        .mockResolvedValueOnce([reviewed])
        .mockResolvedValueOnce([refreshed])
        .mockResolvedValue([]);
      vi.mocked(daemon.deleteMemory)
        .mockRejectedValueOnce(
          new DaemonHttpError(409, {
            error: 'Memory text changed; review it again',
          }),
        )
        .mockResolvedValueOnce(deleted);
      renderPage();

      await screen.findByText('Reviewed wording');
      await user.click(screen.getByRole('button', { name: 'Delete' }));
      await user.click(screen.getByRole('button', { name: 'Delete memory' }));

      expect(await screen.findByRole('alert')).toHaveTextContent(
        'Memory text changed; review it again',
      );
      expect(await screen.findByText('Changed wording')).toBeVisible();
      expect(
        screen.queryByRole('button', { name: 'Delete memory' }),
      ).not.toBeInTheDocument();
      expect(daemon.deleteMemory).toHaveBeenNthCalledWith(
        1,
        'm1',
        'Reviewed wording',
      );

      await user.click(screen.getByRole('button', { name: 'Delete' }));
      await user.click(screen.getByRole('button', { name: 'Delete memory' }));

      await waitFor(() =>
        expect(daemon.deleteMemory).toHaveBeenNthCalledWith(
          2,
          'm1',
          'Changed wording',
        ),
      );
    });

    it('does not send a second deletion while confirmation is in flight', async () => {
      const user = userEvent.setup();
      let finishDelete!: (result: typeof deleted) => void;
      vi.mocked(daemon.deleteMemory).mockImplementation(
        () => new Promise((resolve) => (finishDelete = resolve)),
      );
      renderPage();

      await screen.findByText('Likes tea');
      await user.click(screen.getByRole('button', { name: 'Delete' }));
      const confirm = screen.getByRole('button', { name: 'Delete memory' });
      await user.click(confirm);
      await user.click(confirm);

      expect(daemon.deleteMemory).toHaveBeenCalledTimes(1);
      expect(daemon.deleteMemory).toHaveBeenCalledWith('m1', 'Likes tea');

      finishDelete(deleted);
      await waitFor(() =>
        expect(screen.getByRole('heading', { name: 'Memories' })).toHaveFocus(),
      );
    });

    it('a 404 on delete says the memory is gone', async () => {
      const user = userEvent.setup();
      vi.mocked(daemon.deleteMemory).mockRejectedValue(
        new DaemonHttpError(404, { error: 'memory not found' }),
      );
      renderPage();
      await screen.findByText('Likes tea');
      await user.click(screen.getByRole('button', { name: 'Delete' }));
      vi.mocked(daemon.recentMemories).mockResolvedValue([]);
      await user.click(screen.getByRole('button', { name: 'Delete memory' }));
      expect(await screen.findByRole('alert')).toHaveTextContent(MEMORY_GONE);
    });
  });

  it('shows hidden characters as markers', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.recentMemories).mockResolvedValue([
      memoryFixture('m1', {
        content: `Likes${ZWSP}tea`,
        tags: [`dr${ZWSP}ink`],
      }),
    ]);
    vi.mocked(daemon.listFacts).mockResolvedValue([
      factFixture('f1', { value: `Short${ZWSP}answers` }),
    ]);
    vi.mocked(daemon.listMemoryEntities).mockResolvedValue([
      entityFixture('e1', { name: `Ana${ZWSP}` }),
    ]);
    vi.mocked(daemon.listMemoryRelationships).mockResolvedValue([
      relationshipFixture('r1', { summary: `Friends${ZWSP}` }),
    ]);
    renderPage();

    const item = (await screen.findByText(`Likes${HIDDEN_MARKER}tea`)).closest(
      'li',
    ) as HTMLElement;
    expect(within(item).getByText(`dr${HIDDEN_MARKER}ink`)).toBeVisible();
    expect(
      within(item).getAllByText('This text contains 1 invisible character'),
    ).toHaveLength(2);

    await user.click(screen.getByRole('tab', { name: 'About you' }));
    expect(
      await screen.findByText(`Short${HIDDEN_MARKER}answers`),
    ).toBeVisible();
    expect(
      screen.getByText('This text contains 1 invisible character'),
    ).toBeVisible();

    await user.click(screen.getByRole('tab', { name: 'People & things' }));
    expect(await screen.findByText(`Ana${HIDDEN_MARKER}`)).toBeVisible();
    expect(screen.getByText(`Friends${HIDDEN_MARKER}`)).toBeVisible();
    expect(
      screen.getAllByText('This text contains 1 invisible character'),
    ).toHaveLength(2);
  });

  it('the editor can remove invisible characters', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.recentMemories).mockResolvedValue([
      memoryFixture('m1', { content: `Likes${TAG_A}tea` }),
    ]);
    renderPage();
    await screen.findByText(`Likes${TAG_MARKER}tea`);
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    const form = screen.getByRole('form', { name: 'Edit memory' });
    expect(
      within(form).getByText('This text contains 1 invisible character'),
    ).toBeVisible();

    await user.click(
      within(form).getByRole('button', { name: 'Remove invisible characters' }),
    );
    expect(
      within(form).queryByText('This text contains 1 invisible character'),
    ).not.toBeInTheDocument();
    await user.click(within(form).getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(daemon.updateMemory).toHaveBeenCalledWith('m1', {
        content: 'Likestea',
      }),
    );
  });

  it('the editor keeps zero-width spaces, which the daemon accepts', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.recentMemories).mockResolvedValue([
      memoryFixture('m1', { content: `Likes${ZWSP}tea` }),
    ]);
    renderPage();
    await screen.findByText(`Likes${HIDDEN_MARKER}tea`);
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    const form = screen.getByRole('form', { name: 'Edit memory' });
    expect(
      within(form).getByText('This text contains 1 invisible character'),
    ).toBeVisible();
    expect(
      within(form).queryByRole('button', {
        name: 'Remove invisible characters',
      }),
    ).not.toBeInTheDocument();
  });

  it('a memory already over 8,000 characters can still have its tags edited', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.recentMemories).mockResolvedValue([
      memoryFixture('m1', { content: 'a'.repeat(8_001), tags: ['old'] }),
    ]);
    renderPage();
    await screen.findByText('a'.repeat(8_001));
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    const form = screen.getByRole('form', { name: 'Edit memory' });
    expect(within(form).queryByRole('alert')).not.toBeInTheDocument();

    const tags = within(form).getByRole('textbox', { name: 'Tags' });
    await user.clear(tags);
    await user.type(tags, 'new');
    await user.click(within(form).getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(daemon.updateMemory).toHaveBeenCalledWith('m1', {
        tags: ['new'],
      }),
    );
  });

  it('a changed memory over 8,000 characters is still refused', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.recentMemories).mockResolvedValue([
      memoryFixture('m1', { content: 'a'.repeat(8_001) }),
    ]);
    renderPage();
    await screen.findByText('a'.repeat(8_001));
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    const form = screen.getByRole('form', { name: 'Edit memory' });
    fireEvent.change(
      within(form).getByRole('textbox', { name: 'Memory text' }),
      {
        target: { value: 'b'.repeat(8_001) },
      },
    );
    expect(within(form).getByRole('alert')).toHaveTextContent('8,001 / 8,000');
    expect(within(form).getByRole('button', { name: 'Save' })).toBeDisabled();
  });

  it('renders model text as text, never markup', async () => {
    vi.mocked(daemon.recentMemories).mockResolvedValue([
      memoryFixture('m1', { content: '<img src=x onerror=alert(1)>' }),
    ]);
    const { container } = renderPage();
    expect(
      await screen.findByText('<img src=x onerror=alert(1)>'),
    ).toBeVisible();
    expect(container.querySelector('img')).toBeNull();
  });

  describe('where it was used', () => {
    beforeEach(() => {
      vi.mocked(daemon.recentMemories).mockResolvedValue([
        memoryFixture('m1', { content: 'Likes tea' }),
      ]);
    });

    it('shows relationships and the facts that cite the memory', async () => {
      const user = userEvent.setup();
      vi.mocked(daemon.listFacts).mockResolvedValue([
        factFixture('f1', {
          predicate: 'favorite_drink',
          value: 'Green tea',
          evidenceMemoryIds: ['m1'],
        }),
        factFixture('f2', { value: 'Unrelated', evidenceMemoryIds: ['other'] }),
      ]);
      vi.mocked(daemon.traceMemory).mockResolvedValue({
        memory: memoryFixture('m1'),
        relationships: [
          relationshipFixture('r1', {
            relationshipType: 'enjoys',
            summary: 'Drinks it daily',
            sourceAgentName: 'You',
            targetAgentName: 'Tea',
          }),
        ],
        entities: [],
      });
      renderPage();
      await screen.findByText('Likes tea');

      const toggle = screen.getByRole('button', { name: 'Where it was used' });
      expect(toggle).toHaveAttribute('aria-expanded', 'false');
      await user.click(toggle);
      expect(daemon.traceMemory).toHaveBeenCalledWith('m1');
      expect(toggle).toHaveAttribute('aria-expanded', 'true');

      const relationships = await screen.findByRole('region', {
        name: 'Relationships',
      });
      expect(relationships).toHaveTextContent(
        'enjoys: You → Tea · Drinks it daily',
      );
      expect(screen.getByRole('region', { name: 'Facts' })).toHaveTextContent(
        'favorite drink: Green tea',
      );
      expect(screen.queryByText('Unrelated')).not.toBeInTheDocument();

      await user.click(toggle);
      expect(toggle).toHaveAttribute('aria-expanded', 'false');
      expect(
        screen.queryByRole('region', { name: 'Relationships' }),
      ).not.toBeInTheDocument();
    });

    it('closing the trail while it loads keeps it closed', async () => {
      const user = userEvent.setup();
      let release: (
        value: Awaited<ReturnType<typeof daemon.traceMemory>>,
      ) => void = () => undefined;
      vi.mocked(daemon.traceMemory).mockImplementation(
        () => new Promise((resolve) => (release = resolve)),
      );
      renderPage();
      await screen.findByText('Likes tea');
      const toggle = screen.getByRole('button', { name: 'Where it was used' });
      await user.click(toggle);
      expect(await screen.findByText('Loading…')).toBeVisible();

      await user.click(toggle);
      expect(toggle).toHaveAttribute('aria-expanded', 'false');
      await act(async () => {
        release({
          memory: memoryFixture('m1'),
          relationships: [],
          entities: [],
        });
      });
      expect(toggle).toHaveAttribute('aria-expanded', 'false');
      expect(
        screen.queryByText('Nothing else cites this memory.'),
      ).not.toBeInTheDocument();
    });

    it('an uncited memory says so', async () => {
      const user = userEvent.setup();
      renderPage();
      await screen.findByText('Likes tea');
      await user.click(
        screen.getByRole('button', { name: 'Where it was used' }),
      );
      expect(
        await screen.findByText('Nothing else cites this memory.'),
      ).toBeVisible();
    });

    it('a failed trail says it could not load', async () => {
      const user = userEvent.setup();
      vi.mocked(daemon.traceMemory).mockRejectedValue(
        new DaemonHttpError(503, { error: 'Memory is busy' }),
      );
      renderPage();
      await screen.findByText('Likes tea');
      await user.click(
        screen.getByRole('button', { name: 'Where it was used' }),
      );
      expect(await screen.findByText('Couldn’t load the trail.')).toBeVisible();
    });
  });
});

describe('MemoryPage: About you', () => {
  it('M40_shows_an_owner_controlled_profile_and_agent_notes_panel', async () => {
    const user = userEvent.setup();
    renderPage();

    await user.click(screen.getByRole('tab', { name: 'About you' }));
    expect(
      await screen.findByRole('heading', { name: 'Profile & agent notes' }),
    ).toBeVisible();
  });

  async function openAbout() {
    const user = userEvent.setup();
    renderPage();
    await screen.findByText(MEMORY_EMPTY);
    await user.click(screen.getByRole('tab', { name: 'About you' }));
    return user;
  }

  it('shows the empty state', async () => {
    await openAbout();
    expect(await screen.findByText(FACTS_EMPTY)).toBeVisible();
  });

  it('selects an exact saved source into the profile with the current view version and hash', async () => {
    const source = {
      id: 'fact-source-1',
      content: 'Prefers short answers',
      sourceHash: 'sha256-current',
    };
    vi.mocked(daemon.memoryViewCandidates).mockResolvedValue({
      candidates: [source],
      hasMore: false,
    });
    vi.mocked(daemon.getMemoryViews).mockResolvedValue({
      version: 7,
      viewDigest: 'view-7',
      userProfile: [],
      agentNotes: [],
      unavailable: [],
    });
    vi.mocked(daemon.selectMemoryViews).mockResolvedValue({
      version: 8,
      viewDigest: 'view-8',
      userProfile: [source],
      agentNotes: [],
      unavailable: [],
    });
    const user = await openAbout();

    await screen.findByText('Prefers short answers');
    await user.click(screen.getByRole('button', { name: 'Add to profile' }));
    await user.click(
      screen.getByRole('button', { name: 'Save profile and notes' }),
    );

    await waitFor(() =>
      expect(daemon.selectMemoryViews).toHaveBeenCalledWith('agent-main', {
        expectedVersion: 7,
        userProfile: [{ id: 'fact-source-1', sourceHash: 'sha256-current' }],
        agentNotes: [],
      }),
    );
    expect(await screen.findByLabelText('Your profile')).toHaveTextContent(
      'Prefers short answers',
    );
  });

  it('keeps correction text after a stale source conflict without retrying', async () => {
    const source = {
      id: 'fact-source-1',
      content: 'Prefers short answers',
      sourceHash: 'sha256-current',
    };
    vi.mocked(daemon.getMemoryViews).mockResolvedValue({
      version: 7,
      viewDigest: 'view-7',
      userProfile: [source],
      agentNotes: [],
      unavailable: [],
    });
    vi.mocked(daemon.correctMemoryViewSource).mockRejectedValue(
      new DaemonHttpError(409, { error: 'Source changed' }),
    );
    const user = await openAbout();
    const editor = await screen.findByRole('textbox', {
      name: 'Correct this saved fact',
    });
    await user.clear(editor);
    await user.type(editor, 'Keep responses concise.');
    await user.click(screen.getByRole('button', { name: 'Save correction' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      /Your edits are kept/,
    );
    expect(editor).toHaveValue('Keep responses concise.');
    expect(daemon.correctMemoryViewSource).toHaveBeenCalledOnce();
    expect(daemon.correctMemoryViewSource).toHaveBeenCalledWith(
      'agent-main',
      'fact-source-1',
      {
        expectedVersion: 7,
        expectedContent: 'Prefers short answers',
        expectedSourceHash: 'sha256-current',
        content: 'Keep responses concise.',
      },
    );
  });

  it('requires explicit removal of unavailable source references before saving', async () => {
    vi.mocked(daemon.getMemoryViews).mockResolvedValue({
      version: 9,
      viewDigest: 'view-9',
      userProfile: [],
      agentNotes: [],
      unavailable: [{ id: 'gone-source', reason: 'unavailable' }],
    });
    const user = await openAbout();
    const save = await screen.findByRole('button', {
      name: 'Save profile and notes',
    });
    expect(save).toBeDisabled();

    await user.click(
      screen.getByRole('button', { name: 'Remove unavailable sources' }),
    );
    expect(save).toBeEnabled();
    await user.click(save);
    await waitFor(() =>
      expect(daemon.selectMemoryViews).toHaveBeenCalledWith('agent-main', {
        expectedVersion: 9,
        userProfile: [],
        agentNotes: [],
      }),
    );
  });

  it('ignores a late profile response after switching agents', async () => {
    let resolveOld!: (
      value: Awaited<ReturnType<typeof daemon.getMemoryViews>>,
    ) => void;
    const oldSource = {
      id: 'old-fact',
      content: 'Old agent fact',
      sourceHash: 'old-hash',
    };
    const newSource = {
      id: 'new-fact',
      content: 'New agent fact',
      sourceHash: 'new-hash',
    };
    vi.mocked(daemon.getMemoryViews).mockImplementation((agentId) =>
      agentId === 'agent-old'
        ? new Promise((resolve) => {
            resolveOld = resolve;
          })
        : Promise.resolve({
            version: 1,
            viewDigest: 'new-view',
            userProfile: [newSource],
            agentNotes: [],
            unavailable: [],
          }),
    );
    const view = render(<MemoryPage agentId="agent-old" online epoch={1} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole('tab', { name: 'About you' }));
    view.rerender(<MemoryPage agentId="agent-new" online epoch={1} />);
    const newProfile = await screen.findByRole('region', {
      name: 'Your profile',
    });
    expect(
      await within(newProfile).findByText('New agent fact', {
        selector: 'span',
      }),
    ).toBeVisible();
    resolveOld({
      version: 1,
      viewDigest: 'old-view',
      userProfile: [oldSource],
      agentNotes: [],
      unavailable: [],
    });
    await waitFor(() =>
      expect(
        within(screen.getByRole('region', { name: 'Your profile' })).getByText(
          'New agent fact',
          { selector: 'span' },
        ),
      ).toBeVisible(),
    );
    expect(screen.queryByText('Old agent fact')).not.toBeInTheDocument();
  });

  it('keeps the exact reviewed source snapshot when a later search returns changed text', async () => {
    const reviewed = {
      id: 'fact-1',
      content: 'Reviewed source text',
      sourceHash: 'hash-reviewed',
    };
    const fresh = {
      id: 'fact-1',
      content: 'New canonical text',
      sourceHash: 'hash-fresh',
    };
    vi.mocked(daemon.memoryViewCandidates)
      .mockResolvedValueOnce({ candidates: [reviewed], hasMore: false })
      .mockResolvedValueOnce({ candidates: [fresh], hasMore: false });
    vi.mocked(daemon.selectMemoryViews).mockRejectedValue(
      new DaemonHttpError(409, { error: 'Source changed' }),
    );
    const user = await openAbout();

    await user.click(
      await screen.findByRole('button', { name: 'Add to profile' }),
    );
    await user.type(
      screen.getByRole('searchbox', {
        name: 'Find saved facts for profile or notes',
      }),
      'new',
    );
    await user.click(screen.getByRole('button', { name: 'Find saved facts' }));

    expect(await screen.findByText('New canonical text')).toBeVisible();
    expect(
      within(screen.getByRole('region', { name: 'Your profile' })).getByText(
        'Reviewed source text',
      ),
    ).toBeVisible();
    await user.click(
      screen.getByRole('button', { name: 'Save profile and notes' }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      /edits are kept/i,
    );
    expect(daemon.selectMemoryViews).toHaveBeenCalledWith('agent-main', {
      expectedVersion: 0,
      userProfile: [{ id: 'fact-1', sourceHash: 'hash-reviewed' }],
      agentNotes: [],
    });
  });

  it('keeps an unsaved reviewed pick pinned when refresh prunes the bounded snapshot cache', async () => {
    const reviewed = {
      id: 'fact-reviewed',
      content: 'Pinned reviewed fact',
      sourceHash: 'hash-reviewed',
    };
    let initialCandidateRequest = true;
    vi.mocked(daemon.memoryViewCandidates).mockImplementation(
      async (_agentId, query) => {
        if (query === undefined) {
          if (initialCandidateRequest) {
            initialCandidateRequest = false;
            return { candidates: [reviewed], hasMore: false };
          }
          return {
            candidates: Array.from({ length: 50 }, (_, index) => ({
              id: `refresh-${index}`,
              content: `Refresh candidate ${index}`,
              sourceHash: `refresh-hash-${index}`,
            })),
            hasMore: false,
          };
        }
        const batch = Number(query.replace('batch-', ''));
        return {
          candidates: Array.from({ length: 50 }, (_, index) => ({
            id: `batch-${batch}-fact-${index}`,
            content: `Batch ${batch} candidate ${index}`,
            sourceHash: `batch-${batch}-hash-${index}`,
          })),
          hasMore: false,
        };
      },
    );
    const user = await openAbout();

    await user.click(
      await screen.findByRole('button', { name: 'Add to profile' }),
    );
    const search = screen.getByRole('searchbox', {
      name: 'Find saved facts for profile or notes',
    });
    for (let batch = 0; batch < 6; batch += 1) {
      fireEvent.change(search, { target: { value: `batch-${batch}` } });
      fireEvent.submit(search.closest('form')!);
      expect(
        await screen.findByText(`Batch ${batch} candidate 49`),
      ).toBeVisible();
    }

    await user.click(
      screen.getByRole('button', { name: 'Refresh profile and notes' }),
    );
    expect(await screen.findByText('Refresh candidate 49')).toBeVisible();
    const profile = screen.getByRole('region', { name: 'Your profile' });
    expect(within(profile).getByText('Pinned reviewed fact')).toBeVisible();
    expect(
      within(profile).getByRole('button', { name: 'Remove from profile' }),
    ).toBeVisible();
  });

  it('retains a correction draft through explicit refresh and requires a new save', async () => {
    const oldSource = {
      id: 'fact-1',
      content: 'Old saved text',
      sourceHash: 'hash-old',
    };
    const currentSource = {
      id: 'fact-1',
      content: 'Current saved text',
      sourceHash: 'hash-current',
    };
    vi.mocked(daemon.getMemoryViews)
      .mockResolvedValueOnce({
        version: 4,
        viewDigest: 'view-4',
        userProfile: [oldSource],
        agentNotes: [],
        unavailable: [],
      })
      .mockResolvedValueOnce({
        version: 5,
        viewDigest: 'view-5',
        userProfile: [currentSource],
        agentNotes: [],
        unavailable: [],
      })
      .mockResolvedValue({
        version: 6,
        viewDigest: 'view-6',
        userProfile: [
          {
            ...currentSource,
            content: 'My corrected text',
            sourceHash: 'hash-corrected',
          },
        ],
        agentNotes: [],
        unavailable: [],
      });
    vi.mocked(daemon.correctMemoryViewSource)
      .mockRejectedValueOnce(
        new DaemonHttpError(409, { error: 'Stale source' }),
      )
      .mockResolvedValue({
        version: 6,
        viewDigest: 'view-6',
        userProfile: [
          {
            id: 'fact-1',
            content: 'My corrected text',
            sourceHash: 'hash-corrected',
          },
        ],
        agentNotes: [],
        unavailable: [],
      });
    const user = await openAbout();
    const editor = await screen.findByRole('textbox', {
      name: 'Correct this saved fact',
    });
    await user.clear(editor);
    await user.type(editor, 'My corrected text');
    await user.click(screen.getByRole('button', { name: 'Save correction' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      /Your edits are kept/,
    );

    await user.click(
      screen.getByRole('button', { name: 'Refresh profile and notes' }),
    );
    await waitFor(() => expect(editor).toHaveValue('My corrected text'));
    expect(
      within(screen.getByRole('region', { name: 'Your profile' })).getByText(
        'Current saved text',
      ),
    ).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Save correction' }));
    await waitFor(() =>
      expect(daemon.correctMemoryViewSource).toHaveBeenCalledTimes(2),
    );
    expect(daemon.correctMemoryViewSource).toHaveBeenLastCalledWith(
      'agent-main',
      'fact-1',
      {
        expectedVersion: 5,
        expectedContent: 'Current saved text',
        expectedSourceHash: 'hash-current',
        content: 'My corrected text',
      },
    );
  });

  it('blocks correction while a selection change is unsaved', async () => {
    const saved = {
      id: 'fact-1',
      content: 'Saved preference',
      sourceHash: 'hash-1',
    };
    const candidate = {
      id: 'fact-2',
      content: 'Another saved fact',
      sourceHash: 'hash-2',
    };
    vi.mocked(daemon.getMemoryViews).mockResolvedValue({
      version: 3,
      viewDigest: 'view-3',
      userProfile: [saved],
      agentNotes: [],
      unavailable: [],
    });
    vi.mocked(daemon.memoryViewCandidates).mockResolvedValue({
      candidates: [saved, candidate],
      hasMore: false,
    });
    const user = await openAbout();
    const eligible = screen.getByRole('list', { name: 'Eligible saved facts' });
    const candidateRow = within(eligible)
      .getAllByRole('listitem')
      .find((item) => item.textContent?.includes('Another saved fact'));
    expect(candidateRow).toBeDefined();
    await user.click(
      within(candidateRow!).getByRole('button', { name: 'Add to notes' }),
    );
    const editor = await screen.findByRole('textbox', {
      name: 'Correct this saved fact',
    });
    await user.clear(editor);
    await user.type(editor, 'Corrected preference');

    expect(
      screen.getByText('Save your selection changes before correcting a fact.'),
    ).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Save correction' }),
    ).toBeDisabled();
    expect(daemon.correctMemoryViewSource).not.toHaveBeenCalled();
    expect(
      screen.getByRole('button', { name: 'Save profile and notes' }),
    ).toBeEnabled();
  });

  it('groups preferences apart and edits a fact by replacing it', async () => {
    vi.mocked(daemon.listFacts).mockResolvedValue([
      factFixture('f1', {
        predicate: 'communication_preference',
        value: 'Prefers short answers',
      }),
      factFixture('f2', { predicate: 'lives_in', value: 'Lisbon' }),
    ]);
    const user = await openAbout();

    const preferences = await screen.findByRole('region', {
      name: 'Preferences',
    });
    expect(
      within(preferences).getByText('Prefers short answers'),
    ).toBeVisible();
    expect(
      within(preferences).getByText('communication preference'),
    ).toBeVisible();
    const told = screen.getByRole('region', { name: 'Things you’ve told me' });
    expect(within(told).getByText('Lisbon')).toBeVisible();

    await user.click(within(told).getByRole('button', { name: 'Edit' }));
    expect(screen.getByText(FACT_EDIT_HINT)).toBeVisible();
    const form = screen.getByRole('form', { name: 'Edit fact' });
    const save = within(form).getByRole('button', { name: 'Save' });
    expect(save).toBeDisabled();
    const text = within(form).getByRole('textbox', { name: 'Fact' });
    await user.clear(text);
    expect(save).toBeDisabled();
    await user.type(text, '  Porto ');
    await user.click(save);
    await waitFor(() =>
      expect(daemon.replaceFact).toHaveBeenCalledWith('f2', 'Porto'),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole('form', { name: 'Edit fact' }),
      ).not.toBeInTheDocument(),
    );
  });

  it('refuses a fact over 500 characters', async () => {
    vi.mocked(daemon.listFacts).mockResolvedValue([factFixture('f1')]);
    const user = await openAbout();
    await user.click(await screen.findByRole('button', { name: 'Edit' }));
    const form = screen.getByRole('form', { name: 'Edit fact' });
    fireEvent.change(within(form).getByRole('textbox', { name: 'Fact' }), {
      target: { value: 'a'.repeat(501) },
    });
    expect(within(form).getByRole('button', { name: 'Save' })).toBeDisabled();
    expect(screen.getByRole('alert')).toHaveTextContent('501 / 500');
  });

  it('the fact editor can remove invisible characters', async () => {
    vi.mocked(daemon.listFacts).mockResolvedValue([
      factFixture('f1', { value: `Short${TAG_A}answers` }),
    ]);
    const user = await openAbout();
    await user.click(await screen.findByRole('button', { name: 'Edit' }));
    const form = screen.getByRole('form', { name: 'Edit fact' });
    expect(
      within(form).getByText('This text contains 1 invisible character'),
    ).toBeVisible();

    await user.click(
      within(form).getByRole('button', { name: 'Remove invisible characters' }),
    );
    expect(
      within(form).queryByText('This text contains 1 invisible character'),
    ).not.toBeInTheDocument();
    await user.click(within(form).getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(daemon.replaceFact).toHaveBeenCalledWith('f1', 'Shortanswers'),
    );
  });

  it('forgetting a fact asks first', async () => {
    vi.mocked(daemon.listFacts).mockResolvedValue([factFixture('f1')]);
    const user = await openAbout();
    await user.click(await screen.findByRole('button', { name: 'Forget' }));
    expect(screen.getByText(DELETE_FACT_PROMPT)).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Keep' }));
    expect(daemon.deleteFact).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: 'Forget' }));
    vi.mocked(daemon.listFacts).mockResolvedValue([]);
    // The confirm row's Forget is the button next to Keep.
    const confirm = screen.getByRole('group', { name: 'Confirm' });
    await user.click(within(confirm).getByRole('button', { name: 'Forget' }));
    await waitFor(() => expect(daemon.deleteFact).toHaveBeenCalledWith('f1'));
    expect(await screen.findByText(FACTS_EMPTY)).toBeVisible();
  });

  it('replaced facts are labelled and read-only after “Show replaced facts”', async () => {
    vi.mocked(daemon.listFacts).mockImplementation(async (options) =>
      options.includeInactive
        ? [
            factFixture('f2'),
            factFixture('f1', { status: 'superseded', value: 'Old value' }),
          ]
        : [factFixture('f2')],
    );
    const user = await openAbout();
    await screen.findByText('Prefers short answers');
    expect(screen.queryByText(REPLACED_LABEL)).not.toBeInTheDocument();

    await user.click(
      screen.getByRole('checkbox', { name: 'Show replaced facts' }),
    );
    const old = (await screen.findByText('Old value')).closest(
      'li',
    ) as HTMLElement;
    expect(daemon.listFacts).toHaveBeenLastCalledWith(
      expect.objectContaining({ includeInactive: true }),
    );
    expect(within(old).getByText(REPLACED_LABEL)).toBeVisible();
    expect(within(old).queryByRole('button')).not.toBeInTheDocument();
  });

  it('a fact with an object and no value cannot be edited', async () => {
    vi.mocked(daemon.listFacts).mockResolvedValue([
      factFixture('f1', {
        predicate: 'works_with',
        value: null,
        objectKind: 'external',
        objectId: 'e1',
        objectName: 'Ada',
      }),
    ]);
    await openAbout();
    expect(await screen.findByText('Ada')).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Edit' }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Forget' })).toBeVisible();
  });
});

describe('MemoryPage: People & things', () => {
  async function openPeople() {
    const user = userEvent.setup();
    renderPage();
    await screen.findByText(MEMORY_EMPTY);
    await user.click(screen.getByRole('tab', { name: 'People & things' }));
    return user;
  }

  it('shows the empty state', async () => {
    await openPeople();
    expect(await screen.findByText(ENTITIES_EMPTY)).toBeVisible();
  });

  it('lists entities and relationships and removes an entity after asking', async () => {
    vi.mocked(daemon.listMemoryEntities).mockResolvedValue([
      entityFixture('e1', {
        name: 'Ada',
        aliases: ['Countess'],
        summary: 'A friend',
      }),
      entityFixture('owner', { kind: 'user', name: 'You' }),
    ]);
    vi.mocked(daemon.listMemoryRelationships).mockResolvedValue([
      relationshipFixture('r1', {
        sourceAgentName: 'You',
        targetAgentName: 'Ada',
        relationshipType: 'knows',
        summary: 'Met in 2020',
      }),
    ]);
    const user = await openPeople();

    const list = await screen.findByRole('list', { name: 'People and things' });
    const ada = within(list).getByText('Ada').closest('li') as HTMLElement;
    expect(within(ada).getByText('Thing')).toBeVisible();
    expect(within(ada).getByText('Countess')).toBeVisible();
    expect(within(ada).getByText('A friend')).toBeVisible();
    const connections = screen.getByRole('region', { name: 'Connections' });
    expect(connections).toHaveTextContent('You → Ada · knows');
    expect(within(connections).getByText('Met in 2020')).toBeVisible();
    // Deleting the user entity is soft, and the page says so.
    expect(
      screen.getByText(
        'Your companion adds this again when it learns about you.',
      ),
    ).toBeVisible();

    await user.click(within(ada).getByRole('button', { name: 'Remove' }));
    expect(screen.getByText(DELETE_ENTITY_PROMPT)).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Keep' }));
    expect(daemon.deleteMemoryEntity).not.toHaveBeenCalled();

    await user.click(within(ada).getByRole('button', { name: 'Remove' }));
    await user.click(
      within(screen.getByRole('group', { name: 'Confirm' })).getByRole(
        'button',
        { name: 'Remove' },
      ),
    );
    await waitFor(() =>
      expect(daemon.deleteMemoryEntity).toHaveBeenCalledWith('external', 'e1'),
    );
  });

  it('an entity that still has memories shows the daemon’s refusal', async () => {
    const refusal = 'This entity still has memories; delete them first';
    vi.mocked(daemon.listMemoryEntities).mockResolvedValue([
      entityFixture('e1', { kind: 'agent', name: 'Helper' }),
    ]);
    vi.mocked(daemon.deleteMemoryEntity).mockRejectedValue(
      new DaemonHttpError(409, { error: refusal }),
    );
    const user = await openPeople();

    await user.click(await screen.findByRole('button', { name: 'Remove' }));
    await user.click(
      within(screen.getByRole('group', { name: 'Confirm' })).getByRole(
        'button',
        { name: 'Remove' },
      ),
    );
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(refusal);
    // The message sits beside the entity, not in a second alert.
    expect(screen.getAllByRole('alert')).toHaveLength(1);
    expect(alert.closest('li')).toHaveTextContent('Helper');
  });
});

describe('MemoryPage: page', () => {
  it('offers a memory search status check on the online page', () => {
    renderPage();

    const checkStatus = screen.getByRole('button', {
      name: 'Check memory search status',
    });

    expect(checkStatus).toBeVisible();
    expect(checkStatus).toBeEnabled();
  });

  it('keeps search, tab, and Refresh separate from the explicit index check', async () => {
    const read = vi.spyOn(daemon, 'memoryReadiness').mockResolvedValue({
      passed: false,
      embeddings: {
        enabled: false,
        provider: 'local',
        model: 'small-embed',
        dimension: 384,
        vectorCount: 0,
        persisted: false,
      },
      evaluation: {
        passed: false,
        totalChecks: 0,
        passedChecks: 0,
        failureMessages: [],
        cases: [],
      },
    });
    const user = userEvent.setup();
    renderPage();
    await screen.findByText(MEMORY_EMPTY);
    await user.type(
      screen.getByRole('searchbox', { name: 'Search memories' }),
      'tea',
    );
    await user.click(
      screen.getByRole('button', { name: 'Search', exact: true }),
    );
    await user.click(screen.getByRole('tab', { name: 'About you' }));
    await user.click(screen.getByRole('tab', { name: 'Memories' }));
    await user.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(read).not.toHaveBeenCalled();
    expect(
      screen.getByRole('searchbox', { name: 'Search memories' }),
    ).toHaveValue('tea');

    await user.click(
      screen.getByRole('button', { name: 'Check memory search status' }),
    );
    expect(read).toHaveBeenCalledTimes(1);
    expect(
      await screen.findByText(/Daemon-wide vector index: disabled/),
    ).toBeVisible();
    expect(
      screen.getByRole('searchbox', { name: 'Search memories' }),
    ).toHaveValue('tea');
    expect(screen.getByRole('tab', { name: 'Memories' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
  });

  it('offline shows the unreachable text and no lists', () => {
    renderPage({ online: false });
    expect(screen.getByText(COMPANION_UNREACHABLE)).toBeVisible();
    expect(screen.queryByRole('tablist')).not.toBeInTheDocument();
    expect(daemon.recentMemories).not.toHaveBeenCalled();
  });

  it('a read error shows an alert with Refresh', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.recentMemories).mockRejectedValueOnce(
      new DaemonHttpError(503, { error: 'Memory is busy' }),
    );
    renderPage();
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Memory is busy',
    );
    await user.click(screen.getByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull());
    expect(daemon.recentMemories).toHaveBeenCalledTimes(2);
  });

  it('a failed delete shows an alert that Refresh clears', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.recentMemories).mockResolvedValue([
      memoryFixture('m1', { content: 'Likes tea' }),
    ]);
    vi.mocked(daemon.deleteMemory).mockRejectedValue(
      new DaemonHttpError(503, { error: 'Memory is busy' }),
    );
    renderPage();
    await screen.findByText('Likes tea');
    await user.click(screen.getByRole('button', { name: 'Delete' }));
    await user.click(
      within(screen.getByRole('group', { name: 'Confirm' })).getByRole(
        'button',
        { name: 'Delete memory' },
      ),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Memory is busy',
    );

    await user.click(screen.getByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull());
  });

  it('a refused entity removal does not follow the owner to another tab', async () => {
    const user = userEvent.setup();
    vi.mocked(daemon.listMemoryEntities).mockResolvedValue([
      entityFixture('e1', { kind: 'agent', name: 'Helper' }),
    ]);
    vi.mocked(daemon.deleteMemoryEntity).mockRejectedValue(
      new DaemonHttpError(409, { error: 'Still has memories' }),
    );
    renderPage();
    await screen.findByText(MEMORY_EMPTY);
    await user.click(screen.getByRole('tab', { name: 'People & things' }));
    await user.click(await screen.findByRole('button', { name: 'Remove' }));
    await user.click(
      within(screen.getByRole('group', { name: 'Confirm' })).getByRole(
        'button',
        { name: 'Remove' },
      ),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Still has memories',
    );

    await user.click(screen.getByRole('tab', { name: 'Memories' }));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('the selected tab survives a remount through session storage', async () => {
    const user = userEvent.setup();
    const first = renderPage();
    await screen.findByText(MEMORY_EMPTY);
    await user.click(screen.getByRole('tab', { name: 'About you' }));
    expect(screen.getByRole('tab', { name: 'About you' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(sessionStorage.getItem('anima.memory.tab')).toBe('about');
    first.unmount();

    renderPage();
    expect(
      await screen.findByRole('tab', { name: 'About you' }),
    ).toHaveAttribute('aria-selected', 'true');
    expect(await screen.findByText(FACTS_EMPTY)).toBeVisible();
  });

  it('still renders when storage throws', async () => {
    const user = userEvent.setup();
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    renderPage();
    expect(await screen.findByText(MEMORY_EMPTY)).toBeVisible();
    await user.click(screen.getByRole('tab', { name: 'People & things' }));
    expect(await screen.findByText(ENTITIES_EMPTY)).toBeVisible();
  });
});
