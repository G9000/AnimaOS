import React, { useState } from 'react';
import { Box, Text, useInput } from 'ink';
import type { SessionReview as ReviewState } from '../hooks/use-session-review.js';
import { reviewLiteral } from '../session-review-state.js';
import { ApprovalReview } from './approval-review.js';
import { ClarificationReview } from './clarification-review.js';

export type ReviewFilter = 'all' | 'approvals' | 'clarifications';

export function SessionReview({
  review,
  filter,
  onBack,
  onStop,
}: {
  review: ReviewState;
  filter: ReviewFilter;
  onBack(): void;
  onStop?(): void;
}): React.ReactElement {
  const [detail, setDetail] = useState<{
    kind: 'approval' | 'clarification';
    id: string;
  } | null>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const rows = [
    ...(filter === 'clarifications'
      ? []
      : review.approvals.map((item) => ({
          key: `a:${item.id}`,
          kind: 'approval' as const,
          item,
          label: `${item.tool} (${item.class})`,
        }))),
    ...(filter === 'approvals'
      ? []
      : review.clarifications.map((item) => ({
          key: `c:${item.id}`,
          kind: 'clarification' as const,
          item,
          label: item.questions[0]?.question ?? 'Clarification',
        }))),
  ].sort(
    (a, b) =>
      a.item.createdAtMs - b.item.createdAtMs || a.key.localeCompare(b.key),
  );
  const cursor = Math.max(
    0,
    rows.findIndex((row) => row.key === focusedId),
  );
  const selectedApproval =
    detail?.kind === 'approval'
      ? review.approvals.find((item) => item.id === detail.id)
      : null;
  const selectedClarification =
    detail?.kind === 'clarification'
      ? review.clarifications.find((item) => item.id === detail.id)
      : null;

  useInput((input, key) => {
    if (key.ctrl && input.toLowerCase() === 'x') onStop?.();
  });
  useInput(
    (input, key) => {
      if (key.escape || input === 'q') {
        onBack();
        return;
      }
      if (input === 'r') {
        void review.refresh();
        return;
      }
      if (key.upArrow || key.downArrow) {
        if (rows.length)
          setFocusedId(
            rows[(cursor + (key.upArrow ? rows.length - 1 : 1)) % rows.length]!
              .key,
          );
      } else if (key.return && rows[cursor]) {
        setDetail({ kind: rows[cursor]!.kind, id: rows[cursor]!.item.id });
      }
    },
    { isActive: !detail },
  );
  useInput(
    (_input, key) => {
      if (key.escape || key.return) setDetail(null);
    },
    {
      isActive: Boolean(detail && !selectedApproval && !selectedClarification),
    },
  );

  return (
    <Box flexDirection="column" paddingX={1}>
      <Text bold color="cyan">
        Session review · {filter === 'all' ? 'all requests' : filter}
      </Text>
      {review.error ? (
        <Text color="red">{reviewLiteral(review.error)}</Text>
      ) : null}
      {review.notice ? <Text color="green">{review.notice}</Text> : null}
      {!review.canAct && review.available ? (
        <Text color="yellow">
          Review is read-only until the daemon is available and requests
          refresh.
        </Text>
      ) : null}
      {selectedApproval ? (
        <ApprovalReview
          key={`a:${selectedApproval.id}:${selectedApproval.revision}:${review.formVersions[`a:${selectedApproval.id}`] ?? 0}`}
          approval={selectedApproval}
          disabled={review.approvalBlocked(selectedApproval)}
          onDecide={(input) => review.decide(selectedApproval, input)}
          onBack={() => setDetail(null)}
        />
      ) : selectedClarification ? (
        <ClarificationReview
          key={`c:${selectedClarification.id}:${selectedClarification.revision}:${review.formVersions[`c:${selectedClarification.id}`] ?? 0}`}
          request={selectedClarification}
          disabled={review.clarificationBlocked(selectedClarification)}
          onAnswer={(input) => review.answer(selectedClarification, input)}
          onBack={() => setDetail(null)}
        />
      ) : detail ? (
        <>
          <Text color="yellow">This request is no longer pending.</Text>
          <Text color="gray">enter or esc return to queue</Text>
        </>
      ) : (
        <>
          {!review.available ? (
            <Text color="yellow">
              Session review is unavailable in this launch.
            </Text>
          ) : null}
          {review.loading ? (
            <Text color="gray">Loading review requests…</Text>
          ) : null}
          {!review.loading && rows.length === 0 ? (
            <Text color="gray">No pending requests in this session.</Text>
          ) : null}
          {rows.map((row, index) => (
            <Box key={row.key} flexDirection="column" marginTop={1}>
              <Text color={cursor === index ? 'cyan' : undefined}>
                {cursor === index ? '› ' : '  '}
                {row.kind === 'approval' ? 'Approval' : 'Clarification'}:{' '}
                {reviewLiteral(row.label)}
              </Text>
              <Text color="gray">
                {' '}
                Agent: {reviewLiteral(row.item.agentId)} · Run:{' '}
                {reviewLiteral(row.item.runId)}
              </Text>
            </Box>
          ))}
          <Text color="gray">
            up/down choose · enter open · r refresh · esc back to chat
            {onStop ? ' · ctrl+x stop run' : ''}
          </Text>
        </>
      )}
    </Box>
  );
}
