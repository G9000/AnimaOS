import { useId } from 'react';
import type { Session } from '@animaos/sdk';
import { usePublishHeaderSummary } from '../../lib/header-context';
import { KeptSummary } from './TranscriptNotes';
import './session-continuity.css';

/** When the chat starts to fill the agent's working memory, it says so. */
export const CONTEXT_NOTE_AT = 0.75;

type ContextUsage = NonNullable<Session['contextUsage']>;

function validUsage(usage: Session['contextUsage']): usage is ContextUsage {
  return (
    usage !== undefined &&
    Number.isFinite(usage.estimatedTokens) &&
    usage.estimatedTokens >= 0 &&
    Number.isFinite(usage.budgetTokens) &&
    usage.budgetTokens > 0 &&
    Number.isFinite(usage.replyReserveTokens) &&
    usage.replyReserveTokens >= 0
  );
}

/** Publishes the summary for the header; the main chat renders its own inline
 *  summary card, while other sessions may use the header or local fallback. */
export function SessionContinuity({ session }: { session: Session }) {
  const inHeader = usePublishHeaderSummary(
    session.summary ?? null,
    session.mainChat === true,
  );
  return session.summary && !inHeader && !session.mainChat ? (
    <div className="session-continuity">
      <KeptSummary summary={session.summary} />
    </div>
  ) : null;
}

/** A quiet note above the composer once the conversation nears the agent's
 *  working-memory budget. An estimate that is missing or small says nothing:
 *  it is not news. */
export function ContextNote({
  usage,
  agentName,
  mainChat = false,
}: {
  usage: Session['contextUsage'];
  agentName: string;
  mainChat?: boolean;
}) {
  const descriptionId = useId();
  if (!validUsage(usage)) return null;
  const ratio = usage.estimatedTokens / usage.budgetTokens;
  const showNote = ratio >= CONTEXT_NOTE_AT;
  const percent = Math.round(ratio * 100);
  const explanation = `Estimate of selected conversation history and saved summary only: ~${usage.estimatedTokens.toLocaleString()} of ${usage.budgetTokens.toLocaleString()} tokens. Excludes system instructions and provider context, tool schemas, files, the current draft, and the reply (${usage.replyReserveTokens.toLocaleString()} tokens reserved).`;
  return (
    <>
      <details
        className={
          mainChat
            ? 'main-chat-context-inspector sr-only'
            : 'mx-4 my-2 text-xs text-ink-3'
        }
      >
        <summary className="cursor-pointer">Inspect context estimates</summary>
        <p className="my-2">
          Preview estimates, not billed or actual provider usage. The
          conversation budget meter covers history and summary only.
        </p>
        <p>
          History and summary: ~{usage.estimatedTokens.toLocaleString()} /{' '}
          {usage.budgetTokens.toLocaleString()} tokens; reply reserve:{' '}
          {usage.replyReserveTokens.toLocaleString()}.
        </p>
        {usage.breakdown ? (
          <dl className="my-2 grid grid-cols-2 gap-x-4 gap-y-1">
            {(
              [
                ['Conversation', usage.breakdown.conversationTokens],
                ['Saved summary', usage.breakdown.summaryTokens],
                ['Checkpoint', usage.breakdown.checkpointTokens],
                [
                  'System instructions',
                  usage.breakdown.systemInstructionsTokens,
                ],
                ['Tool schemas', usage.breakdown.toolSchemasTokens],
                ['Companion files', usage.breakdown.companionFilesTokens],
                ['Memory', usage.breakdown.memoryTokens],
                ...(usage.breakdown.projectContextTokens !== undefined ? [['Shared project context', usage.breakdown.projectContextTokens] as const] : []),
              ] as const
            ).map(([label, tokens]) => (
              <div key={label} className="contents">
                <dt>{label}</dt>
                <dd>
                  {tokens === null
                    ? 'Unavailable'
                    : `~${tokens.toLocaleString()} tokens`}
                </dd>
              </div>
            ))}
          </dl>
        ) : (
          <p>Detailed prompt contributions are unavailable from this daemon.</p>
        )}
        {usage.coverage && (
          <>
            <p>Basis: {usage.coverage.basis}</p>
            <p>
              Omitted: {usage.coverage.omitted.join('; ') || 'None reported'}
            </p>
          </>
        )}
        {usage.projectContext && <div aria-label="Shared project context coverage">
          <p>Project sources: current preview · about {usage.projectContext.estimatedTokens.toLocaleString()} tokens.</p>
          <p>The next turn rechecks these sources and records its own admitted selection.</p>
          {usage.projectContext.resources.map((source, index) => <p key={source.resourceId}>
            Selected source {index + 1}: {source.state.replaceAll('_', ' ')}{source.shortened ? ' · shortened' : ''}
          </p>)}
        </div>}
      </details>
      {showNote && (
        <div className="session-context-note">
          <progress
            aria-label="Estimated conversation context"
            aria-describedby={descriptionId}
            aria-valuetext={`Approximately ${usage.estimatedTokens.toLocaleString()} of ${usage.budgetTokens.toLocaleString()} conversation tokens, ${percent} percent of the conversation budget`}
            title={explanation}
            max={usage.budgetTokens}
            value={Math.min(usage.estimatedTokens, usage.budgetTokens)}
          />
          <span>
            This chat is getting long · {percent}% of {agentName}’s working
            memory · older messages get summarized on their own
          </span>
          <span className="sr-only" id={descriptionId}>
            {explanation}
          </span>
        </div>
      )}
    </>
  );
}
