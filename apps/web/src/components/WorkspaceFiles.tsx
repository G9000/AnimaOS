import { useCallback, useEffect, useRef, useState } from 'react';
import { useResetOnChange } from '../hooks/useResetOnChange';
import type {
  WorkspaceFileResponse,
  WorkspaceFilesResponse,
} from '@animaos/sdk';
import { daemon } from '../lib/daemon-api';

const message = (error: unknown) =>
  error instanceof Error ? error.message : String(error);
function sizeLabel(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function WorkspaceFiles({ online }: { online: boolean }) {
  const [listing, setListing] = useState<WorkspaceFilesResponse | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<string | null>(null);
  const [preview, setPreview] = useState<WorkspaceFileResponse | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [reading, setReading] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [downloadError, setDownloadError] = useState<string | null>(null);
  const request = useRef(0);
  const downloadGeneration = useRef(0);
  const downloadController = useRef<AbortController | null>(null);
  const downloadUrls = useRef(new Map<string, number>());
  const selectedSupportsDownload = Boolean(
    selected && /\.(?:txt|md|csv)$/i.test(selected),
  );

  useResetOnChange([online, refresh], () => {
    setDownloading(false);
    setDownloadError(null);
    setSelected(null);
    setPreview(null);
    setPreviewError(null);
    setReading(false);
    setLoading(online);
    if (online) setListError(null);
  });
  const invalidateRequests = useCallback(() => {
    request.current++;
    downloadGeneration.current++;
    downloadController.current?.abort();
    downloadController.current = null;
  }, []);
  useEffect(() => {
    let active = true;
    const ownedUrls = downloadUrls.current;
    const clearUrls = () => {
      for (const [url, timer] of ownedUrls) {
        window.clearTimeout(timer);
        URL.revokeObjectURL(url);
      }
      ownedUrls.clear();
    };
    invalidateRequests();
    clearUrls();
    if (online) {
      daemon
        .listWorkspaceFiles()
        .then((result) => {
          if (active) setListing(result);
        })
        .catch((error) => {
          if (active) setListError(message(error));
        })
        .finally(() => {
          if (active) setLoading(false);
        });
    }
    return () => {
      active = false;
      invalidateRequests();
      clearUrls();
    };
  }, [online, refresh, invalidateRequests]);

  const read = async (path: string) => {
    if (!online) return;
    downloadGeneration.current++;
    downloadController.current?.abort();
    downloadController.current = null;
    setDownloading(false);
    setDownloadError(null);
    const id = ++request.current;
    setSelected(path);
    setPreview(null);
    setPreviewError(null);
    setReading(true);
    try {
      const result = await daemon.readWorkspaceFile(path);
      if (id === request.current) setPreview(result);
    } catch (error) {
      if (id === request.current) setPreviewError(message(error));
    } finally {
      if (id === request.current) setReading(false);
    }
  };
  const download = async () => {
    if (
      !online ||
      !selected ||
      !selectedSupportsDownload ||
      downloading ||
      downloadController.current
    ) {
      return;
    }
    const path = selected;
    const controller = new AbortController();
    const generation = ++downloadGeneration.current;
    downloadController.current = controller;
    setDownloading(true);
    setDownloadError(null);
    const active = () =>
      downloadGeneration.current === generation && !controller.signal.aborted;
    try {
      const blob = await daemon.downloadWorkspaceFile(path, controller.signal);
      if (!active()) return;
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = path.split('/').at(-1) || 'workspace-file';
      const timer = window.setTimeout(() => {
        URL.revokeObjectURL(url);
        downloadUrls.current.delete(url);
      }, 0);
      downloadUrls.current.set(url, timer);
      link.click();
    } catch (error) {
      if (active()) setDownloadError(message(error));
    } finally {
      if (downloadController.current === controller) {
        downloadController.current = null;
        if (active()) setDownloading(false);
      }
    }
  };
  const files =
    listing?.files.filter((file) =>
      file.path.toLowerCase().includes(query.trim().toLowerCase()),
    ) ?? [];

  return (
    <section className="files-page" aria-labelledby="workspace-files-heading">
      <div className="files-head">
        <h1 id="workspace-files-heading" className="sr-only">
          Files
        </h1>
        <button
          type="button"
          className="settings-item-action"
          disabled={!online || loading}
          onClick={() => setRefresh((value) => value + 1)}
        >
          Refresh files
        </button>
      </div>
      {!online ? (
        <p
          role="status"
          className="rounded-2xl border border-line p-6 text-sm text-ink-2"
        >
          Connect to the daemon to browse workspace files.
        </p>
      ) : (
        <>
          <label className="files-search">
            <span className="sr-only">Search files</span>
            <input
              type="search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search by name or path"
            />
          </label>
          {loading && (
            <p role="status" className="text-sm text-ink-3">
              Loading files…
            </p>
          )}
          {listError && (
            <p
              role="alert"
              className="rounded-xl border border-danger/30 p-4 text-sm text-danger"
            >
              {listError}
              {listing ? ' Showing the last loaded file list.' : ''}
            </p>
          )}
          {listing?.truncated && (
            <p className="text-sm text-ink-3">
              The file list is truncated. Search filters only the files shown
              here.
            </p>
          )}
          <div className="files-split">
            <div className="settings-list files-list">
              <div className="files-count">
                {files.length} {files.length === 1 ? 'file' : 'files'} shown
              </div>
              <div className="files-scroll">
                {files.map((file) => (
                  <button
                    key={file.path}
                    type="button"
                    aria-pressed={selected === file.path}
                    onClick={() => void read(file.path)}
                    className="files-row"
                  >
                    <span className="files-row-name">{file.name}</span>
                    <span className="files-row-path">{file.path}</span>
                    <span className="files-row-meta">
                      {sizeLabel(file.sizeBytes)} ·{' '}
                      {file.modifiedAtMs === null
                        ? 'Date unavailable'
                        : new Date(file.modifiedAtMs).toLocaleString()}
                    </span>
                  </button>
                ))}
                {!loading && !listError && !files.length && (
                  <p className="p-6 text-sm text-ink-3">
                    {listing?.files.length
                      ? 'No files match your search.'
                      : 'No workspace files yet.'}
                  </p>
                )}
              </div>
            </div>
            <section aria-label="File preview" className="files-preview">
              <div className="files-preview-head">
                <h2>{selected ?? 'File preview'}</h2>
                <span>Read only</span>
              </div>
              {selected && selectedSupportsDownload && (
                <div className="space-y-2 p-4">
                  <button
                    type="button"
                    className="rounded-xl border border-line px-4 py-2 text-sm text-ink disabled:opacity-50"
                    disabled={!online || downloading}
                    onClick={() => void download()}
                  >
                    {downloading ? 'Downloading…' : 'Download current file'}
                  </button>
                  {downloadError && (
                    <div className="space-y-2">
                      <p role="alert" className="text-sm text-danger">
                        {downloadError}
                      </p>
                      <button
                        type="button"
                        className="text-sm text-accent disabled:opacity-50"
                        disabled={!online || downloading}
                        onClick={() => void download()}
                      >
                        Retry download
                      </button>
                    </div>
                  )}
                </div>
              )}
              {reading && (
                <p role="status" className="p-6 text-sm text-ink-3">
                  Loading preview…
                </p>
              )}
              {previewError && (
                <div className="space-y-3 p-6">
                  <p role="alert" className="text-sm text-danger">
                    {previewError}
                  </p>
                  <button
                    type="button"
                    className="text-sm text-accent"
                    onClick={() => selected && void read(selected)}
                  >
                    Retry preview
                  </button>
                </div>
              )}
              {!selected && (
                <p className="p-6 text-sm text-ink-3">
                  Select a file to read its contents.
                </p>
              )}
              {preview && (
                <>
                  {preview.truncated && (
                    <p className="border-b border-line p-4 text-sm text-ink-3">
                      This preview is truncated.{' '}
                      {selectedSupportsDownload
                        ? 'Download the current file to get its full contents.'
                        : 'Open the file locally to see its full contents.'}
                    </p>
                  )}
                  <pre className="files-preview-body">
                    {preview.content || '(Empty file)'}
                  </pre>
                </>
              )}
            </section>
          </div>
        </>
      )}
    </section>
  );
}
