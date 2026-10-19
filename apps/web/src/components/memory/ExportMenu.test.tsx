import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { DaemonHttpError } from '@animaOS-SWARM/sdk';

import { daemon } from '../../lib/daemon-api';
import { ExportMenu, exportFilename } from './ExportMenu';

afterEach(() => vi.restoreAllMocks());

function stubDownload() {
  const create = vi.fn(() => 'blob:export');
  const revoke = vi.fn();
  Object.defineProperty(URL, 'createObjectURL', {
    configurable: true,
    value: create,
  });
  Object.defineProperty(URL, 'revokeObjectURL', {
    configurable: true,
    value: revoke,
  });
  const downloads: string[] = [];
  vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (
    this: HTMLAnchorElement,
  ) {
    downloads.push(`${this.download} ${this.href}`);
  });
  return { create, downloads };
}

describe('ExportMenu', () => {
  it('offers JSON or Markdown for this agent or all agents', async () => {
    const user = userEvent.setup();
    render(<ExportMenu agentId="agent-1" />);

    await user.click(screen.getByRole('button', { name: 'Export' }));
    expect(
      screen.getAllByRole('menuitem').map((item) => item.textContent),
    ).toEqual([
      'JSON, this agent',
      'Markdown, this agent',
      'JSON, all agents',
      'Markdown, all agents',
    ]);
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('downloads this agent as Markdown through the SDK blob', async () => {
    const { create, downloads } = stubDownload();
    const blob = new Blob(['# export'], { type: 'text/markdown' });
    const exportMemories = vi
      .spyOn(daemon, 'exportMemories')
      .mockResolvedValue(blob);
    const user = userEvent.setup();
    render(<ExportMenu agentId="agent-1" />);

    await user.click(screen.getByRole('button', { name: 'Export' }));
    await user.click(
      screen.getByRole('menuitem', { name: 'Markdown, this agent' }),
    );

    await waitFor(() => expect(downloads).toHaveLength(1));
    expect(exportMemories).toHaveBeenCalledWith({
      format: 'md',
      agentId: 'agent-1',
    });
    expect(create).toHaveBeenCalledWith(blob);
    expect(downloads[0]).toMatch(
      /^animaos-memory-agent-1-\d{4}-\d{2}-\d{2}\.md /,
    );
  });

  it('downloads every agent as JSON without an agent id', async () => {
    const { downloads } = stubDownload();
    const exportMemories = vi
      .spyOn(daemon, 'exportMemories')
      .mockResolvedValue(new Blob(['{}']));
    const user = userEvent.setup();
    render(<ExportMenu agentId="agent-1" />);

    await user.click(screen.getByRole('button', { name: 'Export' }));
    await user.click(
      screen.getByRole('menuitem', { name: 'JSON, all agents' }),
    );

    await waitFor(() => expect(downloads).toHaveLength(1));
    expect(exportMemories).toHaveBeenCalledWith({ format: 'json' });
    expect(downloads[0]).toMatch(
      /^animaos-memory-all-\d{4}-\d{2}-\d{2}\.json /,
    );
  });

  it('shows a refused export as an alert and downloads nothing', async () => {
    const { downloads } = stubDownload();
    vi.spyOn(daemon, 'exportMemories').mockRejectedValue(
      new DaemonHttpError(404, { error: 'not found' }),
    );
    const user = userEvent.setup();
    render(<ExportMenu agentId="agent-1" />);

    await user.click(screen.getByRole('button', { name: 'Export' }));
    await user.click(
      screen.getByRole('menuitem', { name: 'JSON, this agent' }),
    );

    expect(await screen.findByRole('alert')).toHaveTextContent('not found');
    expect(downloads).toEqual([]);
  });

  it('names files like the daemon does', () => {
    const day = new Date('2026-10-19T12:00:00Z');
    expect(exportFilename(null, 'json', day)).toBe(
      'animaos-memory-all-2026-10-19.json',
    );
    expect(exportFilename('a/b c', 'md', day)).toBe(
      'animaos-memory-a_b_c-2026-10-19.md',
    );
  });
});
