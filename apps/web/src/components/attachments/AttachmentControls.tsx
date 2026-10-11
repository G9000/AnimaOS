import {
  useRef,
  useState,
  type ReactNode,
  type DragEvent,
  type ClipboardEvent,
} from 'react';
export function AttachmentDropZone({
  disabled,
  onAdd,
  children,
}: {
  disabled: boolean;
  onAdd?: (files: readonly File[]) => void;
  children: ReactNode;
}) {
  const [dragging, setDragging] = useState(false);
  return (
    <div
      className="studio-attachment-dropzone"
      data-dragging={dragging || undefined}
      onDragOver={(event) => {
        if (
          disabled ||
          !onAdd ||
          !Array.from(event.dataTransfer.types ?? []).includes('Files')
        )
          return;
        event.preventDefault();
        setDragging(true);
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null))
          setDragging(false);
      }}
      onDrop={(event) => {
        setDragging(false);
        if (onAdd) attachmentDrop(event, onAdd, disabled);
      }}
      onPaste={(event) => {
        if (onAdd) attachmentPaste(event, onAdd, disabled);
      }}
    >
      {children}
    </div>
  );
}

export interface AttachmentControlsProps {
  disabled: boolean;
  onAdd(files: readonly File[]): void;
}
export function attachmentDrop(
  event: DragEvent,
  onAdd: (files: readonly File[]) => void,
  disabled: boolean,
) {
  if (disabled || !event.dataTransfer.files.length) return;
  event.preventDefault();
  onAdd(Array.from(event.dataTransfer.files));
}
export function attachmentPaste(
  event: ClipboardEvent,
  onAdd: (files: readonly File[]) => void,
  disabled: boolean,
) {
  if (disabled) return;
  const files: File[] = [];
  const seen = new Set<File>();
  const add = (file: File | null) => {
    if (file && !seen.has(file)) {
      seen.add(file);
      files.push(file);
    }
  };
  for (const item of Array.from(event.clipboardData.items))
    if (item.kind === 'file') add(item.getAsFile());
  // items and files are alternative representations of the same clipboard.
  if (files.length === 0)
    for (const file of Array.from(event.clipboardData.files)) add(file);
  if (files.length === 0) return;
  if (!event.clipboardData.getData?.('text/plain')) event.preventDefault();
  onAdd(files);
}
export function AttachmentControls({
  disabled,
  onAdd,
}: AttachmentControlsProps) {
  const input = useRef<HTMLInputElement>(null);
  return (
    <span className="studio-attachment-picker">
      <button
        type="button"
        disabled={disabled}
        aria-label="Attach files"
        onClick={() => input.current?.click()}
      >
        📎
      </button>
      <input
        ref={input}
        type="file"
        aria-label="Choose attachments"
        multiple
        disabled={disabled}
        className="sr-only"
        tabIndex={-1}
        onChange={(event) => {
          const files = Array.from(event.currentTarget.files ?? []);
          event.currentTarget.value = '';
          if (!disabled && files.length) onAdd(files);
        }}
      />
    </span>
  );
}
