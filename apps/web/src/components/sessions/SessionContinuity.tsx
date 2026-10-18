import { useId } from 'react';
import type { Session } from '@animaOS-SWARM/sdk';
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

/** What the agent kept from earlier in the chat, at the top of the thread. */
export function SessionContinuity({ session }: { session: Session }) {
  return session.summary ? (
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
}: {
  usage: Session['contextUsage'];
  agentName: string;
}) {
  const descriptionId = useId();
  if (!validUsage(usage)) return null;
  const ratio = usage.estimatedTokens / usage.budgetTokens;
  if (ratio < CONTEXT_NOTE_AT) return null;
  const percent = Math.round(ratio * 100);
  const explanation = `Estimate of selected conversation history and saved summary only: ~${usage.estimatedTokens.toLocaleString()} of ${usage.budgetTokens.toLocaleString()} tokens. Excludes system instructions and provider context, tool schemas, files, the current draft, and the reply (${usage.replyReserveTokens.toLocaleString()} tokens reserved).`;
  return (
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
        This chat is getting long · {percent}% of {agentName}’s working memory
        · older messages get summarized on their own
      </span>
      <span className="sr-only" id={descriptionId}>
        {explanation}
      </span>
    </div>
  );
}
