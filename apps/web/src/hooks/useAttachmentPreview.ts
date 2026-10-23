import { useEffect, useRef, useState } from 'react';
import { daemon } from '../lib/daemon-api';
import { ATTACHMENT_ERRORS, IMAGE_ATTACHMENT_MIMES } from '../lib/attachments';
import { useLatest } from './useLatest';
import { useRenderScope } from './useRenderScope';

export function useAttachmentPreview(
  agentId: string,
  attachmentId: string | undefined,
  mime: string | undefined,
  enabled: boolean,
) {
  const shouldLoad = Boolean(
    enabled && attachmentId && mime && IMAGE_ATTACHMENT_MIMES.has(mime),
  );
  const [state, setState] = useState<{
    url: string | null;
    loading: boolean;
    error: string | null;
  }>({ url: null, loading: shouldLoad, error: null });
  const [attempt, setAttempt] = useState(0);
  const scope = useRenderScope(agentId, attachmentId, mime, enabled, attempt);
  const { generation } = scope;
  const boundary = useLatest(scope);
  const [previousScope, setPreviousScope] = useState(scope);
  if (previousScope !== scope) {
    setPreviousScope(scope);
    setState({ url: null, loading: shouldLoad, error: null });
  }
  const owner = useRef<{
    url: string | null;
    controller: AbortController;
    active: boolean;
  } | null>(null);
  useEffect(() => {
    const current = {
      url: null as string | null,
      controller: new AbortController(),
      active: true,
    };
    owner.current = current;
    const active = () =>
      current.active && boundary.current.generation === generation;
    if (enabled && attachmentId && mime && IMAGE_ATTACHMENT_MIMES.has(mime)) {
      void daemon
        .downloadAttachment(agentId, attachmentId, {
          signal: current.controller.signal,
        })
        .then((blob) => {
          if (!active()) return;
          current.url = URL.createObjectURL(blob);
          setState({ url: current.url, loading: false, error: null });
        })
        .catch(() => {
          if (active())
            setState({
              url: null,
              loading: false,
              error: ATTACHMENT_ERRORS.preview,
            });
        });
    }
    return () => {
      current.active = false;
      current.controller.abort();
      if (current.url) {
        URL.revokeObjectURL(current.url);
        current.url = null;
      }
    };
  }, [agentId, attachmentId, mime, enabled, attempt, generation, boundary]);
  const imageFailed = () => {
    const current = owner.current;
    if (!current?.active || boundary.current.generation !== generation) return;
    if (current.url) {
      URL.revokeObjectURL(current.url);
      current.url = null;
    }
    setState({ url: null, loading: false, error: ATTACHMENT_ERRORS.preview });
  };
  return {
    ...state,
    retry: () => setAttempt((value) => value + 1),
    imageFailed,
  };
}
