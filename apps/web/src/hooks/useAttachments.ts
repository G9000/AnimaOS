import { useEffect, useRef, useState } from 'react';
import type { UploadedAttachment } from '@animaos/sdk';
import {
  ATTACHMENT_ERRORS,
  IMAGE_ATTACHMENT_MIMES,
  MAX_DRAFT_ATTACHMENTS,
  attachmentMetadata,
  classifyAttachmentFile,
  uploadAttachmentWithProgress,
  validAttachmentId,
  type AttachmentDraft,
} from '../lib/attachments';

export interface AttachmentOptions {
  chatKey: string | null;
  agentId: string | null;
  sessionId: string | null;
  ensureSession(): Promise<string>;
  enabled: boolean;
}
interface Upload {
  key: string;
  controller: AbortController;
  generation: number;
}

export function useAttachments({
  chatKey,
  agentId,
  sessionId,
  ensureSession,
  enabled,
}: AttachmentOptions) {
  const drafts = useRef(new Map<string, AttachmentDraft[]>());
  const errors = useRef(new Map<string, string>());
  const operations = useRef(new Map<string, Upload>());
  const mounted = useRef(true);
  const [, render] = useState(0);
  const boundary = useRef({
    chatKey,
    agentId,
    sessionId,
    enabled,
    generation: 0,
  });
  if (
    boundary.current.chatKey !== chatKey ||
    boundary.current.agentId !== agentId ||
    boundary.current.sessionId !== sessionId ||
    boundary.current.enabled !== enabled
  )
    boundary.current = {
      chatKey,
      agentId,
      sessionId,
      enabled,
      generation: boundary.current.generation + 1,
    };
  const generation = boundary.current.generation;
  const notify = () => {
    if (mounted.current) render((value) => value + 1);
  };
  const active = () =>
    mounted.current &&
    enabled &&
    boundary.current.enabled &&
    boundary.current.generation === generation;
  const revoke = (item: AttachmentDraft) => {
    if (item.previewUrl) URL.revokeObjectURL(item.previewUrl);
  };
  const cancelKey = (key: string) => {
    for (const [id, operation] of operations.current)
      if (operation.key === key) {
        operations.current.delete(id);
        operation.controller.abort();
      }
    const items = drafts.current.get(key) ?? [];
    for (const item of items) revoke(item);
    drafts.current.set(
      key,
      items.map(({ previewUrl: _url, ...item }) =>
        item.status === 'uploading'
          ? {
              ...item,
              status: 'error' as const,
              progress: null,
              error: ATTACHMENT_ERRORS.cancelled,
            }
          : item,
      ),
    );
  };
  useEffect(() => {
    mounted.current = true;
    notify();
    return () => {
      mounted.current = false;
      if (chatKey) cancelKey(chatKey);
    };
  }, [chatKey, agentId, sessionId, enabled]);
  useEffect(
    () => () => {
      for (const key of drafts.current.keys()) cancelKey(key);
    },
    [],
  );

  const patch = (
    id: string,
    operation: Upload,
    change: Partial<AttachmentDraft>,
  ) => {
    if (
      !mounted.current ||
      operations.current.get(id) !== operation ||
      boundary.current.generation !== operation.generation ||
      !boundary.current.enabled
    )
      return;
    drafts.current.set(
      operation.key,
      (drafts.current.get(operation.key) ?? []).map((item) =>
        item.localId === id ? { ...item, ...change } : item,
      ),
    );
    notify();
  };
  const add = (files: readonly File[]) => {
    if (!active() || !chatKey || !agentId || files.length === 0) return;
    const previous = drafts.current.get(chatKey) ?? [];
    const selected = files.slice(
      0,
      Math.max(0, MAX_DRAFT_ATTACHMENTS - previous.length),
    );
    if (selected.length < files.length)
      errors.current.set(chatKey, ATTACHMENT_ERRORS.limit);
    else errors.current.delete(chatKey);
    const additions = selected.map((file) => {
      const classification = classifyAttachmentFile(file);
      return {
        localId: crypto.randomUUID(),
        name: file.name,
        ...(classification.kind ? { kind: classification.kind } : {}),
        status: classification.error ? 'error' : 'uploading',
        progress: null,
        ...(classification.error ? { error: classification.error } : {}),
        ...(classification.kind === 'image' &&
        IMAGE_ATTACHMENT_MIMES.has(file.type)
          ? { previewUrl: URL.createObjectURL(file) }
          : {}),
      } as AttachmentDraft;
    });
    drafts.current.set(chatKey, [...previous, ...additions]);
    notify();
    const valid = additions.filter((item) => item.status === 'uploading');
    if (!valid.length) return;
    let target: Promise<string>;
    try {
      target = sessionId ? Promise.resolve(sessionId) : ensureSession();
    } catch {
      target = Promise.reject(new Error(ATTACHMENT_ERRORS.upload));
    }
    for (let index = 0; index < additions.length; index++) {
      const item = additions[index];
      if (item.status !== 'uploading') continue;
      const operation: Upload = {
        key: chatKey,
        controller: new AbortController(),
        generation,
      };
      operations.current.set(item.localId, operation);
      void target
        .then((id) => {
          if (
            !mounted.current ||
            operations.current.get(item.localId) !== operation ||
            boundary.current.generation !== generation ||
            operation.controller.signal.aborted
          )
            return null;
          return uploadAttachmentWithProgress(agentId, id, selected[index], {
            signal: operation.controller.signal,
            onProgress: (progress) =>
              patch(item.localId, operation, { progress }),
          });
        })
        .then(
          (attachment) => {
            if (attachment)
              patch(item.localId, operation, {
                status: 'ready',
                attachment,
                name: attachment.name,
                kind: attachment.kind,
                progress: 100,
              });
          },
          () =>
            patch(item.localId, operation, {
              status: 'error',
              progress: null,
              error: ATTACHMENT_ERRORS.upload,
            }),
        )
        .finally(() => {
          if (operations.current.get(item.localId) === operation)
            operations.current.delete(item.localId);
        });
    }
  };
  const remove = (localId: string) => {
    if (!chatKey || boundary.current.generation !== generation) return;
    const operation = operations.current.get(localId);
    operations.current.delete(localId);
    operation?.controller.abort();
    const items = drafts.current.get(chatKey) ?? [];
    const item = items.find((value) => value.localId === localId);
    if (item) revoke(item);
    drafts.current.set(
      chatKey,
      items.filter((value) => value.localId !== localId),
    );
    errors.current.delete(chatKey);
    notify();
  };
  const clear = () => {
    if (!chatKey || boundary.current.generation !== generation) return;
    cancelKey(chatKey);
    drafts.current.set(chatKey, []);
    errors.current.delete(chatKey);
    notify();
  };
  const restore = (
    attachments: readonly UploadedAttachment[],
    attachmentIds?: readonly string[],
  ) => {
    if (
      !chatKey ||
      !mounted.current ||
      boundary.current.generation !== generation
    )
      return false;
    const items = drafts.current.get(chatKey) ?? [];
    const ids = new Set(
      items.map((item) => item.attachment?.id ?? item.savedAttachmentId),
    );
    const metadata = attachments
      .map(attachmentMetadata)
      .filter((item): item is UploadedAttachment => item !== null);
    const recovered = (attachmentIds ?? metadata.map((item) => item.id))
      .filter((id) => {
        if (!validAttachmentId(id) || ids.has(id)) return false;
        ids.add(id);
        return true;
      })
      .map((id) => {
        const attachment = metadata.find((item) => item.id === id);
        return {
          localId: crypto.randomUUID(),
          name: attachment?.name ?? 'Saved attachment',
          status: 'ready' as const,
          progress: 100,
          ...(attachment
            ? { kind: attachment.kind, attachment }
            : { savedAttachmentId: id }),
        };
      });
    if (items.length + recovered.length > MAX_DRAFT_ATTACHMENTS) {
      errors.current.set(chatKey, ATTACHMENT_ERRORS.limit);
      notify();
      return false;
    }
    drafts.current.set(chatKey, [...items, ...recovered]);
    errors.current.delete(chatKey);
    notify();
    return true;
  };
  const items = chatKey ? (drafts.current.get(chatKey) ?? []) : [];
  return {
    items,
    readyAttachments: items.flatMap((item) =>
      item.status === 'ready' && item.attachment ? [item.attachment] : [],
    ),
    readyAttachmentIds: items.flatMap((item) =>
      item.status === 'ready' && (item.attachment?.id ?? item.savedAttachmentId)
        ? [item.attachment?.id ?? (item.savedAttachmentId as string)]
        : [],
    ),
    uploading: items.some((item) => item.status === 'uploading'),
    error: (chatKey ? errors.current.get(chatKey) : null) ?? null,
    add,
    remove,
    clear,
    restore,
  };
}
