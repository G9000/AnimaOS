import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react';
import { speechTextIsValid } from '@animaOS-SWARM/sdk';
import type { ChatMessage } from '../../lib/types';
import { daemon } from '../../lib/daemon-api';

export interface NativeSpeechProps {
  agentId: string;
  sessionId: string;
  conversationKey: string;
  messages: readonly ChatMessage[];
  disabled: boolean;
}

type Phase = 'checking' | 'generating' | 'ready' | 'playing' | 'cleanup';
interface SpeechOwner {
  scope: string;
  operationId: string;
  agentId: string;
  sessionId: string;
  controller: AbortController;
  phase: Phase;
  retired: boolean;
  postStarted: boolean;
  requestSettled: boolean;
  cancelStarted: boolean;
  cancelSettled: boolean;
  playSettled: boolean;
  cleanupCertain: boolean;
  blob: Blob | null;
  player: HTMLAudioElement | null;
  url: string | null;
}

// A single page owner survives component unmount while aborted fetch, DELETE, or
// a browser play promise is still unsettled. No second operation is queued.
let pageOwner: SpeechOwner | null = null;
let ownerVersion = 0;
const ownerSubscribers = new Set<() => void>();

function subscribeOwner(listener: () => void) {
  ownerSubscribers.add(listener);
  return () => ownerSubscribers.delete(listener);
}

function ownerSnapshot() {
  return ownerVersion;
}

function notifyOwner() {
  ownerVersion += 1;
  for (const listener of ownerSubscribers) listener();
}

function releaseIfSettled(owner: SpeechOwner) {
  if (
    pageOwner !== owner ||
    !owner.retired ||
    !owner.requestSettled ||
    (owner.cancelStarted && !owner.cancelSettled) ||
    !owner.playSettled ||
    !owner.cleanupCertain
  )
    return;
  owner.blob = null;
  pageOwner = null;
  notifyOwner();
}

function detachPlayer(owner: SpeechOwner): boolean {
  if (!owner.cleanupCertain) return false;
  const player = owner.player;
  const url = owner.url;
  if (!player && !url) return true;
  let certain = true;
  if (player) {
    try {
      player.onended = null;
    } catch {
      certain = false;
    }
    try {
      player.onerror = null;
    } catch {
      certain = false;
    }
    try {
      player.pause();
    } catch {
      certain = false;
    }
    try {
      player.removeAttribute('src');
    } catch {
      certain = false;
    }
    try {
      player.load();
    } catch {
      certain = false;
    }
  }
  if (url) {
    try {
      URL.revokeObjectURL(url);
    } catch {
      certain = false;
    }
  }
  if (certain) {
    owner.player = null;
    owner.url = null;
  } else {
    // The browser may still own a player or URL. Keep their owner until reload.
    owner.cleanupCertain = false;
    owner.phase = 'cleanup';
    owner.blob = null;
  }
  notifyOwner();
  return certain;
}

function retire(owner: SpeechOwner) {
  if (owner.retired) return;
  owner.retired = true;
  owner.blob = null;
  // Establish the DELETE obligation before abort or media cleanup can release
  // a fully settled owner. Its 2s signal is independent of the POST signal.
  const needsCancel = owner.postStarted && !owner.cancelStarted;
  if (needsCancel) owner.cancelStarted = true;
  owner.controller.abort();
  if (needsCancel) {
    const cancellation = new AbortController();
    const timer = window.setTimeout(() => cancellation.abort(), 2000);
    try {
      void daemon.speech
        .cancel(owner.agentId, owner.sessionId, owner.operationId, {
          signal: cancellation.signal,
        })
        .catch(() => undefined)
        .finally(() => {
          window.clearTimeout(timer);
          owner.cancelSettled = true;
          releaseIfSettled(owner);
          notifyOwner();
        });
    } catch {
      window.clearTimeout(timer);
      owner.cancelSettled = true;
    }
  }
  detachPlayer(owner);
  notifyOwner();
  releaseIfSettled(owner);
}

const UNAVAILABLE: Record<string, string> = {
  available: '',
  disabled: 'Native speech is disabled in the daemon.',
  missing_credentials: 'Native speech needs a configured OpenAI API key.',
  unsupported_configuration:
    'The daemon configuration does not support native speech.',
  unavailable: 'Native speech is currently unavailable.',
};

export function NativeSpeech({
  agentId,
  sessionId,
  conversationKey,
  messages,
  disabled,
}: NativeSpeechProps) {
  const selectId = useId();
  const [selectedId, setSelectedId] = useState('');
  useSyncExternalStore(subscribeOwner, ownerSnapshot, ownerSnapshot);
  const mounted = useRef(false);
  const ownerRef = useRef<SpeechOwner | null>(null);
  const [notice, setNotice] = useState('');
  const replies = messages
    .filter(
      (message) =>
        message.role === 'Assistant' &&
        /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/.test(message.id) &&
        speechTextIsValid(message.content.text) &&
        message.content.metadata?.hidden !== true,
    )
    .slice(-20)
    .reverse();
  const selected = replies.find((message) => message.id === selectedId);
  const scope = JSON.stringify([
    agentId,
    sessionId,
    conversationKey,
    selected?.id ?? null,
    selected?.content.text ?? null,
    disabled,
  ]);
  const committedScope = useRef(scope);
  const notify = notifyOwner;

  useLayoutEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (ownerRef.current) retire(ownerRef.current);
    };
  }, []);
  useLayoutEffect(() => {
    if (committedScope.current !== scope) {
      committedScope.current = scope;
      if (ownerRef.current) retire(ownerRef.current);
      ownerRef.current = null;
    }
    return () => {
      if (ownerRef.current?.scope === scope) retire(ownerRef.current);
    };
  }, [scope]);

  const owned =
    ownerRef.current?.scope === scope && !ownerRef.current.retired
      ? ownerRef.current
      : null;
  const pending = pageOwner !== null && pageOwner !== owned;
  const generate = () => {
    if (
      disabled ||
      !selected ||
      pageOwner ||
      !mounted.current ||
      committedScope.current !== scope
    )
      return;
    const owner: SpeechOwner = {
      scope,
      agentId,
      sessionId,
      operationId: crypto.randomUUID(),
      controller: new AbortController(),
      phase: 'checking',
      retired: false,
      postStarted: false,
      requestSettled: false,
      cancelStarted: false,
      cancelSettled: false,
      playSettled: true,
      cleanupCertain: true,
      blob: null,
      player: null,
      url: null,
    };
    pageOwner = owner;
    ownerRef.current = owner;
    setNotice('');
    notify();
    void (async () => {
      try {
        const cap = await daemon.speech.capabilities(agentId, sessionId, {
          signal: owner.controller.signal,
        });
        if (
          owner.retired ||
          committedScope.current !== scope ||
          !mounted.current
        )
          return;
        if (!cap.available) {
          setNotice(UNAVAILABLE[cap.reason]);
          return;
        }
        owner.phase = 'generating';
        owner.postStarted = true;
        notify();
        const blob = await daemon.speech.generate(
          agentId,
          sessionId,
          {
            operationId: owner.operationId,
            messageId: selected.id,
            text: selected.content.text,
            consent: true,
          },
          { signal: owner.controller.signal },
        );
        if (
          owner.retired ||
          owner.controller.signal.aborted ||
          committedScope.current !== scope ||
          !mounted.current
        )
          return;
        owner.blob = blob;
        owner.phase = 'ready';
        notify();
      } catch {
        if (
          !owner.retired &&
          mounted.current &&
          committedScope.current === scope
        ) {
          setNotice(
            'Speech could not finish. You can try again explicitly. Remote processing and API charges may still occur.',
          );
        }
      } finally {
        owner.requestSettled = true;
        if (!owner.blob && !owner.retired) retire(owner);
        releaseIfSettled(owner);
        notify();
      }
    })();
  };
  const play = () => {
    if (
      disabled ||
      !owned?.blob ||
      owned.player ||
      !owned.playSettled ||
      !owned.cleanupCertain
    )
      return;
    let player: HTMLAudioElement;
    let url: string;
    try {
      player = new Audio();
      url = URL.createObjectURL(owned.blob);
      owned.player = player;
      owned.url = url;
      owned.playSettled = false;
      owned.phase = 'playing';
      player.onended = () => {
        if (!owned.retired && owned.player === player) {
          if (detachPlayer(owned)) {
            owned.phase = 'ready';
            notify();
          } else retire(owned);
        }
      };
      player.onerror = () => {
        if (!owned.retired && owned.player === player) {
          if (detachPlayer(owned)) {
            owned.phase = 'ready';
            setNotice('Speech playback failed.');
            notify();
          } else retire(owned);
        }
      };
      player.src = url;
      // Install rejection observation immediately, including a synchronous throw.
      Promise.resolve(player.play()).then(
        () => {
          owned.playSettled = true;
          releaseIfSettled(owned);
          notify();
        },
        () => {
          owned.playSettled = true;
          if (owned.player === player) {
            if (detachPlayer(owned)) {
              if (!owned.retired) {
                owned.phase = 'ready';
                setNotice('Speech playback failed.');
              }
            } else retire(owned);
          }
          releaseIfSettled(owned);
          notify();
        },
      );
      notify();
    } catch {
      owned.playSettled = true;
      if (detachPlayer(owned)) {
        if (!owned.retired) setNotice('Speech playback failed.');
      } else retire(owned);
      notify();
    }
  };

  return (
    <section
      className="mt-2 border-t border-line px-4 py-2 text-xs text-ink-2"
      aria-label="Native speech"
    >
      <label htmlFor={selectId}>Assistant reply for speech</label>{' '}
      <select
        id={selectId}
        value={selected?.id ?? ''}
        disabled={disabled || pending}
        onChange={(event) => {
          if (ownerRef.current) retire(ownerRef.current);
          ownerRef.current = null;
          setSelectedId(event.target.value);
          setNotice('');
        }}
        className="rounded border border-line bg-panel p-1 text-ink"
      >
        <option value="">Choose an assistant reply</option>
        {replies.map((reply, index) => (
          <option key={reply.id} value={reply.id}>
            Reply {index + 1}: {reply.content.text.slice(0, 80)}
          </option>
        ))}
      </select>
      {selected && (
        <p className="mt-2 whitespace-pre-wrap break-words">
          Selected reply: {selected.content.text}
        </p>
      )}
      <p className="mt-2">
        Generate sends the selected text to OpenAI. OpenAI API charges may
        apply. The voice is AI-generated.
      </p>
      <button
        type="button"
        className="studio-tool-button mt-2"
        disabled={disabled || !selected || Boolean(pageOwner)}
        onClick={generate}
      >
        Generate speech with OpenAI API
      </button>
      {owned &&
        (owned.phase === 'checking' || owned.phase === 'generating') && (
          <button
            type="button"
            className="studio-tool-button ml-2"
            onClick={() => retire(owned)}
          >
            Stop generating speech
          </button>
        )}
      {owned?.blob && (
        <>
          <button
            type="button"
            className="studio-tool-button ml-2"
            disabled={
              disabled ||
              Boolean(owned.player) ||
              !owned.playSettled ||
              !owned.cleanupCertain
            }
            onClick={play}
          >
            Play speech
          </button>
          {owned.player && (
            <button
              type="button"
              className="studio-tool-button ml-2"
              onClick={() => {
                if (detachPlayer(owned)) {
                  owned.phase = 'ready';
                  notify();
                } else retire(owned);
              }}
            >
              Stop speech
            </button>
          )}
          <button
            type="button"
            className="studio-tool-button ml-2"
            onClick={() => retire(owned)}
          >
            Discard speech
          </button>
        </>
      )}
      {pending && (
        <p role="status">
          Speech cleanup pending. Reload if it does not finish.
        </p>
      )}
      {notice && <p role="status">{notice}</p>}
    </section>
  );
}
