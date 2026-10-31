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

const document = () => screen.getByLabelText('Personality document');

describe('PersonalityEditor', () => {
  it('loads the active source as one markdown document and saves it as written', async () => {
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
    expect(await screen.findByLabelText('Personality document')).toHaveValue(
      saved.content,
    );
    expect(screen.getByText('Workspace personality')).toBeVisible();
    // No Guided mode: there is one editor, not tabs.
    expect(screen.queryByRole('tab')).not.toBeInTheDocument();
    fireEvent.change(document(), {
      target: { value: saved.content.replace('Curious.', 'Curious and adventurous.') },
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

  it('keeps typed spaces, newlines and unknown sections exactly', async () => {
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
    const field = await screen.findByLabelText('Personality document');
    await user.type(field, '\n\n## Boundaries\nAsk before purchases.  ');
    await user.click(screen.getByRole('button', { name: 'Save personality' }));
    expect(save).toHaveBeenCalledWith('one', {
      revision: 'r1',
      content: saved.content + '\n\n## Boundaries\nAsk before purchases.  ',
    });
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
    fireEvent.change(await screen.findByLabelText('Personality document'), {
      target: { value: 'My unsaved draft.' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save personality' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Personality changed elsewhere.',
    );
    expect(document()).toHaveValue('My unsaved draft.');
    expect(
      screen.getByRole('button', { name: 'Save personality' }),
    ).toBeDisabled();
    fireEvent.click(
      screen.getByRole('button', { name: 'Reload saved personality' }),
    );
    await waitFor(() => expect(document()).toHaveValue(saved.content));
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
    fireEvent.change(await screen.findByLabelText('Personality document'), {
      target: { value: 'Find a useful opportunity.' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save personality' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Disk unavailable',
    );
    expect(document()).toHaveValue('Find a useful opportunity.');
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
    expect(await screen.findByLabelText('Personality document')).toHaveValue(
      '## Personality\nThoughtful.',
    );
    await act(async () => resolveOld(saved));
    expect(document()).toHaveValue('## Personality\nThoughtful.');
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
    await screen.findByLabelText('Personality document');
    fireEvent.change(document(), { target: { value: '' } });
    expect(
      screen.getByRole('button', { name: 'Save personality' }),
    ).toBeDisabled();
    fireEvent.change(document(), { target: { value: 'A'.repeat(21) } });
    expect(
      screen.getByRole('button', { name: 'Save personality' }),
    ).toBeDisabled();
  });
});
