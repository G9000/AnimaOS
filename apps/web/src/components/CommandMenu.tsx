import { useResetOnChange } from '../hooks/useResetOnChange';
import { Fragment, useEffect, useId, useRef, useState } from 'react';
import type {
  MemorySearchResult,
  SessionMessageSearchHit,
  SessionMessageSearchPage,
} from '@animaos/sdk';
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
  searchMemories,
  onOpenMessage,
  onOpenMemory,
  mainSessionId,
}: {
  commands: StudioCommand[];
  close: () => void;
  searchMessages?: (
    query: string,
    signal: AbortSignal,
  ) => Promise<SessionMessageSearchPage>;
  searchMemories?: (
    query: string,
    signal: AbortSignal,
  ) => Promise<MemorySearchResult[]>;
  onOpenMessage?: (hit: SessionMessageSearchHit) => void;
  onOpenMemory?: (hit: MemorySearchResult) => void;
  mainSessionId?: string;
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
    memorySearch: typeof searchMemories;
    status: 'loading' | 'ready' | 'failed';
    page: SessionMessageSearchPage;
    memories: MemorySearchResult[];
    messageFailed: boolean;
    memoryFailed: boolean;
  } | null>(null);
  const results = commands.filter((command) =>
    `${command.title} ${command.description} ${command.group}`
      .toLowerCase()
      .includes(query.trim().toLowerCase()),
  );
  // Callback identity changes with the selected companion/connection. Never
  // expose a prior companion's results while its replacement request starts.
  const currentRead =
    messageRead?.query === trimmedQuery &&
    messageRead.search === searchMessages &&
    messageRead.memorySearch === searchMemories
      ? messageRead
      : null;
  const messageResults =
    currentRead?.status === 'ready' ? currentRead.page.results : [];
  const memoryResults =
    currentRead?.status === 'ready' ? currentRead.memories : [];
  const orderedMessages = mainSessionId
    ? [
        ...messageResults.filter((hit) => hit.sessionId === mainSessionId),
        ...messageResults.filter((hit) => hit.sessionId !== mainSessionId),
      ]
    : messageResults;
  const totalResults =
    results.length + orderedMessages.length + memoryResults.length;
  const selected = Math.min(active, Math.max(totalResults - 1, 0));

  useResetOnChange([trimmedQuery, searchMessages, searchMemories, retry], () => {
    setMessageRead(
      trimmedQuery && (searchMessages || searchMemories)
        ? {
            query: trimmedQuery,
            search: searchMessages,
            memorySearch: searchMemories,
            page: { results: [], hasMore: false },
            memories: [],
            messageFailed: false,
            memoryFailed: false,
            status: 'loading',
          }
        : null,
    );
  });
  useEffect(() => {
    if (!trimmedQuery || (!searchMessages && !searchMemories)) return;
    const controller = new AbortController();
    let current = true;
    const state = {
      query: trimmedQuery,
      search: searchMessages,
      memorySearch: searchMemories,
      page: { results: [], hasMore: false },
      memories: [],
      messageFailed: false,
      memoryFailed: false,
    };
    const timer = setTimeout(() => {
      void Promise.allSettled([
        searchMessages
          ? searchMessages(trimmedQuery, controller.signal)
          : Promise.resolve({ results: [], hasMore: false }),
        searchMemories
          ? searchMemories(trimmedQuery, controller.signal)
          : Promise.resolve([]),
      ]).then(([messageRead, memoryRead]) => {
        if (!current || controller.signal.aborted) return;
        const succeeded =
          (searchMessages !== undefined &&
            messageRead.status === 'fulfilled') ||
          (searchMemories !== undefined &&
            memoryRead.status === 'fulfilled');
        setMessageRead({
          ...state,
          page:
            messageRead.status === 'fulfilled'
              ? messageRead.value
              : { results: [], hasMore: false },
          memories:
            memoryRead.status === 'fulfilled' ? memoryRead.value : [],
          messageFailed:
            searchMessages !== undefined && messageRead.status === 'rejected',
          memoryFailed:
            searchMemories !== undefined && memoryRead.status === 'rejected',
          status: succeeded ? 'ready' : 'failed',
        });
      });
    }, 200);
    return () => {
      current = false;
      clearTimeout(timer);
      controller.abort();
    };
  }, [trimmedQuery, searchMessages, searchMemories, retry]);

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

  function chooseMemory(hit: MemorySearchResult) {
    close();
    onOpenMemory?.(hit);
  }

  function chooseSelected() {
    if (selected < results.length) choose(results[selected]);
    else if (selected < results.length + orderedMessages.length) {
      const hit = orderedMessages[selected - results.length];
      if (hit) chooseMessage(hit);
    } else {
      const hit = memoryResults[selected - results.length - orderedMessages.length];
      if (hit) chooseMemory(hit);
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
              searchMessages || searchMemories
                ? 'Search commands, messages and memories…'
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
          aria-label={searchMessages || searchMemories ? 'Commands, messages and memories' : 'Commands'}
        >
          {results.map((command, index) => (
            <Fragment key={command.id}>
              {results[index - 1]?.group !== command.group && (
                <div className="studio-command-heading" aria-hidden>
                  {command.group === 'Navigate' ? 'Go to' : command.group}
                </div>
              )}
              <button
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
                <span className="studio-command-enter" aria-hidden>
                  ↵
                </span>
              </button>
            </Fragment>
          ))}
          {searchMessages && trimmedQuery && (
            <div role="group" aria-labelledby={`${id}-messages-heading`}>
              <div
                className="studio-command-message-heading"
                id={`${id}-messages-heading`}
              >
                Messages
              </div>
              {orderedMessages.map((hit, index) => {
                const position = results.length + index;
                const group =
                  mainSessionId && hit.sessionId === mainSessionId
                    ? 'Main chat'
                    : mainSessionId
                      ? 'Elsewhere'
                      : null;
                const previous = orderedMessages[index - 1];
                const previousGroup =
                  previous && mainSessionId
                    ? previous.sessionId === mainSessionId
                      ? 'Main chat'
                      : 'Elsewhere'
                    : null;
                return (
                  <Fragment key={`${hit.sessionId}\u0000${hit.messageId}`}>
                    {group && group !== previousGroup && (
                      <div className="studio-command-subheading">{group}</div>
                    )}
                    <button
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
                        {group === 'Main chat' ? '⌂' : '↩'}
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
                  </Fragment>
                );
              })}
            </div>
          )}
          {searchMemories && trimmedQuery && memoryResults.length > 0 && (
            <div role="group" aria-label="Memory">
              <div className="studio-command-message-heading">Memory</div>
              {memoryResults.map((memory, index) => {
                const position = results.length + orderedMessages.length + index;
                return (
                  <button
                    key={memory.id}
                    type="button"
                    role="option"
                    tabIndex={-1}
                    aria-selected={selected === position}
                    id={`${id}-${position}`}
                    className="studio-command-option"
                    onMouseMove={() => setActive(position)}
                    onClick={() => chooseMemory(memory)}
                  >
                    <span className="studio-command-symbol" aria-hidden>✳</span>
                    <span>
                      <strong>{memory.agentName} · {memory.type.replace('_', ' ')}</strong>
                      <small className="studio-command-message-snippet">{memory.content}</small>
                      <small>{memory.scope} · {new Date(memory.createdAt).toLocaleDateString()}</small>
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </div>
        {!results.length && !trimmedQuery && (
          <p role="status" className="studio-command-empty">
            No commands found. Try “workspace”, “plan”, or “settings”.
          </p>
        )}
        {(searchMessages || searchMemories) && trimmedQuery && (
          <div className="studio-command-empty" role="status">
            {!currentRead || currentRead.status === 'loading' ? (
              searchMemories ? 'Searching messages and memory…' : 'Searching messages…'
            ) : currentRead.status === 'failed' ? (
              <>
                <span>
                  {searchMemories
                    ? 'Search is unavailable. Try again.'
                    : 'Message search is unavailable. Try again.'}
                </span>{' '}
                <button
                  type="button"
                  className="studio-tool-button"
                  onClick={() => setRetry((value) => value + 1)}
                >
                  Search again
                </button>
              </>
            ) : currentRead.messageFailed || currentRead.memoryFailed ? (
              <>
                <span>Some search results are unavailable.</span>{' '}
                <button
                  type="button"
                  className="studio-tool-button"
                  onClick={() => setRetry((value) => value + 1)}
                >
                  Search again
                </button>
              </>
            ) : messageResults.length === 0 && memoryResults.length === 0 ? (
              searchMemories
                ? 'No matching messages or memories.'
                : 'No messages found.'
            ) : currentRead.page.hasMore ? (
              'Showing the newest matches. Narrow your search to find more.'
            ) : (
              `${messageResults.length + memoryResults.length} results found.`
            )}
          </div>
        )}
        <footer>
          <span>↑↓ move · ↵ run</span>
          <span>Prompts fill your draft. You decide when to send.</span>
        </footer>
      </div>
    </div>
  );
}
