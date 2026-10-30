import { useEffect, useId, useRef, useState } from 'react';
import { PaperclipIcon, PlusIcon, TerminalIcon } from './icons';

/**
 * The + at the start of the composer: attach files, or start a command.
 * Either half may be missing (a chat without attachments, or without
 * commands); with neither there is no button.
 */
export function ComposerAdd({
  attachments,
  commands,
}: {
  attachments?: { disabled: boolean; onAdd(files: readonly File[]): void };
  commands?: { disabled: boolean; onStart(): void };
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const menuId = useId();

  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.stopPropagation();
      setOpen(false);
      trigger.current?.focus();
    };
    document.addEventListener('pointerdown', close);
    document.addEventListener('keydown', escape, true);
    return () => {
      document.removeEventListener('pointerdown', close);
      document.removeEventListener('keydown', escape, true);
    };
  }, [open]);

  if (!attachments && !commands) return null;

  return (
    <div ref={root} className="composer-add">
      <button
        ref={trigger}
        type="button"
        className="composer-icon composer-attach"
        aria-label="Add"
        title="Add"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => setOpen((value) => !value)}
      >
        <PlusIcon size={16} />
      </button>
      {open && (
        <div id={menuId} role="menu" className="composer-menu">
          {attachments && (
            <button
              type="button"
              role="menuitem"
              disabled={attachments.disabled}
              className="composer-menu-item"
              onClick={() => {
                setOpen(false);
                input.current?.click();
              }}
            >
              <PaperclipIcon size={14} />
              <span>
                <strong>Attach files</strong>
                <small>Images, PDFs and documents</small>
              </span>
            </button>
          )}
          {commands && (
            <button
              type="button"
              role="menuitem"
              disabled={commands.disabled}
              className="composer-menu-item"
              onClick={() => {
                setOpen(false);
                commands.onStart();
              }}
            >
              <TerminalIcon size={14} />
              <span>
                <strong>Commands</strong>
                <small>
                  {commands.disabled
                    ? 'Start from an empty message'
                    : 'Or type / in the message'}
                </small>
              </span>
            </button>
          )}
        </div>
      )}
      {attachments && (
        <input
          ref={input}
          type="file"
          aria-label="Choose attachments"
          multiple
          disabled={attachments.disabled}
          className="sr-only"
          tabIndex={-1}
          onChange={(event) => {
            const files = Array.from(event.currentTarget.files ?? []);
            event.currentTarget.value = '';
            if (!attachments.disabled && files.length) attachments.onAdd(files);
          }}
        />
      )}
    </div>
  );
}
