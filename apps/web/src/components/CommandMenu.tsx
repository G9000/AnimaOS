import { useEffect, useId, useRef, useState } from 'react';
import type {
  SessionMessageSearchHit,
  SessionMessageSearchPage,
} from '@animaOS-SWARM/sdk';
import './sessions/session-continuity.css';

export interface StudioCommand {
  id: string;
  title: string;
  description: string;
  group: string;
  run: () => void;
}

export function CommandMenu({
  commands,
  close,
  searchMessages,
  onOpenMessage,
}: {
  commands: StudioCommand[];
  close: () => void;
  searchMessages?: (
    query: string,
    signal: AbortSignal,
  ) => Promise<SessionMessageSearchPage>;
  onOpenMessage?: (hit: SessionMessageSearchHit) => void;
}) {
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const dialog = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const id = useId();
  const trimmedQuery = query.trim();
  const [retry, setRetry] = useState(0);
  const [messageRead, setMessageRead] = useState<{
    query: string;
    search: typeof searchMessages;
    status: 'loading' | 'ready' | 'failed';
    page: SessionMessageSearchPage;
  } | null>(null);
  const results = commands.filter((command) =>
    `${command.title} ${command.description} ${command.group}`
      .toLowerCase()
      .includes(query.trim().toLowerCase()),
  );
  // Callback identity changes with the selected companion/connection. Never
  // expose a prior companion's results while its replacement request starts.
  const currentRead =
    messageRead?.query === trimmedQuery && messageRead.search === searchMessages
      ? messageRead
      : null;
  const messageResults =
    currentRead?.status === 'ready' ? currentRead.page.results : [];
  const totalResults = results.length + messageResults.length;
  const selected = Math.min(active, Math.max(totalResults - 1, 0));

  useEffect(() => {
    if (!trimmedQuery || !searchMessages) {
      setMessageRead(null);
      return;
    }
    const controller = new AbortController();
    let current = true;
    const state = {
      query: trimmedQuery,
      search: searchMessages,
      page: { results: [], hasMore: false },
    };
    setMessageRead({ ...state, status: 'loading' });
    const timer = setTimeout(() => {
      void Promise.resolve()
        .then(() => searchMessages(trimmedQuery, controller.signal))
        .then((page) => {
          if (current && !controller.signal.aborted)
            setMessageRead({ ...state, page, status: 'ready' });
        })
        .catch(() => {
          if (current && !controller.signal.aborted)
            setMessageRead({ ...state, status: 'failed' });
        });
    }, 200);
    return () => {
      current = false;
      clearTimeout(timer);
      controller.abort();
    };
  }, [trimmedQuery, searchMessages, retry]);

  useEffect(() => {
    const trigger =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    input.current?.focus();
    return () => {
      if (trigger?.isConnected) trigger.focus();
    };
  }, []);

  useEffect(() => {
    document
      .getElementById(`${id}-${selected}`)
      ?.scrollIntoView?.({ block: 'nearest' });
  }, [id, selected, query, totalResults]);

  function choose(command: StudioCommand) {
    close();
    command.run();
  }

  function chooseMessage(hit: SessionMessageSearchHit) {
    close();
    onOpenMessage?.(hit);
  }

  function chooseSelected() {
    if (selected < results.length) choose(results[selected]);
    else {
      const hit = messageResults[selected - results.length];
      if (hit) chooseMessage(hit);
    }
  }

  return (
    <div
      className="studio-command-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-label="Command menu"
        className="studio-command-menu"
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing || event.keyCode === 229) return;
          if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            close();
          }
          if (event.key === 'Tab') {
            const focusable = dialog.current?.querySelectorAll<HTMLElement>(
              'input, button:not([tabindex="-1"])',
            );
            if (!focusable?.length) return;
            const first = focusable[0];
            const last = focusable[focusable.length - 1];
            if (event.shiftKey && document.activeElement === first) {
              event.preventDefault();
              last.focus();
            } else if (!event.shiftKey && document.activeElement === last) {
              event.preventDefault();
              first.focus();
            }
          }
        }}
      >
        <div className="studio-command-search">
          <span aria-hidden>⌕</span>
          <input
            ref={input}
            role="combobox"
            aria-label="Search commands"
            aria-expanded="true"
            aria-autocomplete="list"
            aria-controls={`${id}-results`}
            aria-activedescendant={
              totalResults ? `${id}-${selected}` : undefined
            }
            placeholder={
              searchMessages
                ? 'Search commands and saved messages…'
                : 'Where do you want to go?'
            }
            maxLength={200}
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setActive(0);
            }}
            onKeyDown={(event) => {
              if (event.nativeEvent.isComposing || event.keyCode === 229)
                return;
              if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
                event.preventDefault();
                setActive(
                  totalResults
                    ? (selected +
                        (event.key === 'ArrowDown' ? 1 : -1) +
                        totalResults) %
                        totalResults
                    : 0,
                );
              }
              if (event.key === 'Enter' && totalResults > 0) {
                event.preventDefault();
                chooseSelected();
              }
            }}
          />
          <button
            type="button"
            className="studio-tool-button"
            aria-label="Close command menu"
            onClick={close}
          >
            Esc
          </button>
        </div>
        <div
          className="studio-command-results"
          role="listbox"
          id={`${id}-results`}
          aria-label={searchMessages ? 'Commands and messages' : 'Commands'}
        >
          {results.map((command, index) => (
            <button
              key={command.id}
              type="button"
              role="option"
              tabIndex={-1}
              aria-selected={selected === index}
              id={`${id}-${index}`}
              className="studio-command-option"
              onMouseMove={() => setActive(index)}
              onClick={() => choose(command)}
            >
              <span className="studio-command-symbol" aria-hidden>
                {command.group === 'Navigate' ? '↗' : '✳'}
              </span>
              <span>
                <strong>{command.title}</strong>
                <small>{command.description}</small>
              </span>
              <span className="studio-command-group">{command.group}</span>
            </button>
          ))}
          {searchMessages && trimmedQuery && (
            <div role="group" aria-labelledby={`${id}-messages-heading`}>
              <div
                className="studio-command-message-heading"
                id={`${id}-messages-heading`}
              >
                Messages
              </div>
              {messageResults.map((hit, index) => {
                const position = results.length + index;
                return (
                  <button
                    key={`${hit.sessionId}\u0000${hit.messageId}`}
                    type="button"
                    role="option"
                    tabIndex={-1}
                    aria-selected={selected === position}
                    id={`${id}-${position}`}
                    className="studio-command-option"
                    onMouseMove={() => setActive(position)}
                    onClick={() => chooseMessage(hit)}
                  >
                    <span className="studio-command-symbol" aria-hidden>
                      ↩
                    </span>
                    <span>
                      <strong>{hit.title}</strong>
                      <small className="studio-command-message-snippet">
                        {hit.snippet}
                      </small>
                      <small>
                        {hit.role === 'user'
                          ? 'You'
                          : hit.role === 'assistant'
                            ? 'anima'
                            : 'Tool'}{' '}
                        ·{' '}
                        <time
                          dateTime={new Date(hit.createdAtMs).toISOString()}
                        >
                          {new Date(hit.createdAtMs).toLocaleString()}
                        </time>
                      </small>
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </div>
        {!results.length && (
          <p role="status" className="studio-command-empty">
            No commands found. Try “workspace”, “plan”, or “settings”.
          </p>
        )}
        {searchMessages && trimmedQuery && (
          <div className="studio-command-empty" role="status">
            {!currentRead || currentRead.status === 'loading' ? (
              'Searching messages…'
            ) : currentRead.status === 'failed' ? (
              <>
                <span>Message search is unavailable. Try again.</span>{' '}
                <button
                  type="button"
                  className="studio-tool-button"
                  onClick={() => setRetry((value) => value + 1)}
                >
                  Search again
                </button>
              </>
            ) : messageResults.length === 0 ? (
              'No messages found.'
            ) : currentRead.page.hasMore ? (
              'Showing the newest matches. Narrow your search to find more.'
            ) : (
              `${messageResults.length} message${messageResults.length === 1 ? '' : 's'} found.`
            )}
          </div>
        )}
        <footer>
          <span>↑ ↓ to explore · Enter to select</span>
          <span>Prompts fill your draft. You decide when to send.</span>
        </footer>
      </div>
    </div>
  );
}
