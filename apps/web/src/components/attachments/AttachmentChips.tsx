import type { AttachmentDraft } from '../../lib/attachments';
import { revealInvisible } from '../../lib/skills';
export function AttachmentChips({
  items,
  onRemove,
}: {
  items: readonly AttachmentDraft[];
  onRemove(localId: string): void;
}) {
  if (!items.length) return null;
  return (
    <ul className="studio-attachment-chips" aria-label="Message attachments">
      {items.map((item) => {
        const name = revealInvisible(item.name).text;
        return (
          <li key={item.localId} className="studio-attachment-chip">
            {item.previewUrl && (
              <img
                src={item.previewUrl}
                alt=""
                className="studio-draft-thumbnail"
              />
            )}
            <span className="studio-attachment-details">
              <strong>{name}</strong>
              <small>
                {item.kind ?? 'file'}
                {item.attachment
                  ? ` · ${formatAttachmentSize(item.attachment.size)}`
                  : ''}
              </small>
              {item.status === 'uploading' && (
                <span
                  role="progressbar"
                  aria-label={`Uploading ${name}`}
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={item.progress ?? undefined}
                >
                  {item.progress === null
                    ? 'Uploading…'
                    : `${Math.round(item.progress)}%`}
                </span>
              )}
              {item.error && (
                <span role="alert">{revealInvisible(item.error).text}</span>
              )}
            </span>
            <button
              type="button"
              aria-label={`Remove ${name}`}
              onClick={() => onRemove(item.localId)}
            >
              ×
            </button>
          </li>
        );
      })}
    </ul>
  );
}
export function formatAttachmentSize(bytes: number): string {
  return bytes < 1024
    ? `${bytes} B`
    : bytes < 1024 * 1024
      ? `${(bytes / 1024).toFixed(1)} KiB`
      : `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}
