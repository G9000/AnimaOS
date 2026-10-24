import { useEffect, useRef, useState } from 'react';
import { VillageApiError, villageApi } from './api';
import { DialoguePanel } from './Dialogue';
import { VillageScene } from './VillageScene';
import {
  readStorage,
  writeStorage,
  removeStorage,
  savedRequest,
  type PendingRequest,
} from './pending';
import type { GameView, Language, NpcId, Suggestion, TurnInput } from './types';
function messageOf(error: unknown) {
  return error instanceof Error
    ? error.message
    : 'The village connection was interrupted. Please try again.';
}

export function VillageApp({ api = villageApi }: { api?: typeof villageApi }) {
  const [language, setLanguage] = useState<Language>(() =>
    readStorage('village.language') === 'zh' ? 'zh' : 'ja',
  );
  const [view, setView] = useState<GameView | null>(null);
  const [open, setOpen] = useState(false);
  const [near, setNear] = useState<NpcId | null>(null);
  const [speaker, setSpeaker] = useState<NpcId>('tavi');
  const [approach, setApproach] = useState(0);
  const [approachNpc, setApproachNpc] = useState<NpcId>('tavi');
  const [book, setBook] = useState(false);
  const [note, setNote] = useState(true);
  const [busy, setBusy] = useState(false);
  const [stalled, setStalled] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const boot = useRef(new Map<string, Promise<GameView>>());
  const sequence = useRef(0);
  const activeRequest = useRef<PendingRequest | null>(null);
  const [pendingRequest, setPendingRequest] = useState<PendingRequest | null>(
    null,
  );
  const interactButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    let cancelled = false;
    sequence.current++;
    writeStorage('village.language', language);
    const key = `${language}:${reload}`;
    if (!boot.current.has(key)) {
      const id = readStorage(`village.save.${language}`);
      boot.current.set(key, id ? api.restore(id) : api.create(language));
    }
    const operation = boot.current.get(key)!;
    void operation
      .then((result) => {
        if (cancelled) return;
        writeStorage(`village.save.${language}`, result.saveId);
        // Retired creator requests must never block an existing save.
        removeStorage(`village.appearance.${result.saveId}`);
        setView(result);
        const pending = savedRequest(result.saveId);
        activeRequest.current = pending;
        setPendingRequest(pending);
        setSpeaker(
          pending
            ? (pending.npcId ?? pending.input?.npcId ?? 'tavi')
            : (result.npcId ?? 'tavi'),
        );
        if (pending) {
          setOpen(true);
          setError(
            'Your last conversation was interrupted. Retry it or choose a prepared reply.',
          );
        }
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(messageOf(reason));
      })
      .finally(() => {
        if (boot.current.get(key) === operation) boot.current.delete(key);
      });
    return () => {
      cancelled = true;
    };
  }, [api, language, reload]);
  useEffect(() => {
    if (!busy) return;
    const timer = window.setTimeout(() => setStalled(true), 8000);
    return () => window.clearTimeout(timer);
  }, [busy]);

  const run = async (action: () => Promise<GameView>, submittedId: string) => {
    const version = ++sequence.current;
    setBusy(true);
    setStalled(false);
    setError(null);
    setNotice(null);
    try {
      const result = await action();
      if (version === sequence.current) {
        setView(result);
        activeRequest.current = null;
        setPendingRequest(null);
        removeStorage(`village.pending.${result.saveId}`);
      }
    } catch (reason) {
      if (version === sequence.current) {
        if (
          reason instanceof VillageApiError &&
          [400, 413, 422].includes(reason.status) &&
          activeRequest.current?.id === submittedId
        ) {
          activeRequest.current = null;
          setPendingRequest(null);
          if (view) removeStorage(`village.pending.${view.saveId}`);
          setNotice(messageOf(reason));
        } else setError(messageOf(reason));
      }
    } finally {
      if (version === sequence.current) setBusy(false);
    }
  };
  const enter = () => {
    if (!view || !near) return;
    setOpen(true);
    setBook(false);
    if (busy) return;
    if (activeRequest.current) {
      setSpeaker(
        activeRequest.current.npcId ??
          activeRequest.current.input?.npcId ??
          'tavi',
      );
      retry();
      return;
    }
    const requestId = crypto.randomUUID();
    setSpeaker(near);
    activeRequest.current = {
      id: requestId,
      kind: 'visit',
      npcId: near,
      ...(view.npcId === near ? { dialogue: view.dialogue } : {}),
    };
    setPendingRequest(activeRequest.current);
    writeStorage(
      `village.pending.${view.saveId}`,
      JSON.stringify(activeRequest.current),
    );
    void run(() => api.visit(view.saveId, requestId, near), requestId);
  };
  const send = (message: string, choiceId?: string) => {
    if (!view || busy || activeRequest.current) return;
    if (message.length > 1000 || message.includes('\0')) {
      setNotice('Please use up to 1000 text characters in one reply.');
      return;
    }
    const input: TurnInput = {
      requestId: crypto.randomUUID(),
      message,
      npcId: speaker,
      ...(choiceId ? { choiceId } : {}),
    };
    activeRequest.current = {
      id: input.requestId,
      kind: 'turn',
      npcId: speaker,
      input,
      dialogue: view.dialogue,
    };
    setPendingRequest(activeRequest.current);
    writeStorage(
      `village.pending.${view.saveId}`,
      JSON.stringify(activeRequest.current),
    );
    void run(() => api.turn(view.saveId, input), input.requestId);
  };
  const recover = (suggestion: Suggestion) => {
    if (!view) return;
    const original = activeRequest.current?.id;
    const input: TurnInput = {
      requestId: crypto.randomUUID(),
      message: suggestion.text,
      choiceId: suggestion.id,
      npcId:
        activeRequest.current?.npcId ??
        activeRequest.current?.input?.npcId ??
        speaker,
      ...(original ? { recoveryOf: original } : {}),
    };
    // The original receipt resolves to the winning outcome even if this recovery response is lost.
    if (!original) {
      activeRequest.current = {
        id: input.requestId,
        kind: 'turn',
        npcId: input.npcId,
        input,
        dialogue: view.dialogue,
      };
      setPendingRequest(activeRequest.current);
      writeStorage(
        `village.pending.${view.saveId}`,
        JSON.stringify(activeRequest.current),
      );
    }
    void run(() => api.turn(view.saveId, input), input.requestId);
  };
  const retry = () => {
    if (!view) {
      setReload((value) => value + 1);
      return;
    }
    const pending = activeRequest.current;
    if (!pending || (busy && !stalled)) return;
    void run(
      () =>
        pending.kind === 'visit'
          ? pending.npcId
            ? api.visit(view.saveId, pending.id, pending.npcId)
            : api.visit(view.saveId, pending.id)
          : api.turn(view.saveId, pending.input!),
      pending.id,
    );
  };
  const close = () => {
    setOpen(false);
    if (!activeRequest.current) setError(null);
    setNotice(null);
    requestAnimationFrame(() => interactButton.current?.focus());
  };
  const switchLanguage = (next: Language) => {
    if (next === language) return;
    sequence.current++;
    setView(null);
    setError(null);
    setNotice(null);
    setOpen(false);
    setBook(false);
    setBusy(false);
    setStalled(false);
    activeRequest.current = null;
    setPendingRequest(null);
    setLanguage(next);
  };

  const target: NpcId = view?.questStatus === 'collected' ? 'mira' : 'tavi';
  const nextNpc = near ?? target;
  const npcName = nextNpc === 'mira' ? 'Mira' : 'Tavi';
  const breadQuantity =
    view?.inventory
      .filter((item) => item.id === 'mira_loaves')
      .reduce((total, item) => total + item.quantity, 0) ?? 0;
  const pending = pendingRequest;
  const dialogueView =
    view && speaker !== view.npcId
      ? {
          ...view,
          npcId: speaker,
          dialogue: pending?.dialogue ?? {
            ...view.dialogue,
            text: '',
            translation: `A moment with ${speaker === 'mira' ? 'Mira' : 'Tavi'}…`,
            reading: '',
            suggestions:
              speaker === 'mira'
                ? [
                    {
                      id: 'ask_supper',
                      text:
                        view.language === 'ja'
                          ? '村の夕食はどうですか？'
                          : '村里的晚餐怎么样？',
                      translation: 'How is the village supper?',
                    },
                  ]
                : view.questStatus === 'pending' && view.phase === 'day'
                  ? [
                      {
                        id: 'collect_order',
                        text:
                          view.language === 'ja'
                            ? 'ミラさんのパンを二つください。'
                            : '请给我米拉订的两个面包。',
                        translation: 'Two loaves for Mira, please.',
                      },
                    ]
                  : [
                      {
                        id: 'ask_recipe',
                        text:
                          view.language === 'ja'
                            ? '新しいレシピはどうですか。'
                            : '新食谱怎么样？',
                        translation: 'How is the new recipe going?',
                      },
                      {
                        id: 'help',
                        text:
                          view.language === 'ja'
                            ? 'きっとおいしいですよ。応援しています。'
                            : '一定很好吃，我支持你。',
                        translation:
                          "I'm sure it'll be delicious. I'm cheering for you.",
                      },
                    ],
          },
        }
      : view;

  return (
    <main className={`village-app phase-${view?.phase ?? 'day'}`}>
      <VillageScene
        paused={open || book || !view}
        phase={view?.phase ?? 'day'}
        preference={view?.breadPreference ?? null}
        collected={view?.questStatus === 'collected'}
        approachSignal={approach}
        approachNpc={approachNpc}
        onNear={setNear}
        onInteract={enter}
      />
      <div className="world-vignette" />
      <header className="village-header">
        <div className="place-marker">
          <span className="place-emblem">✧</span>
          <div>
            <span className="eyebrow">A LITTLE LANGUAGE ADVENTURE</span>
            <h1>Lantern Village</h1>
          </div>
        </div>
        <div className="chapter-tag">
          <span className="sun-symbol">☼</span>
          <div>
            <span>CHAPTER ONE</span>
            <strong>
              {view?.phase === 'complete'
                ? 'A familiar light'
                : view?.phase === 'evening'
                  ? 'The lanterns are lit'
                  : 'Before the supper'}
            </strong>
          </div>
        </div>
        <div className="header-actions">
          <div className="language-switch" aria-label="Practice language">
            <button
              disabled={busy || open}
              aria-pressed={language === 'ja'}
              onClick={() => switchLanguage('ja')}
            >
              日本語 <span>JP</span>
            </button>
            <button
              disabled={busy || open}
              aria-pressed={language === 'zh'}
              onClick={() => switchLanguage('zh')}
            >
              中文 <span>ZH</span>
            </button>
          </div>
          <button
            className="book-toggle"
            disabled={open}
            aria-label="Open phrase journal"
            aria-expanded={book}
            onClick={() => setBook(!book)}
          >
            <BookIcon />
          </button>
        </div>
      </header>
      {!open && (
        <>
          <aside className={`quest-card ${note ? 'expanded' : ''}`}>
            <button
              className="quest-title"
              onClick={() => setNote(!note)}
              aria-expanded={note}
            >
              <span className="quest-symbol">✦</span>
              <div>
                <span className="eyebrow">A FAVOR FOR MIRA</span>
                <h2>
                  {view?.phase === 'complete'
                    ? 'A place at the table'
                    : view?.questStatus === 'delivered'
                      ? 'An evening thank-you'
                      : view?.questStatus === 'collected'
                        ? 'A basket for supper'
                        : 'The bread before supper'}
                </h2>
              </div>
              <span>{note ? '−' : '+'}</span>
            </button>
            {note && (
              <div className="quest-body">
                <p className="handwritten">
                  “Would you collect my two loaves from Tavi? I’ll save you a
                  place at the table.”
                </p>
                <span className="note-signature">— Mira</span>
                <div className="quest-divider" />
                <ol className="quest-steps" aria-label="Village day progress">
                  <li
                    className={
                      view && view.questStatus !== 'pending'
                        ? 'done'
                        : 'current'
                    }
                  >
                    <span>
                      {view && view.questStatus !== 'pending' ? '✓' : '1'}
                    </span>
                    Collect two loaves from Tavi
                  </li>
                  <li
                    className={
                      view?.questStatus === 'delivered'
                        ? 'done'
                        : view?.questStatus === 'collected'
                          ? 'current'
                          : ''
                    }
                  >
                    <span>{view?.questStatus === 'delivered' ? '✓' : '2'}</span>
                    Bring Mira her supper bread
                  </li>
                  <li
                    className={
                      view?.phase === 'complete'
                        ? 'done'
                        : view?.phase === 'evening'
                          ? 'current'
                          : ''
                    }
                  >
                    <span>{view?.phase === 'complete' ? '✓' : '3'}</span>Return
                    to Tavi at lantern time
                  </li>
                </ol>
                <p className="objective">
                  {view?.phase === 'complete'
                    ? 'Your village day is complete. Stay, explore, and talk a little longer.'
                    : view?.questStatus === 'delivered'
                      ? 'Return to Tavi for an evening conversation.'
                      : view?.questStatus === 'collected'
                        ? 'Mira’s order is in your basket.'
                        : 'Find Tavi at the village shop.'}
                </p>
                {pendingRequest && (
                  <button
                    className="resume-conversation"
                    onClick={() => setOpen(true)}
                  >
                    Resume interrupted conversation
                  </button>
                )}
                <button
                  className="approach-button"
                  disabled={!view}
                  onClick={() => {
                    setApproachNpc(target);
                    setApproach((value) => value + 1);
                    interactButton.current?.blur();
                  }}
                >
                  {target === 'mira'
                    ? 'Walk to Mira'
                    : view?.questStatus === 'delivered'
                      ? 'Return to Tavi'
                      : 'Walk to the shop'}{' '}
                  <span>↗</span>
                </button>
              </div>
            )}
          </aside>
          {view && (
            <div className={`interaction-prompt ${near ? 'in-reach' : ''}`}>
              <span className="npc-location">
                {nextNpc === 'mira'
                  ? 'Mira’s supper table'
                  : 'Tavi’s little shop'}
              </span>
              <button
                ref={interactButton}
                aria-label={
                  near ? `Talk to ${npcName}` : `Walk closer to ${npcName}`
                }
                disabled={!near || busy}
                onClick={enter}
              >
                <kbd>E</kbd>
                {near ? `Talk to ${npcName}` : `Walk closer to ${npcName}`}
                <span>↗</span>
              </button>
            </div>
          )}
          <footer className="exploration-footer">
            <div className="movement-help">
              <div className="key-group">
                <kbd>W</kbd>
                <span>
                  <kbd>A</kbd>
                  <kbd>S</kbd>
                  <kbd>D</kbd>
                </span>
              </div>
              <div>
                <span>Take a little walk</span>
                <small>or click a place to go</small>
              </div>
            </div>
            <div className="inventory">
              <span className="inventory-label">YOUR SATCHEL</span>
              <div className="inventory-slot note-slot" title="Mira’s note">
                ▱
              </div>
              <div
                className={`inventory-slot ${breadQuantity ? 'has-bread' : ''}`}
                title={
                  breadQuantity
                    ? 'Mira’s bread order'
                    : 'An empty place for something good'
                }
              >
                {breadQuantity ? (
                  <>
                    <BreadIcon />
                    <small>{breadQuantity}</small>
                  </>
                ) : (
                  <span>·</span>
                )}
              </div>
              <span className="coin-count">
                <span>◉</span>
                {view?.coins ?? 0}
              </span>
            </div>
            <div className="save-status">
              <span
                className={`connection-dot ${view?.runtime.mode === 'live' ? 'live' : ''}`}
              />
              {view
                ? view.runtime.mode === 'live'
                  ? 'Live conversation · progress saved'
                  : 'Reviewed dialogue · progress saved'
                : 'Opening the village…'}
            </div>
          </footer>
        </>
      )}
      {!view && (
        <div className="connection-card ornamental">
          {error ? (
            <>
              <span className="eyebrow">THE VILLAGE IS STILL HERE</span>
              <h2>A moment at the gate</h2>
              <p role="alert">{error}</p>
              <button className="gold-button" onClick={retry}>
                Try again <span>↗</span>
              </button>
            </>
          ) : (
            <>
              <span className="loading-star">✧</span>
              <h2>A light in the window</h2>
              <p>Opening your place in the village…</p>
            </>
          )}
        </div>
      )}
      {view && open && dialogueView && (
        <DialoguePanel
          key={`${view.saveId}:${speaker}`}
          view={dialogueView}
          pending={busy}
          stalled={busy && stalled}
          error={error}
          notice={notice}
          onSend={send}
          onRecover={recover}
          onRetry={retry}
          onClose={close}
        />
      )}
      {view && error && !open && (
        <div className="small-error" role="alert">
          {error}
          <button onClick={retry}>Retry</button>
        </div>
      )}
      {view && book && (
        <aside
          className="journal-panel ornamental"
          role="dialog"
          aria-label="Phrase journal"
        >
          <div className="journal-heading">
            <div>
              <span className="eyebrow">THINGS TO KEEP</span>
              <h2>Your little notebook</h2>
            </div>
            <button
              className="close-button"
              aria-label="Close phrase journal"
              onClick={() => setBook(false)}
            >
              ×
            </button>
          </div>
          <p className="journal-intro">Words you found along the way.</p>
          {view.journal.length ? (
            view.journal.map((phrase) => (
              <article
                className="journal-phrase"
                key={`${phrase.text}-${phrase.translation}`}
              >
                <p lang={view.language}>{phrase.text}</p>
                <small>{phrase.reading}</small>
                <span>{phrase.translation}</span>
              </article>
            ))
          ) : (
            <div className="journal-empty">
              <span>✧</span>
              <p>A conversation is a good place to begin.</p>
            </div>
          )}
          {view.memories.length > 0 && (
            <div className="journal-memories">
              <span className="eyebrow">A LITTLE MORE FAMILIAR</span>
              {view.memories.map((memory) => (
                <p key={memory}>{memory}</p>
              ))}
            </div>
          )}
        </aside>
      )}
    </main>
  );
}

function BookIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      aria-hidden="true"
    >
      <path d="M4 4.5c4-1 6-.5 8 1.5 2-2 4-2.5 8-1.5v15c-4-1-6-.5-8 1.5-2-2-4-2.5-8-1.5zM12 6v15" />
      <path d="M6.5 8H9M6.5 11H9M15 8h2.5M15 11h2.5" />
    </svg>
  );
}
function BreadIcon() {
  return (
    <svg viewBox="0 0 28 24" fill="none" aria-hidden="true">
      <path
        d="M4 17C1 11 6 5 11 5c2-4 9-3 11 1 5 1 6 9 2 12l-1 3H5z"
        fill="#c69656"
        stroke="#f1d5a2"
      />
      <path d="m10 7 2 6m5-8 2 7" stroke="#805531" strokeWidth="2" />
    </svg>
  );
}
