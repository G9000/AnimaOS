import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { ToolResultPage } from '@animaos/sdk';
import {
  savedToolResultId,
  ToolResultArtifact,
  type ToolResultReader,
} from './ToolResultArtifact';

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const artifact = {
  id: `tr_${'a'.repeat(64)}`,
  agentId: 'agent',
  runId: 'run',
  toolCallId: 'call',
  toolName: 'read_file',
  bytes: 20000,
  sha256: 'b'.repeat(64),
  captureComplete: false,
  createdAtMs: 1,
};
const page = (
  text: string,
  offset: number,
  nextOffset: number | null,
): ToolResultPage => ({ artifact, text, offset, nextOffset });

it('loads bounded pages on demand and safely renders retained text', async () => {
  const api: ToolResultReader = {
    read: vi
      .fn()
      .mockResolvedValueOnce(page('<script>alert(1)</script>', 0, 24))
      .mockResolvedValueOnce(page('tail', 24, null)),
    download: vi.fn(),
  };
  const view = render(
    <ToolResultArtifact agentId="agent" artifactId={artifact.id} api={api} />,
  );
  expect(api.read).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'View saved result' }));
  expect(await screen.findByText('<script>alert(1)</script>')).toBeVisible();
  expect(view.container.querySelector('script')).toBeNull();
  expect(screen.getByText(/tool reported incomplete output/i)).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: 'Next page' }));
  expect(await screen.findByText('tail')).toBeVisible();
  expect(api.read).toHaveBeenNthCalledWith(
    2,
    'agent',
    artifact.id,
    expect.objectContaining({ offset: 24, limit: 8192 }),
  );
  expect(screen.getByRole('button', { name: 'Next page' })).toBeDisabled();
});

it('shows read errors and never claims an unavailable payload is complete', async () => {
  const api: ToolResultReader = {
    read: vi.fn().mockRejectedValue(new Error('Saved result unavailable')),
    download: vi.fn(),
  };
  render(
    <ToolResultArtifact agentId="agent" artifactId={artifact.id} api={api} />,
  );
  fireEvent.click(screen.getByRole('button', { name: 'View saved result' }));
  await waitFor(() =>
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Saved result unavailable',
    ),
  );
  expect(
    screen.queryByRole('button', { name: 'Download saved result' }),
  ).not.toBeInTheDocument();
});

it('downloads the owned original JSON as a file and releases the object URL', async () => {
  const create = vi.fn().mockReturnValue('blob:saved-result');
  const revoke = vi.fn();
  const OriginalURL = URL;
  vi.stubGlobal(
    'URL',
    class extends OriginalURL {
      static createObjectURL = create;
      static revokeObjectURL = revoke;
    },
  );
  const click = vi
    .spyOn(HTMLAnchorElement.prototype, 'click')
    .mockImplementation(() => undefined);
  const blob = new Blob(['original saved payload']);
  const api: ToolResultReader = {
    read: vi.fn().mockResolvedValue(page('preview', 0, null)),
    download: vi.fn().mockResolvedValue(blob),
  };
  render(
    <ToolResultArtifact agentId="agent" artifactId={artifact.id} api={api} />,
  );
  fireEvent.click(screen.getByRole('button', { name: 'View saved result' }));
  fireEvent.click(
    await screen.findByRole('button', { name: 'Download saved result' }),
  );
  await waitFor(() => expect(click).toHaveBeenCalledOnce());
  expect(api.download).toHaveBeenCalledWith('agent', artifact.id);
  expect(create).toHaveBeenCalledWith(blob);
  expect(revoke).toHaveBeenCalledWith('blob:saved-result');
});

it('accepts only the reserved first-line header with a bounded opaque id', () => {
  const header = `[saved-tool-result id=${artifact.id} bytes=20000 capture=complete]\n`;
  expect(savedToolResultId(header + 'preview')).toBe(artifact.id);
  expect(savedToolResultId('quoted ' + header)).toBeNull();
  expect(
    savedToolResultId(header.replace(artifact.id, '../../file')),
  ).toBeNull();
  expect(
    savedToolResultId(header.replace('capture=complete', 'capture=unknown')),
  ).toBeNull();
});
