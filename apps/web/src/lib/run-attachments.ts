import type { Run, UploadedAttachment } from '@animaos/sdk';
import { attachmentMetadata } from './attachments';

/** Session-scoped public metadata retained while runs precede history. No bytes. */
export type KnownRunAttachments = Readonly<
  Record<string, readonly UploadedAttachment[]>
>;
export const EMPTY_RUN_ATTACHMENTS: KnownRunAttachments = {};
export function rememberRunAttachments(
  current: KnownRunAttachments,
  agentId: string,
  sessionId: string,
  key: string,
  attachments: readonly UploadedAttachment[],
): KnownRunAttachments {
  const metadata = attachments
    .map(attachmentMetadata)
    .filter((item): item is UploadedAttachment => item !== null);
  if (!metadata.length) return current;
  const next = {
    ...current,
    [`${agentId}\u0000${sessionId}\u0000${key}`]: metadata,
  };
  // Older entries normally are already in history; unknown references remain
  // visible when a page has queued messages across more than 200 identities.
  return Object.fromEntries(Object.entries(next).slice(-200));
}
export function attachmentsForRun(
  current: KnownRunAttachments,
  run: Pick<Run, 'agentId' | 'sessionId' | 'id' | 'idempotencyKey' | 'input'>,
): readonly UploadedAttachment[] {
  const prefix = `${run.agentId}\u0000${run.sessionId}\u0000`;
  const metadata =
    current[`${prefix}run:${run.id}`] ??
    (run.idempotencyKey
      ? current[`${prefix}request:${run.idempotencyKey}`]
      : undefined) ??
    [];
  return run.input.attachmentIds.flatMap((id) => {
    const attachment = metadata.find((item) => item.id === id);
    return attachment ? [attachment] : [];
  });
}
