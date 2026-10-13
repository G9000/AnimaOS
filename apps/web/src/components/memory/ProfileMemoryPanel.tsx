import { useEffect, useRef, useState } from 'react';
import type {
  MemoryViewCandidates,
  MemoryViewPick,
  MemoryViewSource,
  MemoryViews,
} from '@animaOS-SWARM/sdk';
import { DaemonHttpError, MAX_MEMORY_EDIT_CHARS } from '@animaOS-SWARM/sdk';
import { daemon } from '../../lib/daemon-api';
import { RevealedText } from './RevealedText';

type ViewKind = 'userProfile' | 'agentNotes';
type Picks = Record<ViewKind, MemoryViewPick[]>;
const MAX_SOURCE_SNAPSHOTS = 256;

const EMPTY_CANDIDATES: MemoryViewCandidates = {
  candidates: [],
  hasMore: false,
};

export function ProfileMemoryPanel({
  agentId,
  epoch,
}: {
  agentId: string;
  epoch: number;
}) {
  const [view, setView] = useState<MemoryViews | null>(null);
  const [candidates, setCandidates] = useState(EMPTY_CANDIDATES);
  const [sourceSnapshots, setSourceSnapshots] = useState<
    Map<string, MemoryViewSource>
  >(() => new Map());
  const [picks, setPicks] = useState<Picks>({
    userProfile: [],
    agentNotes: [],
  });
  const [query, setQuery] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [removeUnavailable, setRemoveUnavailable] = useState(false);
  const [edits, setEdits] = useState<Record<string, string>>({});
  const generation = useRef(0);
  const sourceGeneration = useRef(0);
  const picksRef = useRef(picks);
  picksRef.current = picks;
  const currentAgent = useRef(agentId);
  currentAgent.current = agentId;

  useEffect(() => {
    const request = ++generation.current;
    const controller = new AbortController();
    setView(null);
    setCandidates(EMPTY_CANDIDATES);
    setSourceSnapshots(new Map());
    setPicks({ userProfile: [], agentNotes: [] });
    setEdits({});
    setQuery('');
    setError(null);
    setBusy(false);
    setRemoveUnavailable(false);
    setLoading(true);
    void Promise.all([
      daemon.getMemoryViews(agentId, { signal: controller.signal }),
      daemon.memoryViewCandidates(agentId, undefined, {
        signal: controller.signal,
      }),
    ]).then(
      ([nextView, nextCandidates]) => {
        if (
          controller.signal.aborted ||
          generation.current !== request ||
          currentAgent.current !== agentId
        )
          return;
        setView(nextView);
        setCandidates(nextCandidates);
        setSourceSnapshots(
          mergeSnapshots(new Map(), [
            ...sourcesFrom(nextView),
            ...nextCandidates.candidates,
          ]),
        );
        setPicks(picksFrom(nextView));
        setLoading(false);
      },
      (cause: unknown) => {
        if (
          controller.signal.aborted ||
          generation.current !== request ||
          currentAgent.current !== agentId
        )
          return;
        setError(errorMessage(cause));
        setLoading(false);
      },
    );
    return () => {
      controller.abort();
      if (generation.current === request) generation.current += 1;
      sourceGeneration.current += 1;
    };
  }, [agentId, epoch]);

  const savedHashes = new Map<string, string>(
    [...(view?.userProfile ?? []), ...(view?.agentNotes ?? [])].map(
      (source) => [source.id, source.sourceHash] as const,
    ),
  );
  const selectedCount = picks.userProfile.length + picks.agentNotes.length;
  const displayedSelectedCount =
    selectedCount + (removeUnavailable ? 0 : (view?.unavailable.length ?? 0));
  const selectionDirty =
    view !== null &&
    (!samePicks(picks, picksFrom(view)) ||
      (removeUnavailable && view.unavailable.length > 0));

  const searchCandidates = async (searchQuery?: string) => {
    const request = ++sourceGeneration.current;
    const requestAgent = agentId;
    try {
      const result = await daemon.memoryViewCandidates(
        requestAgent,
        searchQuery === undefined
          ? undefined
          : Array.from(searchQuery).slice(0, 512).join(''),
      );
      if (
        sourceGeneration.current === request &&
        currentAgent.current === requestAgent
      ) {
        setCandidates(result);
        setSourceSnapshots((current) =>
          mergeSnapshots(current, result.candidates, [
            ...picksRef.current.userProfile,
            ...picksRef.current.agentNotes,
          ]),
        );
      }
    } catch (cause) {
      if (
        sourceGeneration.current === request &&
        currentAgent.current === requestAgent
      )
        setError(errorMessage(cause));
    }
  };

  const refreshView = async () => {
    if (busy || loading) return;
    const request = ++generation.current;
    const keepLocalSelection = selectionDirty;
    const pendingPicks = picks;
    sourceGeneration.current += 1;
    const controller = new AbortController();
    setBusy(true);
    setLoading(true);
    setError(null);
    try {
      const [nextView, nextCandidates] = await Promise.all([
        daemon.getMemoryViews(agentId, { signal: controller.signal }),
        daemon.memoryViewCandidates(agentId, undefined, {
          signal: controller.signal,
        }),
      ]);
      if (
        controller.signal.aborted ||
        generation.current !== request ||
        currentAgent.current !== agentId
      )
        return;
      setView(nextView);
      setCandidates(nextCandidates);
      setPicks(keepLocalSelection ? pendingPicks : picksFrom(nextView));
      setSourceSnapshots((current) =>
        mergeSnapshots(
          current,
          [...sourcesFrom(nextView), ...nextCandidates.candidates],
          [...pendingPicks.userProfile, ...pendingPicks.agentNotes],
        ),
      );
      if (!keepLocalSelection) setRemoveUnavailable(false);
      setQuery('');
    } catch (cause) {
      if (generation.current === request && currentAgent.current === agentId)
        setError(errorMessage(cause));
    } finally {
      if (generation.current === request) {
        setLoading(false);
        setBusy(false);
      }
    }
  };

  const choose = (
    kind: ViewKind,
    source: MemoryViewSource,
    selected: boolean,
  ) => {
    setPicks((current) => {
      const next: Picks = {
        userProfile: current.userProfile.filter(
          (item) => item.id !== source.id,
        ),
        agentNotes: current.agentNotes.filter((item) => item.id !== source.id),
      };
      if (selected)
        next[kind] = [
          ...next[kind],
          { id: source.id, sourceHash: source.sourceHash },
        ];
      return next;
    });
  };

  const saveSelection = async () => {
    if (
      !view ||
      busy ||
      loading ||
      (view.unavailable.length > 0 && !removeUnavailable)
    )
      return;
    const request = generation.current;
    setBusy(true);
    setError(null);
    try {
      const next = await daemon.selectMemoryViews(agentId, {
        expectedVersion: view.version,
        userProfile: picks.userProfile,
        agentNotes: picks.agentNotes,
      });
      if (generation.current !== request || currentAgent.current !== agentId)
        return;
      setView(next);
      setPicks(picksFrom(next));
      setSourceSnapshots((current) =>
        mergeSnapshots(current, sourcesFrom(next)),
      );
      setRemoveUnavailable(false);
      setError(null);
    } catch (cause) {
      if (generation.current === request && currentAgent.current === agentId)
        setError(errorMessage(cause));
    } finally {
      if (generation.current === request) setBusy(false);
    }
  };

  const correct = async (source: MemoryViewSource) => {
    if (
      busy ||
      loading ||
      selectionDirty ||
      !view ||
      savedHashes.get(source.id) !== source.sourceHash
    )
      return;
    const content = edits[source.id] ?? source.content;
    const request = generation.current;
    setBusy(true);
    setError(null);
    try {
      const next = await daemon.correctMemoryViewSource(agentId, source.id, {
        expectedVersion: view.version,
        expectedContent: source.content,
        expectedSourceHash: source.sourceHash,
        content,
      });
      if (generation.current !== request || currentAgent.current !== agentId)
        return;
      setView(next);
      setPicks(picksFrom(next));
      setSourceSnapshots((current) =>
        mergeSnapshots(current, sourcesFrom(next)),
      );
      setEdits((current) => {
        const updated = { ...current };
        delete updated[source.id];
        return updated;
      });
    } catch (cause) {
      if (generation.current === request && currentAgent.current === agentId)
        setError(errorMessage(cause));
    } finally {
      if (generation.current === request) setBusy(false);
    }
  };

  return (
    <section
      className="memory-group space-y-3"
      aria-labelledby="profile-memory-heading"
    >
      <h4 id="profile-memory-heading">Profile &amp; agent notes</h4>
      <p className="memory-note">
        These are selected saved facts, shown as owner-provided source material.
        They are not independently verified truths.
      </p>
      {error ? (
        <p className="memory-error" role="alert">
          {error}
        </p>
      ) : null}
      {loading ? (
        <p className="memory-note" role="status">
          Loading profile and notes…
        </p>
      ) : null}
      {view ? (
        <>
          <div className="grid gap-3 md:grid-cols-2">
            <SelectedSources
              title="Your profile"
              kind="userProfile"
              picks={picks.userProfile}
              sources={sourceSnapshots}
              savedHashes={savedHashes}
              edits={edits}
              selectionDirty={selectionDirty}
              busy={busy}
              onEdit={(id, value) =>
                setEdits((current) => ({ ...current, [id]: value }))
              }
              onCorrect={correct}
              onRemove={(source) => choose('userProfile', source, false)}
            />
            <SelectedSources
              title="Notes for this agent"
              kind="agentNotes"
              picks={picks.agentNotes}
              sources={sourceSnapshots}
              savedHashes={savedHashes}
              edits={edits}
              selectionDirty={selectionDirty}
              busy={busy}
              onEdit={(id, value) =>
                setEdits((current) => ({ ...current, [id]: value }))
              }
              onCorrect={correct}
              onRemove={(source) => choose('agentNotes', source, false)}
            />
          </div>

          {view.unavailable.length > 0 ? (
            <div
              className="memory-note"
              role="group"
              aria-label="Unavailable selected sources"
            >
              <p>
                Some selected sources are no longer available. Remove them
                explicitly before saving this selection:
              </p>
              <ul>
                {view.unavailable.map((source) => (
                  <li key={source.id}>Unavailable source {source.id}</li>
                ))}
              </ul>
              <button
                type="button"
                className="studio-tool-button"
                disabled={busy || removeUnavailable}
                onClick={() => setRemoveUnavailable(true)}
              >
                Remove unavailable sources
              </button>
            </div>
          ) : null}

          <form
            role="search"
            className="memory-search"
            onSubmit={(event) => {
              event.preventDefault();
              void searchCandidates(query.trim() || undefined);
            }}
          >
            <input
              type="search"
              aria-label="Find saved facts for profile or notes"
              maxLength={512}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
            <button
              type="submit"
              className="studio-tool-button"
              disabled={busy}
            >
              Find saved facts
            </button>
          </form>
          {candidates.candidates.length > 0 ? (
            <ul className="memory-list" aria-label="Eligible saved facts">
              {candidates.candidates.map((source) => {
                const profileSelected = picks.userProfile.some(
                  (item) => item.id === source.id,
                );
                const notesSelected = picks.agentNotes.some(
                  (item) => item.id === source.id,
                );
                return (
                  <li key={source.id} className="memory-item">
                    <p className="memory-content">
                      <RevealedText text={source.content} />
                    </p>
                    <div className="memory-actions">
                      <button
                        type="button"
                        className="studio-tool-button"
                        disabled={
                          busy ||
                          (view.unavailable.length > 0 &&
                            !removeUnavailable &&
                            !profileSelected &&
                            !notesSelected) ||
                          (!profileSelected &&
                            !notesSelected &&
                            displayedSelectedCount >= 6)
                        }
                        onClick={() =>
                          choose('userProfile', source, !profileSelected)
                        }
                      >
                        {profileSelected
                          ? 'Remove from profile'
                          : 'Add to profile'}
                      </button>
                      <button
                        type="button"
                        className="studio-tool-button"
                        disabled={
                          busy ||
                          (view.unavailable.length > 0 &&
                            !removeUnavailable &&
                            !notesSelected &&
                            !profileSelected) ||
                          (!notesSelected &&
                            !profileSelected &&
                            displayedSelectedCount >= 6)
                        }
                        onClick={() =>
                          choose('agentNotes', source, !notesSelected)
                        }
                      >
                        {notesSelected ? 'Remove from notes' : 'Add to notes'}
                      </button>
                    </div>
                  </li>
                );
              })}
            </ul>
          ) : !loading ? (
            <p className="memory-empty">No eligible saved facts found.</p>
          ) : null}
          {candidates.hasMore ? (
            <p className="memory-note">
              More saved facts match. Search to narrow the list.
            </p>
          ) : null}
          <div className="memory-actions">
            <button
              type="button"
              className="studio-tool-button"
              disabled={busy || loading}
              onClick={() => void refreshView()}
            >
              Refresh profile and notes
            </button>
            <button
              type="button"
              className="studio-tool-button"
              disabled={
                busy ||
                !selectionDirty ||
                (view.unavailable.length > 0 && !removeUnavailable)
              }
              onClick={() => void saveSelection()}
            >
              Save profile and notes
            </button>
            <span className="memory-note">
              {displayedSelectedCount} of 6 selected
            </span>
          </div>
          {selectionDirty ? (
            <p className="memory-note" role="status">
              Save your selection changes before correcting a fact.
            </p>
          ) : null}
        </>
      ) : null}
    </section>
  );
}

function SelectedSources({
  title,
  kind,
  picks,
  sources,
  savedHashes,
  selectionDirty,
  edits,
  busy,
  onEdit,
  onCorrect,
  onRemove,
}: {
  title: string;
  kind: ViewKind;
  picks: MemoryViewPick[];
  sources: Map<string, MemoryViewSource>;
  savedHashes: Map<string, string>;
  selectionDirty: boolean;
  edits: Record<string, string>;
  busy: boolean;
  onEdit: (id: string, value: string) => void;
  onCorrect: (source: MemoryViewSource) => void;
  onRemove: (source: MemoryViewSource) => void;
}) {
  return (
    <section className="memory-group" aria-label={title}>
      <h5>{title}</h5>
      {picks.length === 0 ? (
        <p className="memory-empty">No sources selected.</p>
      ) : null}
      {picks.map((pick) => {
        const source = sources.get(snapshotKey(pick.id, pick.sourceHash));
        if (!source)
          return (
            <p key={pick.id} className="memory-note">
              Unavailable source {pick.id} must be removed above.
            </p>
          );
        const value = edits[source.id] ?? source.content;
        const tooLong = Array.from(value).length > MAX_MEMORY_EDIT_CHARS;
        const isSaved = savedHashes.get(source.id) === pick.sourceHash;
        const hasCorrection =
          isSaved &&
          value !== source.content &&
          value.trim().length > 0 &&
          !tooLong;
        return (
          <article key={source.id} className="memory-item">
            <p className="memory-content">
              <RevealedText text={source.content} />
            </p>
            {isSaved ? (
              <label className="memory-field">
                Correct this saved fact
                <textarea
                  aria-label="Correct this saved fact"
                  value={value}
                  rows={4}
                  disabled={busy}
                  onChange={(event) => onEdit(source.id, event.target.value)}
                />
                <span className="memory-meta">
                  {Array.from(value).length} / {MAX_MEMORY_EDIT_CHARS}{' '}
                  characters
                </span>
                {tooLong ? (
                  <span role="alert" className="memory-error">
                    This correction is over the memory character limit.
                  </span>
                ) : null}
              </label>
            ) : null}
            <div className="memory-actions">
              <button
                type="button"
                className="studio-tool-button"
                disabled={busy}
                onClick={() => onRemove(source)}
              >
                Remove from {kind === 'userProfile' ? 'profile' : 'notes'}
              </button>
              {hasCorrection ? (
                <button
                  type="button"
                  className="studio-tool-button"
                  disabled={busy || selectionDirty}
                  onClick={() => onCorrect(source)}
                >
                  Save correction
                </button>
              ) : null}
            </div>
          </article>
        );
      })}
    </section>
  );
}

function picksFrom(view: MemoryViews): Picks {
  return {
    userProfile: view.userProfile.map(({ id, sourceHash }) => ({
      id,
      sourceHash,
    })),
    agentNotes: view.agentNotes.map(({ id, sourceHash }) => ({
      id,
      sourceHash,
    })),
  };
}

function sourcesFrom(view: MemoryViews): MemoryViewSource[] {
  return [...view.userProfile, ...view.agentNotes];
}

function mergeSnapshots(
  current: Map<string, MemoryViewSource>,
  sources: MemoryViewSource[],
  protectedPicks: MemoryViewPick[] = [],
): Map<string, MemoryViewSource> {
  const next = new Map(current);
  for (const source of sources)
    next.set(snapshotKey(source.id, source.sourceHash), source);
  const protectedKeys = new Set(
    protectedPicks.map((pick) => snapshotKey(pick.id, pick.sourceHash)),
  );
  for (const key of next.keys()) {
    if (next.size <= MAX_SOURCE_SNAPSHOTS) break;
    if (!protectedKeys.has(key)) next.delete(key);
  }
  return next;
}

function snapshotKey(id: string, sourceHash: string): string {
  return `${id}\0${sourceHash}`;
}

function samePicks(left: Picks, right: Picks): boolean {
  return (['userProfile', 'agentNotes'] as const).every(
    (kind) =>
      left[kind].length === right[kind].length &&
      left[kind].every(
        (item, index) =>
          item.id === right[kind][index]?.id &&
          item.sourceHash === right[kind][index]?.sourceHash,
      ),
  );
}

function errorMessage(cause: unknown): string {
  if (cause instanceof DaemonHttpError && cause.status === 409)
    return 'Saved facts or this selection changed. Your edits are kept. Refresh and review the current text before trying again.';
  return cause instanceof Error
    ? cause.message
    : 'Profile memory request failed.';
}
