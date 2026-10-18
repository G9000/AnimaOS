import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { daemon, type AgentAutonomy } from '../../lib/daemon-api';
import { AutonomyCard, denyPatternProblem } from './AutonomyCard';

const FREE: AgentAutonomy = {
  mode: 'free',
  smartCheck: false,
  denyPatterns: ['rm -rf build/*'],
  policy: { write: 'allow', exec: 'allow', network: 'allow', delegate: 'allow' },
};

afterEach(() => vi.restoreAllMocks());

function mockDaemon(initial: AgentAutonomy = FREE) {
  let current = initial;
  vi.spyOn(daemon, 'autonomy').mockImplementation(async () => current);
  const save = vi
    .spyOn(daemon, 'setAutonomy')
    .mockImplementation(async (_id, patch) => {
      current = {
        ...current,
        ...patch,
        ...(patch.mode === 'strict'
          ? {
              policy: {
                write: 'ask',
                exec: 'ask',
                network: 'allow',
                delegate: 'ask',
              },
            }
          : {}),
      };
      return current;
    });
  return save;
}

describe('AutonomyCard', () => {
  it('switches between Free and Strict and tells whoever shows the rules', async () => {
    const user = userEvent.setup();
    const save = mockDaemon();
    const onModeChanged = vi.fn();
    render(<AutonomyCard agentId="agent-1" onModeChanged={onModeChanged} />);
    const free = await screen.findByRole('radio', { name: /^Free/ });
    expect(free).toBeChecked();
    expect(screen.getByText('Recommended')).toBeVisible();
    await user.click(screen.getByRole('radio', { name: /^Strict/ }));
    await waitFor(() =>
      expect(screen.getByRole('radio', { name: /^Strict/ })).toBeChecked(),
    );
    expect(save).toHaveBeenCalledWith('agent-1', { mode: 'strict' });
    expect(onModeChanged).toHaveBeenCalledOnce();
  });

  it('shows Custom when the owner set their own rules, and reads again on a new revision', async () => {
    vi.spyOn(daemon, 'autonomy')
      .mockResolvedValueOnce(FREE)
      .mockResolvedValue({ ...FREE, mode: 'custom' });
    const { rerender } = render(<AutonomyCard agentId="agent-1" />);
    expect(await screen.findByRole('radio', { name: /^Free/ })).toBeChecked();
    expect(
      screen.queryByText("You've set your own rules below."),
    ).toBeNull();
    rerender(<AutonomyCard agentId="agent-1" revision={1} />);
    expect(
      await screen.findByText("You've set your own rules below."),
    ).toBeVisible();
    expect(screen.getByRole('radio', { name: /^Free/ })).not.toBeChecked();
    expect(screen.getByRole('radio', { name: /^Strict/ })).not.toBeChecked();
  });

  it('turns the smart check on', async () => {
    const user = userEvent.setup();
    const save = mockDaemon();
    render(<AutonomyCard agentId="agent-1" />);
    const smart = await screen.findByRole('switch', { name: 'Smart check' });
    expect(smart).toHaveAttribute('aria-checked', 'false');
    expect(
      screen.getByText(
        "Let anima's model approve risky commands it judges safe (extra model call each time).",
      ),
    ).toBeVisible();
    await user.click(smart);
    expect(save).toHaveBeenCalledWith('agent-1', { smartCheck: true });
    await waitFor(() => expect(smart).toHaveAttribute('aria-checked', 'true'));
  });

  it('adds and removes blocked commands, refusing empty, long and repeated ones', async () => {
    const user = userEvent.setup();
    const save = mockDaemon();
    render(<AutonomyCard agentId="agent-1" />);
    const blocked = await screen.findByRole('list', { name: 'Always blocked' });
    expect(within(blocked).getByText('rm -rf build/*')).toBeVisible();
    const input = screen.getByRole('textbox', { name: 'Command to block' });
    const add = screen.getByRole('button', { name: 'Add' });

    await user.type(input, '   ');
    await user.click(add);
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Type a command to block.',
    );
    await user.clear(input);
    await user.type(input, 'rm -rf build/*');
    await user.click(add);
    expect(screen.getByRole('alert')).toHaveTextContent(
      'That command is already blocked.',
    );
    expect(save).not.toHaveBeenCalled();

    await user.clear(input);
    await user.type(input, ' npm publish* ');
    await user.click(add);
    expect(save).toHaveBeenCalledWith('agent-1', {
      denyPatterns: ['rm -rf build/*', 'npm publish*'],
    });
    expect(await within(blocked).findByText('npm publish*')).toBeVisible();
    expect(input).toHaveValue('');

    await user.click(
      screen.getByRole('button', { name: 'Unblock rm -rf build/*' }),
    );
    expect(save).toHaveBeenLastCalledWith('agent-1', {
      denyPatterns: ['npm publish*'],
    });
    await waitFor(() =>
      expect(within(blocked).queryByText('rm -rf build/*')).toBeNull(),
    );
  });

  it('lists what always asks, in any mode', async () => {
    mockDaemon();
    render(<AutonomyCard agentId="agent-1" />);
    const asks = await screen.findByRole('list', {
      name: 'Always asks, in any mode',
    });
    for (const item of [
      'Sending email',
      'Calendar changes',
      'Editing AGENTS.md or CLAUDE.md',
      'Installing bundles or MCP servers',
      'Handing keys to apps',
    ])
      expect(within(asks).getByText(item)).toBeVisible();
  });

  it('says a helper follows its companion', async () => {
    vi.spyOn(daemon, 'autonomy').mockRejectedValue(
      Object.assign(new Error('conflict'), { status: 409 }),
    );
    render(<AutonomyCard agentId="helper-1" />);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      "Helpers follow their companion's autonomy.",
    );
  });
});

describe('denyPatternProblem', () => {
  it('limits length and count', () => {
    expect(denyPatternProblem('x'.repeat(201), [])).toBe(
      'Keep it to 200 characters.',
    );
    expect(denyPatternProblem('x'.repeat(200), [])).toBeNull();
    const full = Array.from({ length: 50 }, (_, index) => `cmd-${index}`);
    expect(denyPatternProblem('another', full)).toBe(
      'You can block up to 50 commands.',
    );
    expect(denyPatternProblem('a\nb', [])).toBe('Keep it to one line.');
  });
});
