import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { daemon, type ForeignSession } from '../../lib/daemon-api';
import { ImportConversationDialog } from './ImportConversationDialog';

const claude: ForeignSession[] = [
  {
    handle: 'h1',
    source: 'claude-code',
    title: 'Fix flaky upload test',
    cwd: '/Users/ana/code/uploader',
    modifiedAt: Date.UTC(2026, 9, 17, 10),
    sizeBytes: 412 * 1024,
    foreignSessionId: 'c-1',
  },
  {
    handle: 'h2',
    source: 'claude-code',
    title: 'Add retry to the sync worker',
    cwd: '/Users/ana/code/sync',
    modifiedAt: Date.UTC(2026, 9, 16, 10),
    sizeBytes: 1_200_000,
    foreignSessionId: 'c-2',
  },
];

afterEach(() => vi.restoreAllMocks());

function stubDaemon() {
  const list = vi
    .spyOn(daemon, 'foreignSessions')
    .mockImplementation(async (source) =>
      source === 'claude-code'
        ? { sessions: claude, nextCursor: null }
        : { sessions: [], nextCursor: null },
    );
  vi.spyOn(daemon, 'foreignSessionPreview').mockImplementation(async (handle) => ({
    title: handle === 'h1' ? 'Fix flaky upload test' : 'Add retry to the sync worker',
    cwd: '/Users/ana/code/uploader',
    source: 'claude-code',
    messageCount: 86,
    messages: [
      { role: 'user', text: 'The upload test fails one run in ten.', at: null },
      { role: 'assistant', text: '[ran tool: Edit] Switched to fake timers.', at: null },
    ],
  }));
  return list;
}

describe('ImportConversationDialog', () => {
  it('lists conversations with home-relative folders and previews the selected one', async () => {
    stubDaemon();
    render(
      <ImportConversationDialog
        agentId="agent-1"
        agentName="anima"
        onClose={() => undefined}
        onImported={() => undefined}
      />,
    );
    expect(await screen.findByRole('option', { name: /Fix flaky upload test/ })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(screen.getByText('~/code/uploader')).toBeVisible();
    expect(await screen.findByText('86 messages')).toBeVisible();
    expect(screen.getByText('[ran tool: Edit] Switched to fake timers.')).toBeVisible();
    expect(screen.getByText(/Keys and passwords are masked/)).toBeVisible();
  });

  it('imports the selected conversation into the agent', async () => {
    stubDaemon();
    const importer = vi.spyOn(daemon, 'importForeignSession').mockResolvedValue({
      sessionId: 'chat:imported-1',
      title: 'Imported · Add retry to the sync worker',
      messageCount: 40,
      alreadyImported: false,
    });
    const onImported = vi.fn();
    render(
      <ImportConversationDialog
        agentId="agent-1"
        agentName="anima"
        onClose={() => undefined}
        onImported={onImported}
      />,
    );
    const user = userEvent.setup();
    await user.click(await screen.findByRole('option', { name: /Add retry to the sync worker/ }));
    await user.click(screen.getByRole('button', { name: 'Import to anima' }));
    expect(importer).toHaveBeenCalledWith('agent-1', 'h2');
    await waitFor(() =>
      expect(onImported).toHaveBeenCalledWith(
        expect.objectContaining({ sessionId: 'chat:imported-1' }),
      ),
    );
  });

  it('switches to Codex and says when there is nothing to import', async () => {
    const list = stubDaemon();
    render(
      <ImportConversationDialog
        agentId="agent-1"
        agentName="anima"
        onClose={() => undefined}
        onImported={() => undefined}
      />,
    );
    const user = userEvent.setup();
    await screen.findByRole('option', { name: /Fix flaky upload test/ });
    await user.click(screen.getByRole('tab', { name: 'Codex' }));
    expect(await screen.findByText('No Codex conversations on this computer.')).toBeVisible();
    expect(list).toHaveBeenLastCalledWith('codex');
  });
});
