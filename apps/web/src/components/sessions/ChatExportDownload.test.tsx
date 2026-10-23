import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { daemon } from '../../lib/daemon-api';
import {
  ChatExportDownload,
  chatExportTarget,
  ChatExportScope,
} from './ChatExportDownload';

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('chat export download', () => {
  it('never uses the owner transport for another conversation or outside a trusted conversation scope', () => {
    const read = vi.spyOn(daemon, 'exportSession');
    const { rerender } = render(
      <ChatExportScope.Provider value={{ agentId: 'a', sessionId: 'current' }}>
        <ChatExportDownload url="/api/agents/a/sessions/other/export">
          Download this chat
        </ChatExportDownload>
      </ChatExportScope.Provider>,
    );
    expect(
      screen.queryByRole('button', { name: 'Download this chat' }),
    ).not.toBeInTheDocument();
    rerender(
      <ChatExportDownload url="/api/agents/a/sessions/current/export">
        Download this chat
      </ChatExportDownload>,
    );
    expect(
      screen.queryByRole('button', { name: 'Download this chat' }),
    ).not.toBeInTheDocument();
    expect(read).not.toHaveBeenCalled();
  });
  it('saves the complete authenticated Markdown as a local download', async () => {
    const create = vi.fn((_blob: Blob | MediaSource) => 'blob:chat-export');
    const revoke = vi.fn();
    vi.stubGlobal(
      'URL',
      class extends URL {
        static createObjectURL = create;
        static revokeObjectURL = revoke;
      },
    );
    const click = vi
      .spyOn(HTMLAnchorElement.prototype, 'click')
      .mockImplementation(() => {});
    vi.spyOn(daemon, 'exportSession').mockResolvedValue(
      '# Complete transcript\n\nOlder saved messages\n',
    );
    render(
      <ChatExportScope.Provider value={{ agentId: 'a', sessionId: 's' }}>
        <ChatExportDownload
          url="/api/agents/a/sessions/s/export"
          filename="chat.md"
        />
      </ChatExportScope.Provider>,
    );
    fireEvent.click(
      screen.getByRole('button', { name: 'Download chat Markdown' }),
    );
    await waitFor(() => expect(click).toHaveBeenCalledOnce());
    expect(create.mock.calls[0][0]).toBeInstanceOf(Blob);
    expect(revoke).toHaveBeenCalledWith('blob:chat-export');
    const text = await new Promise((resolve) => {
      const reader = new FileReader();
      reader.onload = () => resolve(reader.result);
      reader.readAsText(create.mock.calls[0][0] as Blob);
    });
    expect(text).toBe('# Complete transcript\n\nOlder saved messages\n');
  });
  it('recognizes only exact relative daemon export URLs', () => {
    expect(
      chatExportTarget('/api/agents/agent%2Fa/sessions/chat%3A1/export'),
    ).toEqual({ agentId: 'agent/a', sessionId: 'chat:1' });
    for (const url of [
      'https://other.example/api/agents/a/sessions/s/export',
      '//other.example/file',
      '/api/agents/a/sessions/s/export?token=bad',
      '/api/agents/%zz/sessions/s/export',
    ])
      expect(chatExportTarget(url)).toBeNull();
  });
  it('downloads through the owner daemon client and shows a failed fetch', async () => {
    const read = vi
      .spyOn(daemon, 'exportSession')
      .mockRejectedValue(new Error('Owner session expired'));
    render(
      <ChatExportScope.Provider
        value={{ agentId: 'agent/a', sessionId: 'chat:1' }}
      >
        <ChatExportDownload url="/api/agents/agent%2Fa/sessions/chat%3A1/export">
          Download chat
        </ChatExportDownload>
      </ChatExportScope.Provider>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Download chat' }));
    await waitFor(() => expect(read).toHaveBeenCalledWith('agent/a', 'chat:1'));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Owner session expired',
    );
  });
});
