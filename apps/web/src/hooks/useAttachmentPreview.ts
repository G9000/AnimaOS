import { useEffect, useRef, useState } from 'react';
import { daemon } from '../lib/daemon-api';
import { ATTACHMENT_ERRORS, IMAGE_ATTACHMENT_MIMES } from '../lib/attachments';

export function useAttachmentPreview(
  agentId: string,
  attachmentId: string | undefined,
  mime: string | undefined,
  enabled: boolean,
) {
  const [state, setState] = useState<{
    url: string | null;
    loading: boolean;
    error: string | null;
  }>({ url: null, loading: false, error: null });
  const [attempt, setAttempt] = useState(0);
  const boundary = useRef({
    agentId,
    attachmentId,
    mime,
    enabled,
    attempt,
    generation: 0,
  });
  if (
    boundary.current.agentId !== agentId ||
    boundary.current.attachmentId !== attachmentId ||
    boundary.current.mime !== mime ||
    boundary.current.enabled !== enabled ||
    boundary.current.attempt !== attempt
  )
    boundary.current = {
      agentId,
      attachmentId,
      mime,
      enabled,
      attempt,
      generation: boundary.current.generation + 1,
    };
  const generation = boundary.current.generation;
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
    if (!enabled || !attachmentId || !mime || !IMAGE_ATTACHMENT_MIMES.has(mime))
      setState({ url: null, loading: false, error: null });
    else {
      setState({ url: null, loading: true, error: null });
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
  }, [agentId, attachmentId, mime, enabled, attempt]);
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
