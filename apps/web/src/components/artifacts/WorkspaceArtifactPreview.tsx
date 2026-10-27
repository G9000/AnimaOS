import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent,
  type ReactNode,
} from 'react';
import { daemon } from '../../lib/daemon-api';
import { useResetOnChange } from '../../hooks/useResetOnChange';
import {
  artifactDocument,
  artifactWindowDocument,
  storageSnapshot,
  workspaceArtifactPath,
} from './artifact-document';
import { useArtifactResize } from './useArtifactResize';
import { useArtifactFullscreenMotion } from './useArtifactFullscreenMotion';
import {
  CodeIcon,
  DownloadIcon,
  ExternalLinkIcon,
  MaximizeIcon,
  MinimizeIcon,
  RefreshIcon,
  XIcon,
} from '../icons';

const ArtifactScope = createContext<
  ((path: string, opener: HTMLElement) => void) | null
>(null);
const InsideLink = createContext(false);

export function ArtifactLinkBoundary({ children }: { children: ReactNode }) {
  return <InsideLink.Provider value>{children}</InsideLink.Provider>;
}

export function WorkspaceArtifactLink({
  path,
  literal = false,
  children,
}: {
  path: string;
  literal?: boolean;
  children: ReactNode;
}) {
  const open = useContext(ArtifactScope);
  const insideLink = useContext(InsideLink);
  const target = workspaceArtifactPath(path, literal);
  return open && target && !insideLink ? (
    <ArtifactLinkBoundary>
      <button
        type="button"
        className="break-words text-accent underline underline-offset-2"
        aria-label={`Open ${target}`}
        onClick={(event) => open(target, event.currentTarget)}
      >
        {children}
      </button>
    </ArtifactLinkBoundary>
  ) : (
    <>{children}</>
  );
}

export function WorkspaceArtifactLayout(props: {
  agentId: string;
  sessionId: string;
  clearGeneration: number;
  children: ReactNode;
}) {
  return (
    <ArtifactSession
      key={JSON.stringify([
        props.agentId,
        props.sessionId,
        props.clearGeneration,
      ])}
      {...props}
    />
  );
}

function ArtifactSession({
  agentId,
  sessionId,
  clearGeneration,
  children,
}: Parameters<typeof WorkspaceArtifactLayout>[0]) {
  const [path, setPath] = useState<string | null>(null);
  const [closing, setClosing] = useState(false);
  const {
    layoutRef,
    chatShare,
    minimum,
    resizing,
    stopResizing,
    dividerHandlers,
  } = useArtifactResize();
  const {
    slotRef,
    fullscreen,
    targetFullscreen,
    fullscreenAnimating,
    transitionTo,
    cancelFullscreen,
    closeFullscreen,
  } = useArtifactFullscreenMotion(layoutRef, chatShare);
  const chatPaneId = useId();
  const panel = useRef<HTMLElement>(null);
  const fullscreenTrigger = useRef<HTMLElement | null>(null);
  const opener = useRef<HTMLElement | null>(null);
  const open = useMemo(
    () => (target: string, element: HTMLElement) => {
      opener.current = element;
      cancelFullscreen();
      setClosing(false);
      setPath(target);
      panel.current?.focus();
    },
    [cancelFullscreen],
  );
  useEffect(() => {
    if (path && !closing) panel.current?.focus();
    else if (opener.current?.isConnected && !opener.current.closest('[inert]'))
      opener.current.focus();
  }, [path, closing]);
  const finishClose = useCallback(() => {
    cancelFullscreen();
    setPath(null);
    setClosing(false);
  }, [cancelFullscreen]);
  useEffect(() => {
    if (!closing || fullscreen) return;
    // Retain the document for its exit; reopening cancels this cleanup.
    const timeout = window.setTimeout(finishClose, 220);
    return () => window.clearTimeout(timeout);
  }, [closing, fullscreen, finishClose]);
  useEffect(() => {
    const preview = panel.current;
    if (!fullscreen || !preview) return;
    const background: HTMLElement[] = [];
    // Keep focus inside the expanded preview without moving its live iframe.
    let current: HTMLElement | null = preview;
    while (current && current !== document.body && current.parentElement) {
      for (const sibling of current.parentElement.children) {
        if (
          sibling !== current &&
          sibling instanceof HTMLElement &&
          !sibling.hasAttribute('inert')
        ) {
          sibling.setAttribute('inert', '');
          background.push(sibling);
        }
      }
      current = current.parentElement;
    }
    preview.focus();
    return () => {
      for (const element of background) element.removeAttribute('inert');
      if (
        preview.isConnected &&
        !preview.hasAttribute('inert') &&
        fullscreenTrigger.current?.isConnected
      )
        fullscreenTrigger.current.focus();
    };
  }, [fullscreen]);
  const close = useCallback(() => {
    if (closing) return;
    stopResizing();
    if (fullscreen) {
      setClosing(true);
      closeFullscreen(finishClose);
      return;
    }
    cancelFullscreen();
    if (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches)
      finishClose();
    else setClosing(true);
  }, [
    closing,
    stopResizing,
    fullscreen,
    closeFullscreen,
    finishClose,
    cancelFullscreen,
  ]);
  const escape = useCallback(() => {
    if (closing) return;
    if (fullscreen) {
      if (targetFullscreen) transitionTo(false);
    } else close();
  }, [closing, fullscreen, targetFullscreen, transitionTo, close]);
  const toggleFullscreen = useCallback(
    (trigger: HTMLElement) => {
      if (closing) return;
      stopResizing();
      fullscreenTrigger.current = trigger;
      transitionTo(!targetFullscreen);
    },
    [closing, stopResizing, transitionTo, targetFullscreen],
  );
  return (
    <ArtifactScope.Provider value={open}>
      <div
        ref={layoutRef}
        className="workspace-artifact-layout chat-results-layout relative flex h-full min-h-0 min-w-0 flex-1"
        data-open={Boolean(path && !closing)}
        data-resizing={resizing || undefined}
        data-fullscreen={fullscreen || undefined}
        data-fullscreen-animating={fullscreenAnimating || undefined}
        data-fullscreen-closing={(fullscreen && closing) || undefined}
        style={
          {
            '--artifact-preview-width': `${100 - chatShare}%`,
          } as CSSProperties
        }
      >
        <div
          id={chatPaneId}
          className="workspace-artifact-conversation flex min-h-0 min-w-0 flex-1 flex-col"
        >
          {children}
        </div>
        {path && !closing && !fullscreen && (
          <div className="workspace-artifact-divider-slot">
            <div
              role="separator"
              tabIndex={0}
              aria-label="Resize chat and artifact preview"
              aria-orientation="vertical"
              aria-controls={chatPaneId}
              aria-valuemin={Math.round(minimum)}
              aria-valuemax={Math.round(100 - minimum)}
              aria-valuenow={Math.round(chatShare)}
              aria-valuetext={`${Math.round(chatShare)}% chat, ${Math.round(100 - chatShare)}% artifact preview`}
              title="Drag to resize. Double-click to reset."
              className="workspace-artifact-divider"
              {...dividerHandlers}
            />
          </div>
        )}
        <div ref={slotRef} className="workspace-artifact-slot">
          {path && (
            <aside
              ref={panel}
              tabIndex={-1}
              aria-label="Artifact preview"
              role={fullscreen ? 'dialog' : undefined}
              aria-modal={fullscreen || undefined}
              aria-hidden={closing || undefined}
              inert={closing || undefined}
              className="flex h-full min-h-0 w-full flex-col border-l border-line bg-panel outline-none"
              onKeyDown={(event) => {
                if (event.key === 'Escape') {
                  event.preventDefault();
                  event.stopPropagation();
                  escape();
                }
              }}
            >
              <ArtifactFile
                key={path}
                path={path}
                storageKey={`anima-artifact:${JSON.stringify([agentId, sessionId, clearGeneration, path])}`}
                onClose={close}
                onEscape={escape}
                fullscreen={targetFullscreen}
                onToggleFullscreen={toggleFullscreen}
              />
            </aside>
          )}
        </div>
      </div>
    </ArtifactScope.Provider>
  );
}

function ArtifactFile({
  path,
  storageKey,
  onClose,
  onEscape,
  fullscreen,
  onToggleFullscreen,
}: {
  path: string;
  storageKey: string;
  onClose(): void;
  onEscape(): void;
  fullscreen: boolean;
  onToggleFullscreen(trigger: HTMLElement): void;
}) {
  const [attempt, setAttempt] = useState(0);
  const [loaded, setLoaded] = useState<{
    source: string;
    document: string;
    sourceUrl: string;
    windowUrl: string;
    nonce: string;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const frame = useRef<HTMLIFrameElement>(null);
  const popupUrls = useRef<string[]>([]);
  const request = useMemo(
    () => ({ path, storageKey, attempt }),
    [path, storageKey, attempt],
  );
  useResetOnChange([request], () => {
    setLoaded(null);
    setError(null);
  });
  useEffect(() => {
    const { path: requestPath, storageKey: requestStorageKey } = request;
    const abort = new AbortController();
    const urls: string[] = [];
    void (async () => {
      try {
        const blob = await daemon.downloadWorkspaceFile(
          requestPath,
          abort.signal,
        );
        const source = await blob.text();
        if (abort.signal.aborted) return;
        let snapshot = '[]';
        try {
          snapshot =
            storageSnapshot(localStorage.getItem(requestStorageKey)) ?? '[]';
        } catch {
          /* Storage may be unavailable. */
        }
        const nonce = crypto.randomUUID();
        const document = artifactDocument(source, nonce, snapshot);
        const sourceUrl = URL.createObjectURL(blob);
        urls.push(sourceUrl);
        const windowUrl = URL.createObjectURL(
          new Blob(
            [artifactWindowDocument(document, nonce, requestStorageKey)],
            {
              type: 'text/html',
            },
          ),
        );
        urls.push(windowUrl);
        setLoaded({
          source,
          document: artifactWindowDocument(document, nonce),
          sourceUrl,
          windowUrl,
          nonce,
        });
      } catch (caught) {
        if (!abort.signal.aborted)
          setError(
            caught instanceof Error
              ? caught.message
              : 'This HTML file could not be opened.',
          );
      }
    })();
    return () => {
      abort.abort();
      for (const url of urls) URL.revokeObjectURL(url);
      for (const url of popupUrls.current) URL.revokeObjectURL(url);
      popupUrls.current = [];
    };
  }, [request]);
  useEffect(() => {
    if (!loaded) return;
    const persist = (event: MessageEvent) => {
      if (
        event.source !== frame.current?.contentWindow ||
        event.data?.nonce !== loaded.nonce
      )
        return;
      if (event.data.type === 'anima-artifact-close') {
        onEscape();
        return;
      }
      if (event.data.type !== 'anima-artifact-storage') return;
      const snapshot = storageSnapshot(event.data.snapshot);
      if (snapshot !== null) {
        try {
          localStorage.setItem(storageKey, snapshot);
        } catch {
          /* Keep the live page usable if storage is full. */
        }
      }
    };
    window.addEventListener('message', persist);
    return () => window.removeEventListener('message', persist);
  }, [loaded, storageKey, onEscape]);
  function prepareWindow(event: MouseEvent<HTMLAnchorElement>) {
    if (!loaded) return;
    let snapshot = '[]';
    try {
      snapshot = storageSnapshot(localStorage.getItem(storageKey)) ?? '[]';
    } catch {
      /* Keep the page usable without persistent storage. */
    }
    const nonce = crypto.randomUUID();
    const document = artifactDocument(loaded.source, nonce, snapshot);
    const url = URL.createObjectURL(
      new Blob([artifactWindowDocument(document, nonce, storageKey)], {
        type: 'text/html',
      }),
    );
    popupUrls.current.push(url);
    event.currentTarget.href = url;
  }
  return (
    <>
      <header className="workspace-artifact-toolbar">
        <span className="workspace-artifact-file-icon" aria-hidden="true">
          <CodeIcon size={16} strokeWidth={1.7} />
        </span>
        <div className="workspace-artifact-file-info">
          <h2 title={path}>{path.split('/').at(-1)}</h2>
          <p>HTML preview</p>
        </div>
        <div
          className="workspace-artifact-actions"
          role="group"
          aria-label="Preview actions"
        >
          <button
            type="button"
            className="workspace-artifact-action"
            aria-label="Reload"
            title="Reload preview"
            onClick={() => setAttempt((n) => n + 1)}
          >
            <RefreshIcon size={16} strokeWidth={1.7} />
          </button>
          {loaded && (
            <>
              <a
                className="workspace-artifact-action"
                aria-label="Download HTML"
                title="Download HTML"
                href={loaded.sourceUrl}
                download={path.split('/').at(-1)}
              >
                <DownloadIcon size={16} strokeWidth={1.7} />
              </a>
              <a
                className="workspace-artifact-action"
                aria-label="Open in new window"
                title="Open in new window"
                href={loaded.windowUrl}
                target="_blank"
                rel="noopener noreferrer"
                onClick={prepareWindow}
                onAuxClick={prepareWindow}
                onContextMenu={prepareWindow}
              >
                <ExternalLinkIcon size={16} strokeWidth={1.7} />
              </a>
            </>
          )}
          <button
            type="button"
            className="workspace-artifact-action"
            aria-label={fullscreen ? 'Exit fullscreen' : 'Enter fullscreen'}
            title={fullscreen ? 'Exit fullscreen' : 'Enter fullscreen'}
            aria-pressed={fullscreen}
            onClick={(event) => onToggleFullscreen(event.currentTarget)}
          >
            {fullscreen ? (
              <MinimizeIcon size={16} strokeWidth={1.7} />
            ) : (
              <MaximizeIcon size={16} strokeWidth={1.7} />
            )}
          </button>
          <span
            className="workspace-artifact-action-divider"
            aria-hidden="true"
          />
          <button
            type="button"
            className="workspace-artifact-action"
            aria-label="Close artifact preview"
            title="Close preview"
            onClick={onClose}
          >
            <XIcon size={16} strokeWidth={1.7} />
          </button>
        </div>
      </header>
      {error ? (
        <p role="alert" className="p-4 text-sm text-danger">
          {error}
        </p>
      ) : loaded ? (
        <iframe
          ref={frame}
          title={path}
          className="min-h-0 w-full flex-1 border-0 bg-white"
          sandbox="allow-scripts allow-downloads allow-modals"
          referrerPolicy="no-referrer"
          srcDoc={loaded.document}
        />
      ) : (
        <p role="status" className="p-4 text-sm text-ink-3">
          Opening HTML file…
        </p>
      )}
    </>
  );
}
