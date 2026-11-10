import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { WorkspaceContextView } from '@animaos/sdk';
import { MotherOnboarding } from './MotherOnboarding';

const ready: WorkspaceContextView = {
  revision: 'r1',
  status: 'configured',
  binding: {
    connectionId: 'c1',
    tool: 'mother_get_context',
    mode: 'companion',
    workspaceId: 'w',
    generation: 'g',
    subject: 'new',
  },
  connections: [
    {
      id: 'c1',
      label: 'M.O.T.H.E.R',
      state: 'ready',
      tool: 'mother_get_context',
    },
  ],
};
const independent: WorkspaceContextView = {
  revision: 'r0',
  status: 'independent',
  binding: null,
  connections: [],
};

describe('M.O.T.H.E.R onboarding pairing', () => {
  it('shows setup for exactly the newly created companion and does not activate an unpaired selection', async () => {
    const load = vi.fn().mockResolvedValue(independent);
    const done = vi.fn();
    render(
      <MotherOnboarding agentId="new" name="Ari" load={load} onDone={done} />,
    );
    expect(await screen.findByText('Not paired yet')).toBeVisible();
    expect(
      screen.getByRole('heading', { name: 'Connect M.O.T.H.E.R' }),
    ).toHaveFocus();
    expect(screen.getByText('mother connect animaos')).toBeVisible();
    expect(screen.getByText(/Select Ari/)).toHaveTextContent('Companion');
    expect(load).toHaveBeenCalledWith(
      'new',
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled();
    expect(done).not.toHaveBeenCalled();
  });

  it('finishes at most once with a saved matching companion connection', async () => {
    const done = vi.fn();
    render(
      <MotherOnboarding
        agentId="new"
        name="Ari"
        load={async () => ready}
        onDone={done}
      />,
    );
    const button = await screen.findByRole('button', { name: 'Continue' });
    await waitFor(() => expect(button).toBeEnabled());
    expect(screen.getByText('Saved connection ready')).toBeVisible();
    fireEvent.click(button);
    fireEvent.click(button);
    expect(done).toHaveBeenCalledOnce();
  });

  it.each([
    [
      'shared',
      { ...ready, binding: { ...ready.binding!, mode: 'shared' as const } },
    ],
    [
      'foreign subject',
      { ...ready, binding: { ...ready.binding!, subject: 'other-agent' } },
    ],
    [
      'unreviewed reader',
      {
        ...ready,
        connections: [{ ...ready.connections[0], tool: 'mother_search' }],
      },
    ],
    [
      'other connection',
      { ...ready, connections: [{ ...ready.connections[0], id: 'other' }] },
    ],
    [
      'needs review',
      {
        ...ready,
        connections: [{ ...ready.connections[0], state: 'needs_review' }],
      },
    ],
    ['inconsistent status', { ...ready, status: 'independent' as const }],
  ])('never calls %s setup ready', async (_label, view) => {
    render(
      <MotherOnboarding
        agentId="new"
        name="Ari"
        load={async () => view}
        onDone={vi.fn()}
      />,
    );
    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: 'Check connection' }),
      ).toBeEnabled(),
    );
    expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled();
    expect(
      screen.queryByText('Saved connection ready'),
    ).not.toBeInTheDocument();
  });

  it('distinguishes a bound but offline primary memory from an unpaired local agent', async () => {
    const view = {
      ...ready,
      connections: [{ ...ready.connections[0], state: 'disconnected' }],
    };
    render(
      <MotherOnboarding
        agentId="new"
        name="Ari"
        load={async () => view}
        onDone={vi.fn()}
      />,
    );
    expect(await screen.findByText('M.O.T.H.E.R is offline')).toBeVisible();
    expect(screen.getByText(/M.O.T.H.E.R memory stays offline/)).toBeVisible();
    expect(
      screen.queryByText(
        /Until paired, this companion uses its local personality and memory/,
      ),
    ).not.toBeInTheDocument();
  });

  it('clears cached readiness during a recheck and after a failed read', async () => {
    let reject!: (error: Error) => void;
    const load = vi
      .fn()
      .mockResolvedValueOnce(ready)
      .mockImplementationOnce(
        () =>
          new Promise((_resolve, fail) => {
            reject = fail;
          }),
      );
    render(
      <MotherOnboarding
        agentId="new"
        name="Ari"
        load={load}
        onDone={vi.fn()}
      />,
    );
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Continue' })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Check connection' }));
    expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled();
    await act(async () => reject(new Error('Server unavailable')));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Server unavailable',
    );
    expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled();
    expect(
      screen.queryByText('Saved connection ready'),
    ).not.toBeInTheDocument();
  });

  it('ignores a previous agent’s late response', async () => {
    let resolve!: (view: WorkspaceContextView) => void;
    const load = vi.fn((id: string) =>
      id === 'old'
        ? new Promise<WorkspaceContextView>((done) => {
            resolve = done;
          })
        : Promise.resolve(independent),
    );
    const props = { name: 'Ari', load, onDone: vi.fn() };
    const view = render(<MotherOnboarding {...props} agentId="old" />);
    view.rerender(<MotherOnboarding {...props} agentId="new" />);
    expect(await screen.findByText('Not paired yet')).toBeVisible();
    await act(async () =>
      resolve({ ...ready, binding: { ...ready.binding!, subject: 'old' } }),
    );
    expect(
      screen.queryByText('Saved connection ready'),
    ).not.toBeInTheDocument();
  });
});
