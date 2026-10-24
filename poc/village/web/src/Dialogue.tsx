import { useEffect, useRef, useState } from 'react';
import { shouldSubmitDialogue } from './movement';
import type { GameView, Suggestion } from './types';
export interface DialogueProps {
  view: GameView;
  pending: boolean;
  stalled: boolean;
  error: string | null;
  notice?: string | null;
  onSend: (message: string, choiceId?: string) => void;
  onRecover: (suggestion: Suggestion) => void;
  onRetry?: () => void;
  onClose: () => void;
}
export function DialoguePanel({
  view,
  pending,
  stalled,
  error,
  notice,
  onSend,
  onRecover,
  onRetry,
  onClose,
}: DialogueProps) {
  const [message, setMessage] = useState('');
  const [selected, setSelected] = useState<string>();
  const [meaning, setMeaning] = useState(true);
  const [reading, setReading] = useState(false);
  const composing = useRef(false);
  const input = useRef<HTMLTextAreaElement>(null);
  const blocked = pending || Boolean(error);
  const name = view.npcId === 'mira' ? 'Mira' : 'Tavi';
  const panel = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!pending) input.current?.focus();
  }, [pending]);
  const send = () => {
    if (!message.trim() || blocked || composing.current) return;
    onSend(
      message.trim(),
      view.runtime.mode === 'reviewed' ? selected : undefined,
    );
    setMessage('');
    setSelected(undefined);
  };
  const expression =
    { warm: '0%', thoughtful: '50%', happy: '100%' }[
      view.dialogue.expression
    ] ?? '0%';
  return (
    <section className="conversation" aria-label={`Conversation with ${name}`}>
      <div className="conversation-shade" />
      <div className="portrait-stage" aria-hidden="true">
        <div className="portrait-halo" />
        <div
          className="tavi-portrait"
          style={{
            backgroundPosition: `${expression} center`,
            backgroundImage:
              view.npcId === 'mira'
                ? "url('/art/mira-portraits.png')"
                : undefined,
          }}
        />
      </div>
      <div
        ref={panel}
        className="dialogue-card ornamental"
        role="dialog"
        aria-modal="true"
        aria-labelledby="speaker-name"
        onKeyDown={(event) => {
          if (event.key === 'Escape' && !composing.current) {
            event.stopPropagation();
            onClose();
          }
          if (event.key === 'Tab') {
            const buttons = panel.current?.querySelectorAll<HTMLElement>(
              'button:not(:disabled), textarea:not(:disabled)',
            );
            if (!buttons?.length) return;
            const first = buttons[0];
            const last = buttons[buttons.length - 1];
            if (event.shiftKey && document.activeElement === first) {
              event.preventDefault();
              last?.focus();
            } else if (!event.shiftKey && document.activeElement === last) {
              event.preventDefault();
              first?.focus();
            }
          }
        }}
      >
        <div className="dialogue-heading">
          <div>
            <span className="eyebrow">
              {view.npcId === 'mira'
                ? 'THE SUPPER ORGANIZER'
                : 'THE SHOPKEEPER'}
            </span>
            <h2 id="speaker-name">{name}</h2>
          </div>
          <button
            className="close-button"
            onClick={onClose}
            aria-label="Leave conversation"
          >
            ×
          </button>
        </div>
        <div className="dialogue-rule">
          <span />✧<span />
        </div>
        <div
          className="spoken-line"
          aria-live="polite"
          lang={view.language === 'ja' ? 'ja' : 'zh-Hans'}
        >
          {view.dialogue.text}
        </div>
        {reading && <p className="reading-line">{view.dialogue.reading}</p>}
        {meaning && <p className="meaning-line">{view.dialogue.translation}</p>}
        <div className="language-tools">
          <button
            className={meaning ? 'selected' : ''}
            aria-pressed={meaning}
            onClick={() => setMeaning(!meaning)}
          >
            Meaning
          </button>
          <button
            className={reading ? 'selected' : ''}
            aria-pressed={reading}
            onClick={() => setReading(!reading)}
          >
            {view.language === 'ja' ? 'Reading' : 'Pinyin'}
          </button>
          {view.dialogue.source === 'reviewed' && (
            <span className="reply-source">Reviewed dialogue</span>
          )}
        </div>
        <div className="reply-area">
          <p className="reply-prompt">
            {view.phase === 'complete'
              ? 'A place at the table, and time for another conversation.'
              : view.npcId === 'mira'
                ? view.questStatus === 'collected'
                  ? 'Give Mira her two loaves, or ask about supper.'
                  : view.questStatus === 'delivered'
                    ? 'The bread has arrived. Return to Tavi for an evening conversation.'
                    : 'Ask Mira about the village supper.'
                : view.questStatus === 'pending'
                  ? 'Ask about Mira’s order, or get to know Tavi.'
                  : view.questStatus === 'collected'
                    ? 'The bread is ready. Mira is waiting by the supper table.'
                    : 'Tell Tavi how the delivery went.'}
          </p>
          <div className="suggestions" aria-label="Reply suggestions">
            {view.dialogue.suggestions.map((suggestion) => (
              <button
                key={suggestion.id}
                disabled={blocked}
                onClick={() => {
                  setMessage(suggestion.text);
                  setSelected(suggestion.id);
                  input.current?.focus();
                }}
              >
                <span lang={view.language}>{suggestion.text}</span>
                <small>{suggestion.translation}</small>
              </button>
            ))}
          </div>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              send();
            }}
          >
            <label className="sr-only" htmlFor="reply">
              Your reply
            </label>
            <div className="reply-input">
              <textarea
                id="reply"
                ref={input}
                value={message}
                disabled={blocked}
                rows={2}
                maxLength={1000}
                placeholder={
                  view.language === 'ja'
                    ? '日本語で話してみよう…'
                    : '试着用中文说说吧…'
                }
                onChange={(event) => {
                  setMessage(event.target.value);
                  setSelected(undefined);
                }}
                onCompositionStart={() => {
                  composing.current = true;
                }}
                onCompositionEnd={() => {
                  composing.current = false;
                }}
                onKeyDown={(event) => {
                  event.stopPropagation();
                  if (
                    shouldSubmitDialogue({
                      key: event.key,
                      isComposing:
                        composing.current || event.nativeEvent.isComposing,
                      keyCode: event.nativeEvent.keyCode,
                      shiftKey: event.shiftKey,
                    })
                  ) {
                    event.preventDefault();
                    send();
                  }
                }}
              />
              <button
                className="send-button"
                type="submit"
                disabled={!message.trim() || blocked}
                aria-label="Send reply"
              >
                ↗
              </button>
            </div>
            <div className="input-caption">
              <span>
                {pending
                  ? `${name} is thinking…`
                  : 'A few words are enough. You can always ask for help.'}
              </span>
              <kbd>Enter ↵</kbd>
            </div>
          </form>
        </div>
        {(error || notice) && (
          <p role="alert" className="dialogue-error">
            {error ?? notice}
          </p>
        )}
        {(stalled || error) && (
          <div className="recovery-panel">
            <p>Retry your message or keep going with a prepared reply.</p>
            <button onClick={onRetry}>Retry my last message</button>
            {view.dialogue.suggestions.map((suggestion) => (
              <button
                key={suggestion.id}
                onClick={() => onRecover(suggestion)}
                aria-label={`Use prepared reply: ${suggestion.translation}`}
              >
                {suggestion.translation}
              </button>
            ))}
          </div>
        )}
        <div className="conversation-footer">
          <span className="tiny-star">✦</span> A small conversation. A little
          more familiar.
          <button onClick={onClose}>
            Back to the village <span>↗</span>
          </button>
        </div>
      </div>
    </section>
  );
}
