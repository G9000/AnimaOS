import { useEffect, useRef, useState } from 'react';
import type {
  CoreMemory,
  CoreMemoryBlock,
  CoreMemoryBlockName,
  CoreMemoryEdit,
  CoreMemoryEntry,
} from '@animaos/sdk';
import { DaemonHttpError } from '@animaos/sdk';

import { daemon } from '../../lib/daemon-api';
import { COMPANION_UNREACHABLE } from '../../lib/approvals';
import { ConfirmRow, formatCount } from './memory-ui';
import { RevealedText } from './RevealedText';

const BLOCKS: readonly { name: CoreMemoryBlockName; title: string }[] = [
  { name: 'user', title: 'About you' },
  { name: 'notes', title: 'Agent notes' },
];

const DELETE_ENTRY_PROMPT =
  'Delete this entry? Your companion will stop seeing it.';

type Messages = Partial<Record<CoreMemoryBlockName, string>>;

/** The always-on core memory: what the companion knows about you and its own
 *  notes. The owner adds, edits and deletes entries; the daemon applies the
 *  same budget and duplicate rules it applies to the companion. Entry text is
 *  model-written, so it renders only through `RevealedText`. */
export function CoreMemoryPanel({
  agentId,
  epoch,
}: {
  agentId: string;
  epoch: number;
}) {
  const [memory, setMemory] = useState<CoreMemory | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [errors, setErrors] = useState<Messages>({});
  const [warnings, setWarnings] = useState<Messages>({});
  const current = useRef({ agentId, request: 0 });
  current.current.agentId = agentId;

  useEffect(() => {
    const request = ++current.current.request;
    const controller = new AbortController();
    setMemory(null);
    setLoadError(null);
    setErrors({});
    setWarnings({});
    daemon.coreMemory(agentId, { signal: controller.signal }).then(
      (next) => {
        if (current.current.request === request) setMemory(next);
      },
      (cause: unknown) => {
        if (current.current.request === request && !controller.signal.aborted)
          setLoadError(messageOf(cause));
      },
    );
    return () => {
      controller.abort();
      current.current.request += 1;
    };
  }, [agentId, epoch]);

  /** Runs one edit; true when the daemon took it. A result for another agent
   *  (the owner switched while it was in flight) is dropped. */
  const run = async (
    block: CoreMemoryBlockName,
    work: () => Promise<CoreMemoryEdit>,
  ): Promise<boolean> => {
    const requestAgent = agentId;
    setErrors((existing) => ({ ...existing, [block]: undefined }));
    try {
      const result = await work();
      if (current.current.agentId !== requestAgent) return false;
      setMemory({ user: result.user, notes: result.notes });
      setWarnings((existing) => ({
        ...existing,
        [block]: warningNotice(result.warnings),
      }));
      return true;
    } catch (cause) {
      if (current.current.agentId === requestAgent)
        setErrors((existing) => ({ ...existing, [block]: messageOf(cause) }));
      return false;
    }
  };

  return (
    <section
      className="memory-group"
      aria-labelledby="core-memory-heading"
      aria-busy={memory === null && loadError === null}
    >
      <h4 id="core-memory-heading">Always remembered</h4>
      <p className="memory-note">
        Short notes your companion sees in every conversation. Edits follow the
        same size limits the companion has.
      </p>
      {loadError && (
        <p className="memory-error" role="alert">
          {loadError}
        </p>
      )}
      {memory === null && !loadError && (
        <p className="memory-note" role="status">
          Loading what is always remembered…
        </p>
      )}
      {memory &&
        BLOCKS.map(({ name, title }) => (
          <BlockSection
            key={name}
            title={title}
            block={memory[name]}
            error={errors[name]}
            warning={warnings[name]}
            onAdd={(text) =>
              run(name, () => daemon.addCoreEntry(agentId, name, text))
            }
            onEdit={(entry, text) =>
              run(name, () =>
                daemon.updateCoreEntry(agentId, name, entry.id, text),
              )
            }
            onDelete={(entry) =>
              run(name, () => daemon.removeCoreEntry(agentId, name, entry.id))
            }
          />
        ))}
    </section>
  );
}

function BlockSection({
  title,
  block,
  error,
  warning,
  onAdd,
  onEdit,
  onDelete,
}: {
  title: string;
  block: CoreMemoryBlock;
  error: string | undefined;
  warning: string | undefined;
  onAdd: (text: string) => Promise<boolean>;
  onEdit: (entry: CoreMemoryEntry, text: string) => Promise<boolean>;
  onDelete: (entry: CoreMemoryEntry) => Promise<boolean>;
}) {
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const percent =
    block.limit > 0 ? Math.min(100, (block.used / block.limit) * 100) : 0;
  const text = draft.trim();

  return (
    <section className="memory-group" aria-label={title}>
      <h5 className="memory-core-title">{title}</h5>
      <div
        className="memory-usage"
        role="meter"
        aria-label={`${title} usage`}
        aria-valuemin={0}
        aria-valuemax={block.limit}
        aria-valuenow={block.used}
        aria-valuetext={`${formatCount(block.used, block.limit)} characters`}
      >
        <div className="memory-usage-fill" style={{ width: `${percent}%` }} />
      </div>
      <small className="memory-meta">
        {formatCount(block.used, block.limit)} characters
      </small>
      {error && (
        <p className="memory-error" role="alert">
          {error}
        </p>
      )}
      {warning && (
        <p className="memory-note" role="status">
          {warning}
        </p>
      )}
      {block.entries.length === 0 ? (
        <p className="memory-empty">Nothing here yet.</p>
      ) : (
        <ul className="memory-list">
          {block.entries.map((entry) => (
            <EntryRow
              key={entry.id}
              entry={entry}
              onEdit={onEdit}
              onDelete={onDelete}
            />
          ))}
        </ul>
      )}
      <form
        className="memory-form"
        aria-label={`Add to ${title}`}
        onSubmit={async (event) => {
          event.preventDefault();
          if (!text || busy) return;
          setBusy(true);
          const added = await onAdd(text);
          setBusy(false);
          if (added) setDraft('');
        }}
      >
        <label className="memory-field">
          Add to {title}
          <textarea
            value={draft}
            rows={2}
            onChange={(event) => setDraft(event.target.value)}
          />
        </label>
        <div className="memory-actions">
          <button
            type="submit"
            className="studio-tool-button"
            disabled={!text || busy}
          >
            Add
          </button>
        </div>
      </form>
    </section>
  );
}

function EntryRow({
  entry,
  onEdit,
  onDelete,
}: {
  entry: CoreMemoryEntry;
  onEdit: (entry: CoreMemoryEntry, text: string) => Promise<boolean>;
  onDelete: (entry: CoreMemoryEntry) => Promise<boolean>;
}) {
  const [mode, setMode] = useState<'view' | 'edit' | 'delete'>('view');
  const [value, setValue] = useState(entry.content);
  const [busy, setBusy] = useState(false);
  const trimmed = value.trim();
  const canSave = trimmed !== '' && trimmed !== entry.content && !busy;

  return (
    <li className="memory-item">
      {mode === 'edit' ? (
        <form
          className="memory-form"
          aria-label="Edit entry"
          onSubmit={async (event) => {
            event.preventDefault();
            if (!canSave) return;
            setBusy(true);
            const saved = await onEdit(entry, trimmed);
            setBusy(false);
            if (saved) setMode('view');
          }}
        >
          <label className="memory-field">
            Entry
            <textarea
              value={value}
              rows={3}
              onChange={(event) => setValue(event.target.value)}
            />
          </label>
          <div className="memory-actions">
            <button
              type="submit"
              className="studio-tool-button"
              disabled={!canSave}
            >
              Save
            </button>
            <button
              type="button"
              className="studio-tool-button"
              onClick={() => {
                setValue(entry.content);
                setMode('view');
              }}
            >
              Cancel
            </button>
          </div>
        </form>
      ) : (
        <div className="memory-item-body">
          <p className="memory-content">
            <RevealedText text={entry.content} />
          </p>
          {entry.flagged.length > 0 && (
            <small className="memory-hidden-note">
              Your companion does not see this text: it matched{' '}
              {entry.flagged.join(', ')}.
            </small>
          )}
        </div>
      )}
      {mode === 'delete' && (
        <ConfirmRow
          prompt={DELETE_ENTRY_PROMPT}
          confirmLabel="Delete"
          onConfirm={async () => {
            if (busy) return;
            setBusy(true);
            const gone = await onDelete(entry);
            setBusy(false);
            if (!gone) setMode('view');
          }}
          onKeep={() => setMode('view')}
        />
      )}
      {mode === 'view' && (
        <div className="memory-actions">
          <button
            type="button"
            className="studio-tool-button"
            onClick={() => {
              setValue(entry.content);
              setMode('edit');
            }}
          >
            Edit
          </button>
          <button
            type="button"
            className="studio-tool-button"
            onClick={() => setMode('delete')}
          >
            Delete
          </button>
        </div>
      )}
    </li>
  );
}

function warningNotice(ids: readonly string[]): string | undefined {
  return ids.length === 0
    ? undefined
    : `Saved, but this text matched ${ids.join(', ')}. Your companion will not see it until it is changed.`;
}

function messageOf(cause: unknown): string {
  return cause instanceof DaemonHttpError
    ? cause.message
    : COMPANION_UNREACHABLE;
}
