import { useResetOnChange } from '../../hooks/useResetOnChange';
import { useState } from 'react';
import type { ChatModelPreferencesView, Session } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { ChatToolsPanel } from './ChatToolsPanel';
import { ChatModelControls } from './ChatModelControls';
import { ChatReviewPanel } from './ChatReviewPanel';
import { ShareConversationDialog } from './ShareConversationDialog';

export type ChatPanelName = 'model' | 'tools' | 'review' | 'reports' | 'share';
export interface ChatPanelRequest {
  scope: string;
  panel: ChatPanelName;
  id: string;
  focus?: string;
}
export const chatPanelScope = (session: Session) =>
  `${session.agentId}\0${session.id}\0${session.clearGeneration ?? 0}`;

export function ChatExtensions({
  session,
  request,
  onOpen,
  onModelSaved,
  refreshKey,
}: {
  session: Session;
  request?: ChatPanelRequest | null;
  onOpen(panel: ChatPanelName): void;
  onModelSaved?(view: ChatModelPreferencesView): void;
  refreshKey?: string | number;
}) {
  const [panel, setPanel] = useState<ChatPanelName | null>(null);
  const scope = chatPanelScope(session);
  useResetOnChange([request, scope], () => {
    setPanel(request?.scope === scope ? request.panel : null);
  });
  const button = (name: ChatPanelName, label: string) => (
    <button
      type="button"
      className="rounded px-2 py-1 hover:bg-white/5"
      aria-expanded={panel === name}
      onClick={() => (panel === name ? setPanel(null) : onOpen(name))}
    >
      {label}
    </button>
  );
  return (
    <div
      className="main-chat-extensions shrink-0 border-b border-white/10 text-xs"
      data-main-chat={session.mainChat ? 'true' : undefined}
      data-panel={panel ?? ''}
    >
      <nav
        className="flex flex-wrap gap-1 px-3 py-1"
        aria-label="Chat controls"
      >
        {button('model', 'Model & thinking')}
        {button('tools', 'Tools')}
        {button('review', 'Second opinion')}
        {button('reports', 'Reports')}
        {button('share', 'Share')}
      </nav>
      {panel && panel !== 'share' && (
        <div
          className="max-h-80 overflow-y-auto p-3"
          key={`${scope}:${request?.id ?? panel}`}
        >
          {panel === 'model' && (
            <ChatModelControls
              agentId={session.agentId}
              sessionId={session.id}
              clearGeneration={session.clearGeneration ?? 0}
              api={daemon.chatModels}
              onSaved={onModelSaved}
            />
          )}
          {panel === 'tools' && (
            <ChatToolsPanel
              agentId={session.agentId}
              sessionId={session.id}
              refreshKey={refreshKey}
            />
          )}
          {panel === 'review' && (
            <ChatReviewPanel
              agentId={session.agentId}
              sessionId={session.id}
              clearGeneration={session.clearGeneration ?? 0}
              api={daemon.chatModels}
              focus={request?.focus}
            />
          )}
          {panel === 'reports' && (
            <p>
              Saved interactive results appear in the conversation. Use Pin
              report on a result to keep it beside this chat. Ask Anima to
              update the saved report by its title.
            </p>
          )}
        </div>
      )}
      {panel === 'share' && (
        <ShareConversationDialog
          agentId={session.agentId}
          sessionId={session.id}
          clearGeneration={session.clearGeneration ?? 0}
          title={session.title}
          onClose={() => setPanel(null)}
        />
      )}
    </div>
  );
}
