import {
  useCallback,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
import { useRenderScope } from '../../hooks/useRenderScope';
import { ChatExportScope } from '../sessions/ChatExportDownload';
import { ProjectKnowledgeScope } from '../projects/ProjectKnowledgeLink';
import { PdfPagePreview } from './PdfPagePreview';
import '../projects/projects.css';

type AttachmentPageTarget = {
  agentId: string;
  attachmentId: string;
  page: number;
};

export function isAttachmentPageLink(href: string | undefined): boolean {
  return Boolean(
    href &&
    /\/api\/agents\/[^?#]*\/attachments\/[^?#]*\/pages(?:\/|[?#]|$)/.test(href),
  );
}

export function attachmentPageTarget(
  href: string,
): AttachmentPageTarget | null {
  try {
    const url = new URL(href, window.location.origin);
    if (
      url.origin !== window.location.origin ||
      url.search ||
      url.hash ||
      url.username ||
      url.password
    )
      return null;
    const match =
      /^\/api\/agents\/([^/]+)\/attachments\/([^/]+)\/pages\/(\d+)$/.exec(
        url.pathname,
      );
    if (!match) return null;
    const agentId = decodeURIComponent(match[1]);
    const attachmentId = decodeURIComponent(match[2]);
    const page = Number(match[3]);
    if (
      !/^[A-Za-z0-9_:-]{1,200}$/.test(agentId) ||
      !/^[A-Za-z0-9_-]{1,128}$/.test(attachmentId) ||
      !Number.isInteger(page) ||
      page < 1 ||
      page > 100
    )
      return null;
    return { agentId, attachmentId, page };
  } catch {
    return null;
  }
}

export function AttachmentPageLink({
  href,
  children,
}: {
  href: string;
  children: ReactNode;
}) {
  const chat = useContext(ChatExportScope);
  const project = useContext(ProjectKnowledgeScope);
  const scope = useRenderScope(
    href,
    chat?.agentId,
    chat?.sessionId,
    project?.agentId,
    project?.projectId,
  );
  const target = attachmentPageTarget(href);
  if (
    !target ||
    (chat && chat.agentId !== target.agentId) ||
    (project && project.agentId !== target.agentId)
  ) {
    return <span>{children}</span>;
  }
  return (
    <PageLinkAction key={scope.generation} target={target}>
      {children}
    </PageLinkAction>
  );
}

function PageLinkAction({
  target,
  children,
}: {
  target: AttachmentPageTarget;
  children: ReactNode;
}) {
  const [opened, setOpened] = useState(false);
  const close = useCallback(() => setOpened(false), []);
  return (
    <span>
      <button
        type="button"
        className="text-accent underline underline-offset-2"
        onClick={() => setOpened(true)}
      >
        {children}
      </button>
      {opened && <PageEvidenceDialog target={target} onClose={close} />}
    </span>
  );
}

function PageEvidenceDialog({
  target,
  onClose,
}: {
  target: AttachmentPageTarget;
  onClose: () => void;
}) {
  const titleId = useId();
  const close = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const previous = document.activeElement;
    close.current?.focus();
    return () => {
      if (previous instanceof HTMLElement && previous.isConnected)
        previous.focus();
    };
  }, []);
  return createPortal(
    <div
      className="project-citation-backdrop"
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        className="project-citation-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault();
            onClose();
          }
          if (event.key === 'Tab') {
            const controls = Array.from(
              event.currentTarget.querySelectorAll<HTMLElement>(
                'button, [tabindex="0"]',
              ),
            );
            const first = controls[0];
            const last = controls.at(-1);
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
        <div className="project-row">
          <h2 id={titleId}>Original PDF page {target.page}</h2>
          <button ref={close} type="button" onClick={onClose}>
            Close page evidence
          </button>
        </div>
        <PdfPagePreview
          source={{ kind: 'attachment_page', ...target }}
          openOnMount
        />
      </section>
    </div>,
    document.body,
  );
}
