import { useEffect, useId, useRef, useState } from 'react';
import type { VoiceCapabilities } from '@animaos/sdk';
import type { AttachmentDraft } from '../../lib/attachments';
import { daemon } from '../../lib/daemon-api';
import { revealInvisible } from '../../lib/skills';

export interface NativeVoiceScope {
  agentId: string;
  sessionId?: string;
  /** Reuses the same session as the selected READY upload, including new chats. */
  ensureSession(): Promise<string>;
}
interface Boundary {
  signature: string;
  generation: number;
}
interface Operation {
  boundary: Boundary;
  agentId: string;
  sessionId: string;
  attachmentId: string;
  operationId: string;
  controller: AbortController;
}
const UNAVAILABLE = {
  available: '',
  disabled:
    'Transcription is unavailable: the daemon has not enabled native transcription.',
  missing_credentials:
    'Transcription is unavailable: the daemon has no OpenAI API credentials configured.',
  unsupported_configuration:
    'Transcription is unavailable: the daemon configuration does not support the OpenAI API transcription service.',
  unavailable:
    'Transcription is unavailable. Try again after the daemon service is available.',
} as const;

/** Selected-file transcription stays ephemeral until the separate Use action. */
export function NativeTranscription({
  scope,
  conversationKey,
  items,
  disabled = false,
  onUse,
}: {
  scope: NativeVoiceScope;
  conversationKey?: string | null;
  items: readonly AttachmentDraft[];
  disabled?: boolean;
  onUse(text: string): void;
}) {
  const selectId = useId();
  const [selectedId, setSelectedId] = useState('');
  const audio = items.filter(
    (item) => item.status === 'ready' && item.attachment?.kind === 'audio',
  );
  const selected = audio.find(
    (item) => item.attachment!.id === selectedId,
  )?.attachment;
  const signature = JSON.stringify([
    conversationKey,
    scope.agentId,
    scope.sessionId,
    selected?.id ?? null,
    disabled,
  ]);
  const boundary = useRef<Boundary>({ signature, generation: 0 });
  if (boundary.current.signature !== signature)
    boundary.current = {
      signature,
      generation: boundary.current.generation + 1,
    };
  const current = boundary.current;
  const mounted = useRef(true);
  const scopeRef = useRef(scope);
  scopeRef.current = scope;
  const operation = useRef<Operation | null>(null);
  const [capability, setCapability] = useState<{
    boundary: Boundary;
    sessionId: string;
    value: VoiceCapabilities;
  } | null>(null);
  const [pending, setPending] = useState<Boundary | null>(null);
  const [preview, setPreview] = useState<{
    boundary: Boundary;
    text: string;
  } | null>(null);
  const [notice, setNotice] = useState<{
    boundary: Boundary;
    text: string;
  } | null>(null);
  const live = (owner: Boundary) =>
    mounted.current && boundary.current === owner;

  const cancel = () => {
    const previous = operation.current;
    if (!previous) return;
    operation.current = null;
    // Dispatch before aborting the POST; the DELETE must not inherit its signal.
    try {
      void daemon.voice
        .cancel(previous.agentId, previous.sessionId, previous.operationId)
        .catch(() => undefined);
    } finally {
      previous.controller.abort();
    }
  };
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      cancel();
    };
  }, []);
  useEffect(() => {
    const controller = new AbortController();
    setCapability(null);
    setPreview(null);
    setPending(null);
    setNotice(null);
    if (selected && !disabled) {
      const target = scopeRef.current;
      void (async () => {
        try {
          const sessionId = target.sessionId ?? (await target.ensureSession());
          if (!live(current) || controller.signal.aborted) return;
          const value = await daemon.voice.capabilities(
            target.agentId,
            sessionId,
            { signal: controller.signal },
          );
          if (live(current) && !controller.signal.aborted)
            setCapability({ boundary: current, sessionId, value });
        } catch {
          if (live(current) && !controller.signal.aborted)
            setNotice({ boundary: current, text: UNAVAILABLE.unavailable });
        }
      })();
    }
    return () => {
      controller.abort();
      cancel();
    };
  }, [signature]);

  const activeCapability = capability?.boundary === current ? capability : null;
  const activePreview = preview?.boundary === current ? preview : null;
  const activeNotice = notice?.boundary === current ? notice.text : null;
  const busy = pending === current;
  const start = () => {
    if (
      disabled ||
      !selected ||
      !live(current) ||
      !activeCapability?.value.available ||
      operation.current ||
      busy ||
      activePreview
    )
      return;
    const op: Operation = {
      boundary: current,
      agentId: scope.agentId,
      sessionId: activeCapability.sessionId,
      attachmentId: selected.id,
      operationId: crypto.randomUUID(),
      controller: new AbortController(),
    };
    operation.current = op;
    setPending(current);
    setNotice(null);
    void (async () => {
      try {
        const result = await daemon.voice.transcribe(
          op.agentId,
          op.sessionId,
          {
            operationId: op.operationId,
            attachmentId: op.attachmentId,
            provider: 'openai',
            consent: true,
          },
          { signal: op.controller.signal },
        );
        if (
          live(op.boundary) &&
          operation.current === op &&
          !op.controller.signal.aborted
        )
          setPreview({ boundary: current, text: result.text });
      } catch {
        if (
          live(op.boundary) &&
          operation.current === op &&
          !op.controller.signal.aborted
        )
          setNotice({
            boundary: current,
            text: 'Transcription could not finish. You can try again explicitly. Remote processing and API charges may still occur.',
          });
      } finally {
        if (operation.current === op) {
          operation.current = null;
          if (live(op.boundary)) setPending(null);
        }
      }
    })();
  };
  if (!audio.length) return null;
  return (
    <section
      className="studio-native-transcription mt-2 rounded-xl border border-line p-3 text-xs text-ink-2"
      data-live
      aria-label="Native audio transcription"
    >
      <label htmlFor={selectId}>Audio to transcribe</label>{' '}
      <select
        id={selectId}
        value={selected?.id ?? ''}
        disabled={disabled}
        onChange={(event) => {
          cancel();
          setSelectedId(event.target.value);
        }}
        className="rounded border border-line bg-panel p-1 text-ink"
      >
        <option value="">Choose an uploaded WAV file</option>
        {audio.map((item) => (
          <option key={item.localId} value={item.attachment!.id}>
            {revealInvisible(item.name).text}
          </option>
        ))}
      </select>
      {selected && (
        <>
          <p className="mt-2">
            The selected audio will be sent to OpenAI for transcription. OpenAI
            API charges may apply. This uses the daemon’s configured OpenAI API
            account. Review the transcript before adding it to your draft.
          </p>
          {!activeCapability && !activeNotice && !disabled && (
            <p role="status">Checking transcription availability…</p>
          )}
          {activeCapability && !activeCapability.value.available && (
            <p role="status">{UNAVAILABLE[activeCapability.value.reason]}</p>
          )}
          {activeCapability?.value.available &&
            !busy &&
            !activePreview &&
            !disabled && (
              <button
                type="button"
                className="studio-tool-button mt-2"
                onClick={start}
              >
                Transcribe with OpenAI API
              </button>
            )}
          {busy && (
            <div role="status" className="mt-2">
              Transcribing selected audio…{' '}
              <button
                type="button"
                className="studio-tool-button"
                onClick={() => {
                  cancel();
                  setPending(null);
                  setPreview(null);
                  setNotice({
                    boundary: current,
                    text: 'Stopped waiting locally. Remote processing and API charges may still occur.',
                  });
                }}
              >
                Stop transcription
              </button>
            </div>
          )}
          {activePreview && !disabled && (
            <div className="mt-2">
              <label>
                Transcript preview
                <textarea
                  aria-label="Transcript preview"
                  className="mt-1 block w-full rounded border border-line bg-panel p-2 text-ink"
                  rows={3}
                  value={activePreview.text}
                  onChange={(event) =>
                    setPreview({ boundary: current, text: event.target.value })
                  }
                />
              </label>
              <button
                type="button"
                className="studio-tool-button mt-2"
                disabled={!activePreview.text.trim()}
                onClick={() => {
                  if (
                    !live(current) ||
                    disabled ||
                    !selected ||
                    preview?.boundary !== current ||
                    !preview.text.trim()
                  )
                    return;
                  onUse(preview.text.trim());
                  setPreview(null);
                }}
              >
                Use transcript
              </button>{' '}
              <button
                type="button"
                className="studio-tool-button"
                onClick={() => setPreview(null)}
              >
                Discard
              </button>
            </div>
          )}
          {activeNotice && (
            <p role="status" className="mt-2">
              {activeNotice}
            </p>
          )}
        </>
      )}
    </section>
  );
}
