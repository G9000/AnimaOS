import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { DaemonHttpError, type AgentFilesPreview } from '@animaos/sdk';
import { AgentFilesPanel } from './AgentFilesPanel';

function preview(
  overrides: Partial<AgentFilesPreview> = {},
): AgentFilesPreview {
  return {
    mode: 'current_preview',
    availability: 'available',
    capturedAtMs: 1,
    snapshotDigest: 'a'.repeat(64),
    identitySource: 'legacy',
    projectSourceKind: null,
    estimatedTokens: 0,
    entries: [],
    ...overrides,
  };
}

describe('AgentFilesPanel', () => {
  it.each([
    ['workspace_soul', 'Workspace SOUL.md'],
    ['profile_soul', 'Agent profile SOUL.md'],
  ] as const)(
    'labels %s identity and shows only closed exclusion metadata',
    async (identitySource, label) => {
      render(
        <AgentFilesPanel
          agentId="one"
          loadPreview={async () =>
            preview({
              identitySource,
              entries: [
                {
                  relativePath: '.animaos-swarm/agent-profiles/6f6e65/SOUL.md',
                  kind: 'soul',
                  state: 'blocked',
                  rawByteCount: 10,
                  admittedCharCount: 0,
                  digest: 'd'.repeat(64),
                  truncated: false,
                  threatIds: ['override_authority'],
                  reasonCode: 'threat_detected',
                },
              ],
            })
          }
        />,
      );
      expect(
        await screen.findByText(
          new RegExp(`Identity: ${label.replace('.', '\\.')}`),
        ),
      ).toBeInTheDocument();
      expect(screen.getByText('Reason: threat_detected')).toBeInTheDocument();
      expect(
        screen.getByText('Threat IDs: override_authority'),
      ).toBeInTheDocument();
    },
  );
  it('shows current preview timing and legacy fallback without source bodies', async () => {
    render(
      <AgentFilesPanel
        agentId="one"
        loadPreview={async () => ({
          ...preview(),
          sourceText: 'PRIVATE SOURCE',
          workspaceRoot: 'C:/PRIVATE',
        })}
      />,
    );
    expect(screen.getByRole('status')).toHaveTextContent('Loading');
    expect(
      await screen.findByText(/Identity: Legacy identity/),
    ).toBeInTheDocument();
    expect(screen.getByText(/Current preview · Captured/)).toBeInTheDocument();
    expect(
      screen.getByText(/active run keeps its accepted snapshot/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/PRIVATE/)).not.toBeInTheDocument();
  });

  it('renders relative source metadata, exclusion reasons and priority', async () => {
    render(
      <AgentFilesPanel
        agentId="one"
        loadPreview={async () =>
          preview({
            projectSourceKind: 'anima',
            entries: [
              {
                relativePath: 'AGENTS.md',
                kind: 'agents',
                state: 'shadowed',
                rawByteCount: 30,
                admittedCharCount: 0,
                digest: 'b'.repeat(64),
                truncated: false,
                threatIds: [],
                reasonCode: null,
              },
              {
                relativePath: 'SOUL.md',
                kind: 'soul',
                state: 'truncated',
                rawByteCount: 90,
                admittedCharCount: 40,
                digest: 'c'.repeat(64),
                truncated: true,
                threatIds: [],
                reasonCode: null,
              },
            ],
          })
        }
      />,
    );
    expect(await screen.findByText('AGENTS.md')).toBeInTheDocument();
    expect(screen.getByText(/Project instructions: Anima/)).toBeInTheDocument();
    expect(screen.getByText(/higher-priority source/)).toBeInTheDocument();
    expect(
      screen.getByText(/40 admitted characters · truncated/),
    ).toBeInTheDocument();
  });

  it('distinguishes workspace unavailability and neutral missing preview', async () => {
    const view = render(
      <AgentFilesPanel
        agentId="one"
        loadPreview={async () =>
          preview({ availability: 'unavailable_workspace' })
        }
      />,
    );
    expect(
      await screen.findByText(/Configure a workspace/),
    ).toBeInTheDocument();
    view.rerender(
      <AgentFilesPanel
        agentId="two"
        loadPreview={async () => {
          throw new DaemonHttpError(404, {});
        }}
      />,
    );
    expect(
      await screen.findByText(/unavailable for this agent/),
    ).toBeInTheDocument();
  });

  it.each([401, 403, 500])(
    'shows a real error for HTTP %s and allows an explicit retry',
    async (status) => {
      const load = vi
        .fn()
        .mockRejectedValueOnce(
          new DaemonHttpError(status, { error: 'PRIVATE OS MESSAGE' }),
        )
        .mockResolvedValueOnce(preview());
      render(<AgentFilesPanel agentId="one" loadPreview={load} />);
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'Could not load',
      );
      expect(screen.queryByText(/PRIVATE/)).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'Retry preview' }));
      expect(
        await screen.findByText(/Identity: Legacy identity/),
      ).toBeInTheDocument();
      expect(load).toHaveBeenCalledTimes(2);
    },
  );

  it('aborts selection changes and ignores a late result even if transport ignores abort', async () => {
    let finishOld!: (value: AgentFilesPreview) => void;
    let oldSignal: AbortSignal | undefined;
    const load = vi.fn((id: string, options: { signal?: AbortSignal }) => {
      if (id === 'one') {
        oldSignal = options.signal;
        return new Promise<AgentFilesPreview>((resolve) => {
          finishOld = resolve;
        });
      }
      return Promise.resolve(preview({ snapshotDigest: 'new-preview' }));
    });
    const view = render(<AgentFilesPanel agentId="one" loadPreview={load} />);
    view.rerender(<AgentFilesPanel agentId="two" loadPreview={load} />);
    expect(oldSignal?.aborted).toBe(true);
    expect(
      await screen.findByText('Snapshot digest: new-preview'),
    ).toBeInTheDocument();
    await act(async () =>
      finishOld(preview({ snapshotDigest: 'old-preview' })),
    );
    expect(
      screen.queryByText('Snapshot digest: old-preview'),
    ).not.toBeInTheDocument();
    view.unmount();
    expect(load.mock.calls[1][1].signal?.aborted).toBe(true);
  });
});
