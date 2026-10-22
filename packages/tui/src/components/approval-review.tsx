import React, { useEffect, useRef, useState } from 'react';
import { Box, Text, useInput } from 'ink';
import type {
  Approval,
  ApprovalDecision,
  ApprovalDecisionInput,
  ApprovalMatcher,
} from '@animaos/sdk';
import {
  appendReviewText,
  approvalInputError,
  removeReviewCharacter,
  reviewLiteral,
} from '../session-review-state.js';

export interface ApprovalReviewProps {
  approval: Approval;
  disabled: boolean;
  onDecide(input: ApprovalDecisionInput): Promise<boolean>;
  onBack(): void;
}

const DECISIONS: ApprovalDecision[] = [
  'allow_once',
  'allow_session',
  'allow_always',
  'deny',
];
const LABELS: Record<ApprovalDecision, string> = {
  allow_once: 'Allow once',
  allow_session: 'Allow for session',
  allow_always: 'Always allow',
  deny: 'Deny',
};

export function ApprovalReview({
  approval,
  disabled,
  onDecide,
  onBack,
}: ApprovalReviewProps): React.ReactElement {
  const [decision, setDecision] = useState<ApprovalDecision>('allow_once');
  const [matcher, setMatcher] = useState<ApprovalMatcher>(
    approval.suggestedMatcher,
  );
  const [note, setNote] = useState('');
  const [field, setField] = useState(0);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);
  const scoped = decision === 'allow_session' || decision === 'allow_always';
  const input: ApprovalDecisionInput = {
    decision,
    revision: approval.revision,
    ...(note.trim() ? { note: note.trim() } : {}),
    ...(scoped
      ? {
          matcher: {
            kind: matcher.kind,
            value: matcher.kind === 'any' ? '' : matcher.value.trim(),
          },
        }
      : {}),
  };
  const invalid = approvalInputError(approval, input);
  const submit = async () => {
    if (disabled || savingRef.current) return;
    const validation = approvalInputError(approval, input);
    if (validation) {
      setError(validation);
      return;
    }
    if (!confirming) {
      setError(null);
      setConfirming(true);
      return;
    }
    savingRef.current = true;
    setSaving(true);
    try {
      if ((await onDecide(input)) && mountedRef.current) onBack();
    } finally {
      savingRef.current = false;
      if (mountedRef.current) setSaving(false);
    }
  };

  useInput((text, key) => {
    if (key.escape) {
      if (confirming) setConfirming(false);
      else onBack();
      return;
    }
    if (savingRef.current) return;
    if (confirming) {
      if (key.return) void submit();
      return;
    }
    if (key.tab) {
      setField((current) => (current + (key.shift ? 4 : 1)) % 5);
      return;
    }
    if (key.return) {
      if (field === 4) void submit();
      else setField((current) => current + 1);
      return;
    }
    if (key.leftArrow || key.rightArrow || key.upArrow || key.downArrow) {
      const delta = key.leftArrow || key.upArrow ? -1 : 1;
      if (field === 0)
        setDecision(
          (current) =>
            DECISIONS[
              (DECISIONS.indexOf(current) + delta + DECISIONS.length) %
                DECISIONS.length
            ]!,
        );
      if (field === 1 && scoped && approval.matcherKinds.length) {
        setMatcher((current) => ({
          ...current,
          kind: approval.matcherKinds[
            (approval.matcherKinds.indexOf(current.kind) +
              delta +
              approval.matcherKinds.length) %
              approval.matcherKinds.length
          ]!,
        }));
      }
      return;
    }
    if (key.ctrl || key.meta) return;
    if (field === 2 && scoped && matcher.kind !== 'any') {
      setMatcher((current) => ({
        ...current,
        value:
          key.backspace || key.delete
            ? removeReviewCharacter(current.value)
            : appendReviewText(current.value, text, 512),
      }));
    } else if (field === 3) {
      setNote((current) =>
        key.backspace || key.delete
          ? removeReviewCharacter(current)
          : appendReviewText(current, text, 1_000),
      );
    }
  });

  return (
    <Box flexDirection="column" paddingX={1}>
      <Text bold color="yellow">
        {confirming ? 'Confirm approval decision' : 'Approval request'}
      </Text>
      <Text>
        Tool: {reviewLiteral(approval.tool)} · Risk: {approval.class}
      </Text>
      <Text color="gray">
        Agent: {reviewLiteral(approval.agentId)} · Session:{' '}
        {reviewLiteral(approval.sessionId)}
      </Text>
      <Text color="gray">
        Run: {reviewLiteral(approval.runId)} · Revision: {approval.revision} ·
        Expires: {new Date(approval.expiresAtMs).toLocaleString()}
      </Text>
      {approval.reason || approval.reasonDetail ? (
        <Text color="yellow">
          Reason:{' '}
          {reviewLiteral(approval.reasonDetail ?? approval.reason ?? '')}
        </Text>
      ) : null}
      <Text bold>
        Arguments{approval.argumentsTruncated ? ' (truncated)' : ''}:
      </Text>
      <Text wrap="wrap">{reviewLiteral(approval.arguments)}</Text>
      <Box marginTop={1} flexDirection="column">
        <Text color={field === 0 ? 'cyan' : undefined}>
          {confirming ? '' : field === 0 ? '› ' : '  '}Decision:{' '}
          {LABELS[decision]}
        </Text>
        <Text color={field === 1 ? 'cyan' : undefined}>
          {confirming ? '' : field === 1 ? '› ' : '  '}Matcher kind:{' '}
          {scoped ? matcher.kind : 'not used for this decision'}
        </Text>
        {scoped ? (
          <Text color={field === 2 ? 'cyan' : undefined}>
            {confirming ? '' : field === 2 ? '› ' : '  '}Scope:{' '}
            {matcher.kind === 'any'
              ? 'ALL calls to this tool'
              : reviewLiteral(matcher.value) || '(empty)'}
          </Text>
        ) : (
          <Text color="gray"> Scope: this call only</Text>
        )}
        <Text color={field === 3 ? 'cyan' : undefined}>
          {confirming ? '' : field === 3 ? '› ' : '  '}Note:{' '}
          {reviewLiteral(note) || '(none)'}
        </Text>
        {scoped ? (
          <Text color="yellow">
            {decision === 'allow_always'
              ? 'Creates a persistent rule for this agent.'
              : 'Allows matching calls in this session.'}{' '}
            Broad matchers can authorize more than this command.
          </Text>
        ) : null}
        {error || invalid ? <Text color="red">{error ?? invalid}</Text> : null}
        {disabled ? (
          <Text color="yellow">
            Submission unavailable while offline, refreshing, or no longer
            pending.
          </Text>
        ) : null}
        {saving ? <Text color="yellow">Saving decision…</Text> : null}
        {!confirming ? (
          <Text color={field === 4 ? 'cyan' : undefined}>
            {field === 4 ? '› ' : '  '}Review decision
          </Text>
        ) : (
          <Text bold>Enter to confirm {LABELS[decision]}. Escape to edit.</Text>
        )}
        {!confirming ? (
          <Text color="gray">
            tab next field · shift+tab previous · arrows change decision/kind ·
            enter continue · esc back
          </Text>
        ) : null}
      </Box>
    </Box>
  );
}
