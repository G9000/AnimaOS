import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { BrowserPolicy, BrowserScope } from '@animaOS-SWARM/sdk';
import {
  BrowserTaskControls,
  type BrowserControlsApi,
} from './BrowserTaskControls';

const policy: BrowserPolicy = {
  id: 'quotes',
  origin: 'http://127.0.0.1:43220',
  routes: [{ id: 'home', method: 'GET', path: '/' }],
  policyDigest: 'a'.repeat(64),
  installationId: 'edge',
  installationDigest: 'b'.repeat(64),
  workerAvailable: true,
  limits: {
    actions: 40,
    requests: 80,
    scopeMs: 120000,
    frameBytes: 65536,
    wireBytes: 1048576,
    observationNodes: 128,
    observationTextBytes: 8192,
    inputBytes: 16384,
    outputBytes: 16384,
    screenshotBytes: 262144,
  },
};
const scope: BrowserScope = {
  id: 'scope-a',
  agentId: 'a',
  runId: 'run-a',
  sessionId: 'chat:a',
  version: 2,
  enabled: true,
  state: 'active',
  policyId: 'quotes',
  origin: policy.origin,
  policyDigest: policy.policyDigest,
  installationId: policy.installationId,
  installationDigest: policy.installationDigest,
  selectedInputIds: [],
  expiresAtMs: 1900000000000,
  actionsUsed: 2,
  requestsUsed: 3,
  wireBytesUsed: 900,
  physicalLeaseHeld: true,
  document: null,
  artifacts: [],
  toolNames: ['browser_navigate'],
  error: null,
};

function api(): BrowserControlsApi {
  return {
    policies: vi.fn().mockResolvedValue([policy]),
    get: vi.fn().mockResolvedValue(scope),
    stop: vi.fn().mockResolvedValue({
      ...scope,
      version: 3,
      state: 'closed',
      enabled: false,
      physicalLeaseHeld: false,
    }),
  };
}

describe('browser task controls', () => {
  it('loads metadata only on opening and selects only installed policies and ready inputs', async () => {
    const mock = api();
    const onSelection = vi.fn();
    const view = render(
      <BrowserTaskControls
        agentId="a"
        runId={null}
        disabled={false}
        attachments={[]}
        selection={null}
        onSelection={onSelection}
        api={mock}
      />,
    );
    expect(mock.policies).not.toHaveBeenCalled();
    const details = screen.getByText('Browser task').closest('details')!;
    details.open = true;
    fireEvent(details, new Event('toggle'));
    await screen.findByRole('option', { name: /quotes/ });
    fireEvent.change(screen.getByLabelText('Browser policy for next message'), {
      target: { value: 'quotes' },
    });
    expect(onSelection).toHaveBeenLastCalledWith({
      policyId: 'quotes',
      selectedInputIds: [],
    });
    expect(mock.get).not.toHaveBeenCalled();
    expect(mock.stop).not.toHaveBeenCalled();
    view.unmount();
  });

  it('uses current server CAS for Stop and retains a physical cleanup warning on conflict', async () => {
    const mock = api();
    vi.mocked(mock.stop).mockRejectedValue({
      status: 409,
      message: '/private/path',
    });
    render(
      <BrowserTaskControls
        agentId="a"
        runId="run-a"
        disabled={false}
        attachments={[]}
        selection={null}
        onSelection={vi.fn()}
        api={mock}
      />,
    );
    const details = screen.getByText('Browser task').closest('details')!;
    details.open = true;
    fireEvent(details, new Event('toggle'));
    const stop = await screen.findByRole('button', { name: 'Stop browser' });
    await waitFor(() => expect(stop).toBeEnabled());
    fireEvent.click(stop);
    await screen.findByRole('alert');
    expect(mock.stop).toHaveBeenCalledExactlyOnceWith('a', 'run-a', {
      expectedVersion: 2,
    });
    expect(
      screen.getByText(/Physical browser cleanup is pending/),
    ).toBeInTheDocument();
    expect(screen.queryByText('/private/path')).not.toBeInTheDocument();
  });

  it('ignores an old agent response after the owner changes and aborts its read', async () => {
    const mock = api();
    let resolve!: (value: BrowserPolicy[]) => void;
    vi.mocked(mock.policies).mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const props = {
      runId: null,
      disabled: false,
      attachments: [],
      selection: null,
      onSelection: vi.fn(),
      api: mock,
    };
    const view = render(<BrowserTaskControls {...props} agentId="a" />);
    const details = screen.getByText('Browser task').closest('details')!;
    details.open = true;
    fireEvent(details, new Event('toggle'));
    await waitFor(() => expect(mock.policies).toHaveBeenCalledTimes(1));
    const signal = vi.mocked(mock.policies).mock.calls[0][1]?.signal;
    view.rerender(<BrowserTaskControls {...props} agentId="b" />);
    resolve([{ ...policy, id: 'foreign-policy' }]);
    await waitFor(() => expect(signal?.aborted).toBe(true));
    expect(
      screen.queryByRole('option', { name: /foreign-policy/ }),
    ).not.toBeInTheDocument();
    expect(mock.stop).not.toHaveBeenCalled();
  });
});
