import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { SkillDraft } from '@animaos/sdk';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { daemon } from '../../lib/daemon-api';
import { SessionSkillDrafts } from './SessionSkillDrafts';

function draft(id: string, sessionId: string, name: string): SkillDraft {
  return {
    id,
    slug: name.toLowerCase().replace(/\s+/g, '-'),
    name,
    description: 'Journal each step, then replay unfinished ones once.',
    body: '# Steps',
    source: 'agent',
    proposedBy: { agentId: 'main', sessionId, runId: 'run-1' },
    baseHash: null,
    currentHash: null,
    stale: false,
    fileHash: null,
    createdAtMs: 1,
    status: 'pending',
    decidedAtMs: null,
    problem: null,
  };
}

afterEach(() => vi.restoreAllMocks());

function renderDrafts(onReview = vi.fn()) {
  return render(
    <SessionSkillDrafts
      sessionId="chat:restart"
      version={1}
      epoch={1}
      enabled
      onReview={onReview}
    />,
  );
}

describe('SessionSkillDrafts', () => {
  it('offers only the skills learned in this conversation', async () => {
    vi.spyOn(daemon, 'listSkillDrafts').mockResolvedValue([
      draft('skd_1', 'chat:restart', 'Replay-safe restarts'),
      draft('skd_2', 'chat:elsewhere', 'Weekly review'),
    ]);
    renderDrafts();
    expect(
      await screen.findByRole('region', { name: 'New skill: Replay-safe restarts' }),
    ).toBeVisible();
    expect(screen.queryByText('Weekly review')).not.toBeInTheDocument();
  });

  it('keeps a skill when the owner says so', async () => {
    vi.spyOn(daemon, 'listSkillDrafts').mockResolvedValue([
      draft('skd_1', 'chat:restart', 'Replay-safe restarts'),
    ]);
    const approve = vi
      .spyOn(daemon, 'approveSkillDraft')
      .mockResolvedValue(undefined as never);
    renderDrafts();
    await userEvent.click(await screen.findByRole('button', { name: 'Keep skill' }));
    expect(approve).toHaveBeenCalledWith('skd_1', {});
    expect(await screen.findByRole('status')).toHaveTextContent(
      'Kept. anima will use it next time.',
    );
  });

  it('discards on request and hides on Not now without deciding', async () => {
    vi.spyOn(daemon, 'listSkillDrafts').mockResolvedValue([
      draft('skd_1', 'chat:restart', 'Replay-safe restarts'),
      draft('skd_3', 'chat:restart', 'Launch plan check'),
    ]);
    const reject = vi
      .spyOn(daemon, 'rejectSkillDraft')
      .mockResolvedValue(undefined as never);
    const approve = vi.spyOn(daemon, 'approveSkillDraft');
    renderDrafts();
    const first = await screen.findByRole('region', {
      name: 'New skill: Replay-safe restarts',
    });
    await userEvent.click(
      within(first).getByRole('button', { name: 'Discard' }),
    );
    expect(reject).toHaveBeenCalledWith('skd_1');
    const second = screen.getByRole('region', {
      name: 'New skill: Launch plan check',
    });
    await userEvent.click(
      within(second).getByRole('button', { name: 'Not now' }),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole('region', { name: 'New skill: Launch plan check' }),
      ).not.toBeInTheDocument(),
    );
    expect(approve).not.toHaveBeenCalled();
  });

  it('shows a failed save and keeps the choice open', async () => {
    vi.spyOn(daemon, 'listSkillDrafts').mockResolvedValue([
      draft('skd_1', 'chat:restart', 'Replay-safe restarts'),
    ]);
    vi.spyOn(daemon, 'approveSkillDraft').mockRejectedValue(
      new Error('Skills need a workspace'),
    );
    renderDrafts();
    await userEvent.click(await screen.findByRole('button', { name: 'Keep skill' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Skills need a workspace',
    );
    expect(screen.getByRole('button', { name: 'Keep skill' })).toBeEnabled();
  });
});
