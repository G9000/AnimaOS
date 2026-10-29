import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { DaemonHttpError } from '@animaos/sdk';
import userEvent from '@testing-library/user-event';
import { PersonalityEditor } from './PersonalityEditor';

const saved = {
  source: 'workspace_soul' as const,
  path: 'SOUL.md',
  content: '# Nova\n\n## Personality\nCurious.\n\n## Custom rule\nKeep me.',
  revision: 'r1',
  maxChars: 16000,
};

describe('PersonalityEditor', () => {
  it('keeps recognized nested sections independent during sequential edits', async () => {
    const content =
      '## Communication\nBe clear.\n### Examples\nExplain simply.\n\n## Boundaries\nAsk.';
    const save = vi.fn(
      async (_: string, input: { content: string; revision: string }) => ({
        ...saved,
        ...input,
        revision: 'r2',
      }),
    );
    render(
      <PersonalityEditor initialScreen="editor"
        agentId="one"
        loadPersonality={async () => ({ ...saved, content })}
        savePersonality={save}
      />,
    );
    const communication = await screen.findByLabelText('Communication');
    expect(communication).toHaveValue('Be clear.');
    fireEvent.change(communication, {
      target: { value: 'Be clear and direct.' },
    });
    fireEvent.change(screen.getByLabelText('Example replies'), {
      target: { value: 'Use my new example.' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save personality' }));
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith('one', {
        revision: 'r1',
        content: content
          .replace('Be clear.', 'Be clear and direct.')
          .replace('Explain simply.', 'Use my new example.'),
      }),
    );
  });
  it('keeps typed spaces and newlines without accumulating document separators', async () => {
    const save = vi.fn(
      async (_: string, input: { content: string; revision: string }) => ({
        ...saved,
        ...input,
        revision: 'r2',
      }),
    );
    const user = userEvent.setup();
    render(
      <PersonalityEditor initialScreen="editor"
        agentId="one"
        loadPersonality={async () => saved}
        savePersonality={save}
      />,
    );
    const field = await screen.findByLabelText('Personality and temperament');
    await user.type(field, '\nCurious and adventurous.');
    expect(field).toHaveValue('Curious.\nCurious and adventurous.');
    await user.click(screen.getByRole('button', { name: 'Save personality' }));
    expect(save).toHaveBeenCalledWith('one', {
      revision: 'r1',
      content: saved.content.replace(
        'Curious.',
        'Curious.\nCurious and adventurous.',
      ),
    });
  });
  it('loads the active source, edits guided fields, and saves the same document', async () => {
    const save = vi.fn(
      async (_: string, input: { content: string; revision: string }) => ({
        ...saved,
        ...input,
        revision: 'r2',
      }),
    );
    render(
      <PersonalityEditor initialScreen="editor"
        agentId="one"
        loadPersonality={async () => saved}
        savePersonality={save}
      />,
    );
    expect(screen.getByRole('status')).toHaveTextContent('Loading personality');
    expect(
      await screen.findByLabelText('Personality and temperament'),
    ).toHaveValue('Curious.');
    expect(screen.getByText('Workspace personality')).toBeVisible();
    fireEvent.change(screen.getByLabelText('Personality and temperament'), {
      target: { value: 'Curious and adventurous.' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save personality' }));
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith('one', {
        revision: 'r1',
        content: saved.content.replace('Curious.', 'Curious and adventurous.'),
      }),
    );
    expect(await screen.findByText(/Personality saved/)).toBeVisible();
  });

  it('preserves raw text and unknown sections when switching views', async () => {
    render(
      <PersonalityEditor initialScreen="editor"
        agentId="one"
        loadPersonality={async () => saved}
        savePersonality={vi.fn()}
      />,
    );
    await screen.findByLabelText('Personality and temperament');
    fireEvent.click(screen.getByRole('tab', { name: 'Markdown' }));
    const raw = screen.getByLabelText('Personality document');
    fireEvent.change(raw, {
      target: {
        value: saved.content + '\n\n## Boundaries\nAsk before purchases.',
      },
    });
    fireEvent.click(screen.getByRole('tab', { name: 'Guided' }));
    expect(screen.getByLabelText('Boundaries')).toHaveValue(
      'Ask before purchases.',
    );
    fireEvent.click(screen.getByRole('tab', { name: 'Markdown' }));
    expect(screen.getByLabelText('Personality document')).toHaveValue(
      saved.content + '\n\n## Boundaries\nAsk before purchases.',
    );
  });

  it('keeps a failed draft and blocks stale saves until explicit reload', async () => {
    const load = vi.fn(async () => saved);
    const save = vi.fn(async () => {
      throw new DaemonHttpError(409, {
        error: 'Personality changed elsewhere.',
      });
    });
    render(
      <PersonalityEditor initialScreen="editor"
        agentId="one"
        loadPersonality={load}
        savePersonality={save}
      />,
    );
    fireEvent.change(
      await screen.findByLabelText('Personality and temperament'),
      { target: { value: 'My unsaved draft.' } },
    );
    fireEvent.click(screen.getByRole('button', { name: 'Save personality' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Personality changed elsewhere.',
    );
    expect(screen.getByLabelText('Personality and temperament')).toHaveValue(
      'My unsaved draft.',
    );
    expect(
      screen.getByRole('button', { name: 'Save personality' }),
    ).toBeDisabled();
    fireEvent.click(
      screen.getByRole('button', { name: 'Reload saved personality' }),
    );
    await waitFor(() =>
      expect(screen.getByLabelText('Personality and temperament')).toHaveValue(
        'Curious.',
      ),
    );
  });

  it('keeps the draft on a transient save failure and allows retry', async () => {
    const save = vi.fn(async () => {
      throw new Error('Disk unavailable');
    });
    render(
      <PersonalityEditor initialScreen="editor"
        agentId="one"
        loadPersonality={async () => saved}
        savePersonality={save}
      />,
    );
    fireEvent.change(await screen.findByLabelText('Goals and priorities'), {
      target: { value: 'Find a useful opportunity.' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save personality' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Disk unavailable',
    );
    expect(screen.getByLabelText('Goals and priorities')).toHaveValue(
      'Find a useful opportunity.',
    );
    expect(
      screen.getByRole('button', { name: 'Save personality' }),
    ).toBeEnabled();
  });

  it('ignores an old agent response after switching agents', async () => {
    let resolveOld!: (value: typeof saved) => void;
    const old = new Promise<typeof saved>((resolve) => {
      resolveOld = resolve;
    });
    const load = vi.fn((id: string) =>
      id === 'one'
        ? old
        : Promise.resolve({ ...saved, content: '## Personality\nThoughtful.' }),
    );
    const { rerender } = render(
      <PersonalityEditor initialScreen="editor"
        agentId="one"
        loadPersonality={load}
        savePersonality={vi.fn()}
      />,
    );
    rerender(
      <PersonalityEditor initialScreen="editor"
        agentId="two"
        loadPersonality={load}
        savePersonality={vi.fn()}
      />,
    );
    expect(
      await screen.findByLabelText('Personality and temperament'),
    ).toHaveValue('Thoughtful.');
    await act(async () => resolveOld(saved));
    expect(screen.getByLabelText('Personality and temperament')).toHaveValue(
      'Thoughtful.',
    );
  });

  it('offers retry after a failed load and refuses empty or oversized documents', async () => {
    const load = vi
      .fn()
      .mockRejectedValueOnce(new Error('Could not load'))
      .mockResolvedValue({ ...saved, maxChars: 20, content: 'Be curious.' });
    render(
      <PersonalityEditor initialScreen="editor"
        agentId="one"
        loadPersonality={load}
        savePersonality={vi.fn()}
      />,
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not load',
    );
    fireEvent.click(
      screen.getByRole('button', { name: 'Retry loading personality' }),
    );
    await screen.findByLabelText('Personality and temperament');
    fireEvent.click(screen.getByRole('tab', { name: 'Markdown' }));
    fireEvent.change(screen.getByLabelText('Personality document'), {
      target: { value: '' },
    });
    expect(
      screen.getByRole('button', { name: 'Save personality' }),
    ).toBeDisabled();
    fireEvent.change(screen.getByLabelText('Personality document'), {
      target: { value: 'A'.repeat(21) },
    });
    expect(
      screen.getByRole('button', { name: 'Save personality' }),
    ).toBeDisabled();
  });
});
