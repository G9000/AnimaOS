import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { DaemonCapabilities, ImageOperation, ImageVersion, SessionImages } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { WorkspaceArtifactLayout } from '../artifacts/WorkspaceArtifactPreview';
import { ImageGallery } from './ImageGallery';

const makeVersion = (id: string, parentAttachmentId: string | null = null): ImageVersion => ({ id, attachment: { id: `att-${id}`, name: `${id}.png`, mime: 'image/png', size: 20, kind: 'image', path: `attachments/${id}.png`, createdAtMs: 2 }, parentAttachmentId, width: 1024, height: 1024, model: 'gpt-image-2.5-sunburst', createdAtMs: 2 });
const first = makeVersion('first');
const second = makeVersion('second', first.attachment.id);
const page = (versions = [first, second], clearGeneration = 0): SessionImages => ({ clearGeneration, provider: 'openai', model: 'gpt-image-2.5-sunburst', operations: versions.map((version) => ({ id: `op-${version.id}`, prompt: `Prompt ${version.id}`, sourceAttachmentId: version.parentAttachmentId, createdAtMs: 1, status: 'succeeded', version })) });
function layout(sessionId = 'chat:one', clearGeneration = 0, refreshKey = 0, useSaved = vi.fn().mockReturnValue(true), knownImageWork = false) {
  return <WorkspaceArtifactLayout agentId="a" sessionId={sessionId} clearGeneration={clearGeneration} onUseSavedImage={useSaved}>
    <textarea data-workspace-composer aria-label="Message" />
    <ImageGallery agentId="a" sessionId={sessionId} clearGeneration={clearGeneration} refreshKey={refreshKey} knownImageWork={knownImageWork} enabled />
  </WorkspaceArtifactLayout>;
}
const inventory = (supported = true) => ({ schemaVersion: 1, tools: [], persistence: { controlPlane: 'sqlite', memory: 'sqlite', executionJournal: true }, extensions: [], limitations: [], ...(supported ? { runtime: { images: { ledgerVersion: 1, observationVersion: 1 } } } : {}) }) as DaemonCapabilities;
beforeEach(() => { vi.spyOn(daemon, 'capabilities').mockResolvedValue(inventory()); });

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
  await waitFor(() => expect(list).toHaveBeenCalledOnce());
  rerender(layout('chat:two', 1));
  expect(await screen.findByRole('button', { name: 'Open image version 1' })).toHaveTextContent('second.png');
  await act(async () => resolveOld(page()));
  expect(screen.queryByText('first.png')).toBeNull();
  list.mockResolvedValue(page([first], 0));
  rerender(layout('chat:two', 1, 1));
  await waitFor(() => expect(list).toHaveBeenCalledTimes(3));
  expect(screen.queryByText('first.png')).toBeNull();
  expect(screen.getByRole('button', { name: 'Open image version 1' })).toHaveTextContent('second.png');
  expect(screen.getByText(/Image versions could not be loaded/)).toBeVisible();
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

it('does not request unsupported image history or show an error in an unused chat', async () => {
  vi.mocked(daemon.capabilities).mockResolvedValue(inventory(false));
  const list = vi.spyOn(daemon.images, 'list').mockRejectedValue(new Error('404'));
  render(layout());
  await act(async () => {});
  expect(list).not.toHaveBeenCalled();
  expect(screen.queryByRole('region', { name: 'Chat images' })).toBeNull();
  expect(screen.queryByRole('status')).toBeNull();
});

it('explains unsupported history when a native image request is known and rechecks support on refresh', async () => {
  vi.mocked(daemon.capabilities).mockResolvedValueOnce(inventory(false)).mockResolvedValue(inventory());
  const list = vi.spyOn(daemon.images, 'list').mockResolvedValue(page());
  render(layout('chat:one', 0, 0, vi.fn(), true));
  expect(await screen.findByText(/running backend does not advertise image history/)).toBeVisible();
  expect(list).not.toHaveBeenCalled();
  expect(screen.queryByRole('region', { name: 'Chat images' })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Refresh images' }));
  expect(await screen.findByRole('button', { name: 'Open image version 1' })).toBeVisible();
  expect(daemon.capabilities).toHaveBeenCalledTimes(2);
});

it('keeps a current empty ledger quiet', async () => {
  const list = vi.spyOn(daemon.images, 'list').mockResolvedValue(page([]));
  render(layout());
  await waitFor(() => expect(list).toHaveBeenCalledOnce());
  expect(screen.queryByRole('region', { name: 'Chat images' })).toBeNull();
  expect(screen.queryByRole('status')).toBeNull();
});

it.each(['404', '403', 'network unavailable'])('shows a compact failure for known image work when supported history fails with %s', async (message) => {
  vi.spyOn(daemon.images, 'list').mockRejectedValue(new Error(message));
  render(layout('chat:one', 0, 0, vi.fn(), true));
  expect(await screen.findByText(/Image versions could not be loaded/)).toBeVisible();
  expect(screen.queryByRole('region', { name: 'Chat images' })).toBeNull();
});

it('retains saved versions after a failed refresh without resending image work', async () => {
  const list = vi.spyOn(daemon.images, 'list').mockResolvedValueOnce(page()).mockRejectedValue(new Error('403'));
  render(layout());
  await screen.findByRole('button', { name: 'Open image version 1' });
  fireEvent.click(screen.getByRole('button', { name: 'Refresh images' }));
  expect(await screen.findByText(/Image versions could not be loaded/)).toBeVisible();
  expect(screen.getByRole('button', { name: 'Open image version 1' })).toHaveTextContent('first.png');
  expect(list).toHaveBeenCalledTimes(2);
});



it('discards capability responses from the previous conversation', async () => {
  let resolveOld!: (value: DaemonCapabilities) => void;
  vi.mocked(daemon.capabilities).mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; })).mockResolvedValue(inventory(false));
  const list = vi.spyOn(daemon.images, 'list').mockResolvedValue(page());
  const { rerender } = render(layout('chat:one', 0, 0, vi.fn(), true));
  rerender(layout('chat:two', 0, 0, vi.fn(), true));
  await screen.findByText(/running backend does not advertise image history/);
  await act(async () => resolveOld(inventory()));
  expect(list).not.toHaveBeenCalled();
  expect(screen.queryByRole('button', { name: 'Open image version 1' })).toBeNull();
});

it.each([
  ['bytes_matched', 'Saved bytes matched', { sha256: 'a'.repeat(64), byteLength: 20 }],
  ['unavailable', 'Saved image bytes could not be verified', { reason: 'bytes_unavailable' }],
  ['not_checked', 'Saved image bytes were not checked within this read limit', { reason: 'budget_exceeded' }],
  ['not_checked', 'Saved image bytes were not checked because the readers are busy', { reason: 'reader_busy' }],
] as const)('shows the bounded %s observation without claiming visual verification', async (state, expected, detail) => {
  const result = page([first]);
  result.operations[0].observation = {
    state, operationId: result.operations[0].id, versionId: first.id, attachmentId: first.attachment.id,
    observedAtMs: 1700000000000, limit: 'saved_bytes_only', ...detail,
  } as ImageOperation['observation'];
  vi.spyOn(daemon.images, 'list').mockResolvedValue(result);
  render(layout());
  expect(await screen.findByText(new RegExp(expected))).toBeVisible();
  expect(screen.getByText(/Visual content has not been verified/)).toBeVisible();
});

it('keeps legacy saved metadata visibly unobserved', async () => {
  vi.spyOn(daemon.images, 'list').mockResolvedValue(page([first]));
  render(layout());
  expect(await screen.findByText(/Saved image bytes have not been checked by this backend/)).toBeVisible();
});
