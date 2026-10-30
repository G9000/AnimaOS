import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { ImageVersion, SessionImages } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { WorkspaceArtifactLayout } from '../artifacts/WorkspaceArtifactPreview';
import { ImageGallery } from './ImageGallery';

const makeVersion = (id: string, parentAttachmentId: string | null = null): ImageVersion => ({ id, attachment: { id: `att-${id}`, name: `${id}.png`, mime: 'image/png', size: 20, kind: 'image', path: `attachments/${id}.png`, createdAtMs: 2 }, parentAttachmentId, width: 1024, height: 1024, model: 'gpt-image-2.5-sunburst', createdAtMs: 2 });
const first = makeVersion('first');
const second = makeVersion('second', first.attachment.id);
const page = (versions = [first, second], clearGeneration = 0): SessionImages => ({ clearGeneration, provider: 'openai', model: 'gpt-image-2.5-sunburst', operations: versions.map((version) => ({ id: `op-${version.id}`, prompt: `Prompt ${version.id}`, sourceAttachmentId: version.parentAttachmentId, createdAtMs: 1, status: 'succeeded', version })) });
function layout(sessionId = 'chat:one', clearGeneration = 0, refreshKey = 0, useSaved = vi.fn().mockReturnValue(true)) {
  return <WorkspaceArtifactLayout agentId="a" sessionId={sessionId} clearGeneration={clearGeneration} onUseSavedImage={useSaved}>
    <textarea data-workspace-composer aria-label="Message" />
    <ImageGallery agentId="a" sessionId={sessionId} clearGeneration={clearGeneration} refreshKey={refreshKey} enabled />
  </WorkspaceArtifactLayout>;
}
afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); });

it('opens immutable image versions in the shared preview shell without an HTML iframe', async () => {
  vi.spyOn(daemon.images, 'list').mockResolvedValue(page());
  const download = vi.spyOn(daemon, 'downloadAttachment').mockResolvedValue(new Blob(['png'], { type: 'image/png' }));
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:image');
  const revoke = vi.spyOn(URL, 'revokeObjectURL');
  const useSaved = vi.fn().mockReturnValue(true);
  const { unmount } = render(layout('chat:one', 0, 0, useSaved));
  fireEvent.click(await screen.findByRole('button', { name: 'Open image version 1' }));
  expect(await screen.findByRole('img', { name: 'first.png' })).toHaveAttribute('src', 'blob:image');
  expect(download).toHaveBeenCalledWith('a', first.attachment.id, { signal: expect.any(AbortSignal) });
  expect(document.querySelector('iframe')).toBeNull();
  expect(screen.getByRole('separator', { name: 'Resize chat and artifact preview' })).toBeVisible();
  expect(screen.getByRole('link', { name: 'Download PNG' })).toHaveAttribute('download', first.attachment.name);
  fireEvent.click(screen.getByRole('button', { name: 'Next image version' }));
  expect(await screen.findByRole('img', { name: 'second.png' })).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: 'Edit in chat' }));
  expect(useSaved).toHaveBeenCalledWith(second.attachment);
  unmount();
  expect(revoke).toHaveBeenCalledWith('blob:image');
});

it('keeps the image preview open and explains a failed composer selection', async () => {
  vi.spyOn(daemon.images, 'list').mockResolvedValue(page());
  vi.spyOn(daemon, 'downloadAttachment').mockResolvedValue(new Blob(['png'], { type: 'image/png' }));
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:image');
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
  render(layout('chat:one', 0, 0, vi.fn().mockReturnValue(false)));
  fireEvent.click(await screen.findByRole('button', { name: 'Open image version 1' }));
  await screen.findByRole('img', { name: 'first.png' });
  fireEvent.click(screen.getByRole('button', { name: 'Edit in chat' }));
  expect(screen.getByRole('alert')).toHaveTextContent('other image attachments');
  expect(screen.getByRole('img', { name: 'first.png' })).toBeVisible();
});

it('discards a late preview download when the conversation is cleared', async () => {
  vi.spyOn(daemon.images, 'list').mockResolvedValue(page());
  let finish!: (blob: Blob) => void;
  vi.spyOn(daemon, 'downloadAttachment').mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  const create = vi.spyOn(URL, 'createObjectURL');
  const { rerender } = render(layout());
  fireEvent.click(await screen.findByRole('button', { name: 'Open image version 1' }));
  expect(screen.getByText('Opening image…')).toBeVisible();
  vi.mocked(daemon.images.list).mockResolvedValue(page([], 1));
  rerender(layout('chat:one', 1));
  await act(async () => finish(new Blob(['old'], { type: 'image/png' })));
  expect(create).not.toHaveBeenCalled();
  expect(screen.queryByRole('complementary', { name: 'Artifact preview' })).toBeNull();
});

it('ignores old session loads and clear generations and refreshes the current ledger', async () => {
  let resolveOld!: (value: SessionImages) => void;
  const list = vi.spyOn(daemon.images, 'list').mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; })).mockResolvedValue(page([second], 1));
  const { rerender } = render(layout());
  rerender(layout('chat:two', 1));
  expect(await screen.findByRole('button', { name: 'Open image version 1' })).toHaveTextContent('second.png');
  await act(async () => resolveOld(page()));
  expect(screen.queryByText('first.png')).toBeNull();
  list.mockResolvedValue(page([first], 0));
  rerender(layout('chat:two', 1, 1));
  await waitFor(() => expect(list).toHaveBeenCalledTimes(3));
  expect(screen.queryByText('first.png')).toBeNull();
  expect(screen.queryByRole('button', { name: 'Open image version 1' })).toBeNull();
});

it('polls only dispatching operations and shows fixed unknown outcome and charge text', async () => {
  vi.useFakeTimers();
  const pending: SessionImages = { ...page([]), operations: [{ id: 'pending', prompt: 'A bird', sourceAttachmentId: null, createdAtMs: 1, status: 'dispatching', version: null }] };
  const list = vi.spyOn(daemon.images, 'list').mockResolvedValueOnce(pending).mockResolvedValue({ ...pending, operations: [{ ...pending.operations[0], status: 'outcome_unknown' }] });
  render(layout());
  await act(async () => {});
  expect(screen.getByText('Creating image…')).toBeVisible();
  await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
  expect(list).toHaveBeenCalledTimes(2);
  expect(screen.getByText(/outcome is unknown/)).toHaveTextContent('provider may charge');
  await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
  expect(list).toHaveBeenCalledTimes(2);
});
