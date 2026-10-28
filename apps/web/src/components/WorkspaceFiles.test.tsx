import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { daemon } from '../lib/daemon-api';
import { WorkspaceFiles } from './WorkspaceFiles';

afterEach(() => vi.restoreAllMocks());
const files = [
  {
    path: 'notes/brief.md',
    name: 'brief.md',
    sizeBytes: 120,
    modifiedAtMs: null,
  },
  { path: 'plan.txt', name: 'plan.txt', sizeBytes: 2000, modifiedAtMs: 1000 },
];

it('shows receipt-backed Office content in pages and refuses a changed fingerprint', async () => {
  const path = 'reports/edited.xlsx';
  const source = 'a'.repeat(64);
  const page = { version: 1 as const, format: 'xlsx' as const, source_sha256: source, source: { kind: 'workspace', display_name: path }, targets: [{ id: 'cell1', label: 'Total', editable: true, value: { kind: 'text' as const, text: '<script>safe</script>' } }], coverage: { total_targets: 2, returned_targets: 1, complete: false }, retained_hyperlink_count: 1, limitations: ['Layout is not rendered'], next_cursor: 'next' };
  vi.spyOn(daemon, 'listWorkspaceFiles').mockResolvedValue({ files: [{ path, name: 'edited.xlsx', sizeBytes: 100, modifiedAtMs: null }], truncated: false });
  const read = vi.spyOn(daemon, 'readWorkspaceFile').mockResolvedValueOnce({ path, content: '', truncated: true, previewKind: 'office_content', sha256: source, officeContent: page }).mockResolvedValueOnce({ path, content: '', truncated: false, previewKind: 'office_content', sha256: 'b'.repeat(64), officeContent: { ...page, source_sha256: 'b'.repeat(64), targets: [{ ...page.targets[0], id: 'cell2', label: 'Changed data' }], coverage: { total_targets: 2, returned_targets: 1, complete: true }, next_cursor: null } });
  const view = render(<WorkspaceFiles online />);
  fireEvent.click(await screen.findByRole('button', { name: /reports\/edited.xlsx/ }));
  expect(await screen.findByText(/<script>safe<\/script>/)).toBeVisible();
  expect(view.container.querySelector('script')).toBeNull();
  expect(screen.getByText('Layout is not rendered')).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: 'Load more content' }));
  expect(await screen.findByRole('alert')).toHaveTextContent(/changed/i);
  expect(screen.queryByText('Changed data')).toBeNull();
  expect(read).toHaveBeenCalledWith(path, expect.objectContaining({ cursor: 'next', signal: expect.any(AbortSignal) }));
});

it('loads the next receipt-backed Office page without replacing the first', async () => {
  const path = 'report.docx'; const sha256 = 'a'.repeat(64);
  const page = { version: 1 as const, format: 'docx' as const, source_sha256: sha256, source: { kind: 'workspace', display_name: path }, targets: [{ id: 'one', label: 'First', editable: true, value: { kind: 'text' as const, text: 'Page one' } }], coverage: { total_targets: 2, returned_targets: 1, complete: false }, retained_hyperlink_count: 0, limitations: [], next_cursor: 'cursor' };
  vi.spyOn(daemon, 'listWorkspaceFiles').mockResolvedValue({ files: [{ path, name: path, sizeBytes: 100, modifiedAtMs: null }], truncated: false });
  const read = vi.spyOn(daemon, 'readWorkspaceFile').mockResolvedValueOnce({ path, content: '', truncated: true, previewKind: 'office_content', sha256, officeContent: page }).mockResolvedValueOnce({ path, content: '', truncated: false, previewKind: 'office_content', sha256, officeContent: { ...page, targets: [{ ...page.targets[0], id: 'two', label: 'Second', value: { kind: 'text', text: 'Page two' } }], coverage: { total_targets: 2, returned_targets: 1, complete: true }, next_cursor: null } });
  render(<WorkspaceFiles online />);
  fireEvent.click(await screen.findByRole('button', { name: /report.docx/ }));
  expect(await screen.findByText(/Page one/)).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: 'Load more content' }));
  expect(await screen.findByText(/Page two/)).toBeVisible();
  expect(screen.getByText(/Page one/)).toBeVisible();
  expect(screen.getByText(/2 of 2 content items shown/)).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Load more content' })).toBeNull();
  expect(read).toHaveBeenCalledTimes(2);
});

it('retires a pending Office page when a different file is selected', async () => {
  const path = 'report.docx'; const sha256 = 'a'.repeat(64);
  const page = { version: 1 as const, format: 'docx' as const, source_sha256: sha256, source: { kind: 'workspace', display_name: path }, targets: [{ id: 'one', label: 'First', editable: true, value: { kind: 'text' as const, text: 'Page one' } }], coverage: { total_targets: 2, returned_targets: 1, complete: false }, retained_hyperlink_count: 0, limitations: [], next_cursor: 'cursor' };
  vi.spyOn(daemon, 'listWorkspaceFiles').mockResolvedValue({ files: [{ path, name: path, sizeBytes: 100, modifiedAtMs: null }, files[1]], truncated: false });
  let finish!: (value: Awaited<ReturnType<typeof daemon.readWorkspaceFile>>) => void;
  const read = vi.spyOn(daemon, 'readWorkspaceFile').mockImplementation((requested, options) => {
    if (requested === 'plan.txt') return Promise.resolve({ path: requested, content: 'Other file', truncated: false });
    if (options?.cursor) return new Promise((resolve) => { finish = resolve; });
    return Promise.resolve({ path, content: '', truncated: true, previewKind: 'office_content', sha256, officeContent: page });
  });
  render(<WorkspaceFiles online />);
  fireEvent.click(await screen.findByRole('button', { name: /report.docx/ }));
  expect(await screen.findByText(/Page one/)).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: 'Load more content' }));
  const signal = read.mock.calls[1][1]?.signal;
  fireEvent.click(screen.getByRole('button', { name: /plan.txt/ }));
  expect(await screen.findByText('Other file')).toBeVisible();
  expect(signal?.aborted).toBe(true);
  await act(async () => finish({ path, content: '', truncated: false, previewKind: 'office_content', sha256, officeContent: { ...page, targets: [{ ...page.targets[0], label: 'Late page' }], coverage: { total_targets: 2, returned_targets: 1, complete: true }, next_cursor: null } }));
  expect(screen.queryByText('Late page')).toBeNull();
});

it.each(['docx', 'xlsx', 'pptx', 'pdf'])(
  'shows the generated %s content preview and downloads only on request',
  async (extension) => {
    const path = `reports/finished.${extension}`;
    vi.spyOn(daemon, 'listWorkspaceFiles').mockResolvedValue({
      files: [
        {
          path,
          name: `finished.${extension}`,
          sizeBytes: 800000,
          modifiedAtMs: null,
        },
      ],
      truncated: false,
    });
    vi.spyOn(daemon, 'readWorkspaceFile').mockResolvedValue({
      path,
      content:
        '# Quarterly report\n\n| Region | Total |\n| --- | --- |\n| North | 42 |',
      truncated: false,
      previewKind: 'document',
      sha256: 'a'.repeat(64),
    });
    const download = vi
      .spyOn(daemon, 'downloadWorkspaceFile')
      .mockResolvedValue(
        new Blob(['finished file'], { type: 'application/octet-stream' }),
      );
    vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {});
    render(<WorkspaceFiles online />);
    fireEvent.click(
      await screen.findByRole('button', {
        name: new RegExp(`reports/finished\\.${extension}`),
      }),
    );
    expect(
      await screen.findByRole('heading', { name: 'Quarterly report' }),
    ).toBeVisible();
    expect(screen.getByRole('table')).toHaveTextContent('North42');
    expect(screen.getByText('Content preview')).toBeVisible();
    expect(screen.getByText(/open the downloaded file.*layout/i)).toBeVisible();
    expect(download).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole('button', { name: 'Download current file' }),
    );
    await waitFor(() =>
      expect(download).toHaveBeenCalledWith(path, expect.any(AbortSignal)),
    );
  },
);

it('keeps document preview content inert and does not load external images', async () => {
  const path = 'reports/preview.docx';
  vi.spyOn(daemon, 'listWorkspaceFiles').mockResolvedValue({
    files: [{ path, name: 'preview.docx', sizeBytes: 500, modifiedAtMs: null }],
    truncated: false,
  });
  vi.spyOn(daemon, 'readWorkspaceFile').mockResolvedValue({
    path,
    content:
      '# Preview\n\n<script>danger()</script>\n\n![tracking](https://example.invalid/pixel.png)',
    truncated: false,
    previewKind: 'document',
  });
  const view = render(<WorkspaceFiles online />);
  fireEvent.click(
    await screen.findByRole('button', { name: /reports\/preview.docx/ }),
  );
  expect(await screen.findByRole('heading', { name: 'Preview' })).toBeVisible();
  expect(view.container.querySelector('script')).toBeNull();
  expect(view.container.querySelector('img')).toBeNull();
  expect(screen.getByText('[Image: tracking]')).toBeVisible();
});

it('searches real paths and renders escaped, read-only truncated previews', async () => {
  vi.spyOn(daemon, 'listWorkspaceFiles').mockResolvedValue({
    files,
    truncated: true,
  });
  const read = vi.spyOn(daemon, 'readWorkspaceFile').mockResolvedValue({
    path: 'notes/brief.md',
    content: '<script>danger</script>',
    truncated: true,
  });
  render(<WorkspaceFiles online />);
  expect(
    await screen.findByRole('button', { name: /notes\/brief.md/ }),
  ).toBeVisible();
  fireEvent.change(screen.getByRole('searchbox'), {
    target: { value: 'notes/' },
  });
  expect(
    screen.queryByRole('button', { name: /plan.txt/ }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: /notes\/brief.md/ }));
  expect(await screen.findByText('<script>danger</script>')).toBeVisible();
  expect(read).toHaveBeenCalledWith('notes/brief.md');
  expect(screen.getByText(/preview is truncated/i)).toBeVisible();
  expect(screen.getByText(/file list is truncated/i)).toBeVisible();
});

it('ignores a late preview after selecting another file', async () => {
  vi.spyOn(daemon, 'listWorkspaceFiles').mockResolvedValue({
    files,
    truncated: false,
  });
  let finish!: (value: {
    path: string;
    content: string;
    truncated: boolean;
  }) => void;
  vi.spyOn(daemon, 'readWorkspaceFile').mockImplementation((path) =>
    path === 'plan.txt'
      ? Promise.resolve({ path, content: 'New preview', truncated: false })
      : new Promise((resolve) => {
          finish = resolve;
        }),
  );
  render(<WorkspaceFiles online />);
  fireEvent.click(
    await screen.findByRole('button', { name: /notes\/brief.md/ }),
  );
  fireEvent.click(screen.getByRole('button', { name: /plan.txt/ }));
  expect(await screen.findByText('New preview')).toBeVisible();
  await act(async () =>
    finish({
      path: 'notes/brief.md',
      content: 'Old preview',
      truncated: false,
    }),
  );
  expect(screen.queryByText('Old preview')).not.toBeInTheDocument();
});

it('avoids requests offline and recovers from listing failure', async () => {
  const list = vi
    .spyOn(daemon, 'listWorkspaceFiles')
    .mockRejectedValueOnce(new Error('Unavailable'))
    .mockResolvedValue({ files: [], truncated: false });
  const { rerender } = render(<WorkspaceFiles online={false} />);
  expect(screen.getByText(/connect to the daemon/i)).toBeVisible();
  expect(list).not.toHaveBeenCalled();
  rerender(<WorkspaceFiles online />);
  expect(await screen.findByRole('alert')).toHaveTextContent('Unavailable');
  fireEvent.click(screen.getByRole('button', { name: 'Refresh files' }));
  expect(await screen.findByText('No workspace files yet.')).toBeVisible();
});

it('m68_downloads_a_selected_report_only_after_the_owner_clicks', async () => {
  vi.spyOn(daemon, 'listWorkspaceFiles').mockResolvedValue({
    files: [
      {
        path: 'reports/annual 2026.csv',
        name: 'annual 2026.csv',
        sizeBytes: 42,
        modifiedAtMs: null,
      },
    ],
    truncated: false,
  });
  vi.spyOn(daemon, 'readWorkspaceFile').mockResolvedValue({
    path: 'reports/annual 2026.csv',
    content: 'month,total\nMay,42\n',
    truncated: false,
  });
  const download = vi.spyOn(daemon, 'downloadWorkspaceFile').mockResolvedValue(
    new Blob(['month,total\r\nMay,42\r\n'], {
      type: 'application/octet-stream',
    }),
  );
  render(<WorkspaceFiles online />);

  const file = await screen.findByRole('button', {
    name: /reports\/annual 2026\.csv/,
  });
  expect(download).not.toHaveBeenCalled();
  fireEvent.click(file);
  expect(await screen.findByText(/month,total/)).toBeVisible();
  expect(download).not.toHaveBeenCalled();

  const downloadButton = screen.getByRole('button', {
    name: 'Download current file',
  });
  expect(downloadButton).toBeEnabled();
  fireEvent.click(downloadButton);
  expect(download).toHaveBeenCalledWith(
    'reports/annual 2026.csv',
    expect.any(AbortSignal),
  );
});

it('m68_retires_a_late_download_when_the_owner_selects_another_file', async () => {
  vi.spyOn(daemon, 'listWorkspaceFiles').mockResolvedValue({
    files: [
      { ...files[1], path: 'reports/first.csv', name: 'first.csv' },
      { ...files[1], path: 'reports/second.md', name: 'second.md' },
      { ...files[1], path: 'notes/private.xlsx', name: 'private.xlsx' },
    ],
    truncated: false,
  });
  vi.spyOn(daemon, 'readWorkspaceFile').mockImplementation(async (path) => ({
    path,
    content: `Preview for ${path}`,
    truncated: false,
  }));
  let finish!: (blob: Blob) => void;
  const download = vi.spyOn(daemon, 'downloadWorkspaceFile').mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const create = vi
    .spyOn(URL, 'createObjectURL')
    .mockReturnValue('blob:m68-stale');
  render(<WorkspaceFiles online />);

  fireEvent.click(
    await screen.findByRole('button', { name: /reports\/first\.csv/ }),
  );
  fireEvent.click(
    screen.getByRole('button', { name: 'Download current file' }),
  );
  expect(
    await screen.findByRole('button', { name: 'Downloading…' }),
  ).toBeDisabled();
  const signal = download.mock.calls[0][1]!;
  fireEvent.click(screen.getByRole('button', { name: /reports\/second\.md/ }));
  expect(signal.aborted).toBe(true);

  await act(async () => {
    finish(new Blob(['stale bytes'], { type: 'application/octet-stream' }));
  });
  expect(create).not.toHaveBeenCalled();
  expect(screen.getByText('Preview for reports/second.md')).toBeVisible();
});

it('m68_keeps_download_errors_separate_from_preview_and_allows_retry', async () => {
  vi.spyOn(daemon, 'listWorkspaceFiles').mockResolvedValue({
    files: [{ ...files[1], path: 'reports/retry.csv', name: 'retry.csv' }],
    truncated: false,
  });
  vi.spyOn(daemon, 'readWorkspaceFile').mockResolvedValue({
    path: 'reports/retry.csv',
    content: 'Preview is still available',
    truncated: false,
  });
  const download = vi
    .spyOn(daemon, 'downloadWorkspaceFile')
    .mockRejectedValueOnce(new Error('Download unavailable'))
    .mockResolvedValueOnce(new Blob(['report']));
  render(<WorkspaceFiles online />);

  fireEvent.click(
    await screen.findByRole('button', { name: /reports\/retry\.csv/ }),
  );
  expect(await screen.findByText('Preview is still available')).toBeVisible();
  fireEvent.click(
    screen.getByRole('button', { name: 'Download current file' }),
  );
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Download unavailable',
  );
  expect(screen.getByText('Preview is still available')).toBeVisible();

  fireEvent.click(screen.getByRole('button', { name: 'Retry download' }));
  await act(async () => {});
  expect(download).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

it('m68_aborts_and_ignores_a_download_that_finishes_after_going_offline', async () => {
  vi.spyOn(daemon, 'listWorkspaceFiles').mockResolvedValue({
    files: [{ ...files[1], path: 'reports/offline.md', name: 'offline.md' }],
    truncated: false,
  });
  vi.spyOn(daemon, 'readWorkspaceFile').mockResolvedValue({
    path: 'reports/offline.md',
    content: 'Offline report preview',
    truncated: false,
  });
  let finish!: (blob: Blob) => void;
  const download = vi.spyOn(daemon, 'downloadWorkspaceFile').mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const create = vi
    .spyOn(URL, 'createObjectURL')
    .mockReturnValue('blob:m68-offline');
  const view = render(<WorkspaceFiles online />);

  fireEvent.click(
    await screen.findByRole('button', { name: /reports\/offline\.md/ }),
  );
  fireEvent.click(
    screen.getByRole('button', { name: 'Download current file' }),
  );
  const signal = download.mock.calls[0][1]!;
  view.rerender(<WorkspaceFiles online={false} />);
  expect(signal.aborted).toBe(true);

  await act(async () => {
    finish(new Blob(['late bytes'], { type: 'application/octet-stream' }));
  });
  expect(create).not.toHaveBeenCalled();
  expect(screen.getByText(/connect to the daemon/i)).toBeVisible();
});

it('m68_downloads_independently_of_preview_and_retires_its_object_url', async () => {
  vi.spyOn(daemon, 'listWorkspaceFiles').mockResolvedValue({
    files: [{ ...files[1], path: 'reports/plan.txt', name: 'plan.txt' }],
    truncated: false,
  });
  vi.spyOn(daemon, 'readWorkspaceFile').mockRejectedValue(
    new Error('Preview unavailable'),
  );
  vi.spyOn(daemon, 'downloadWorkspaceFile').mockResolvedValue(
    new Blob(['raw\r\nreport'], { type: 'application/octet-stream' }),
  );
  const create = vi
    .spyOn(URL, 'createObjectURL')
    .mockReturnValue('blob:m68-report');
  const revoke = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
  const click = vi
    .spyOn(HTMLAnchorElement.prototype, 'click')
    .mockImplementation(() => {});
  render(<WorkspaceFiles online />);

  fireEvent.click(
    await screen.findByRole('button', { name: /reports\/plan\.txt/ }),
  );
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Preview unavailable',
  );
  fireEvent.click(
    screen.getByRole('button', { name: 'Download current file' }),
  );
  await waitFor(() => expect(revoke).toHaveBeenCalledWith('blob:m68-report'));

  expect(create).toHaveBeenCalledWith(expect.any(Blob));
  expect(click).toHaveBeenCalledOnce();
  expect(click.mock.instances[0]).toHaveProperty('download', 'plan.txt');
  expect(revoke).toHaveBeenCalledWith('blob:m68-report');
});
