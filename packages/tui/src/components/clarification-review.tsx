import React, { useEffect, useRef, useState } from 'react';
import { Box, Text, useInput } from 'ink';
import type {
  ClarificationAnswer,
  ClarificationAnswerInput,
  ClarificationRequest,
} from '@animaos/sdk';
import {
  appendReviewText,
  removeReviewCharacter,
  reviewLiteral,
} from '../session-review-state.js';

export interface ClarificationReviewProps {
  request: ClarificationRequest;
  disabled: boolean;
  onAnswer(input: ClarificationAnswerInput): Promise<boolean>;
  onBack(): void;
}

export function ClarificationReview({
  request,
  disabled,
  onAnswer,
  onBack,
}: ClarificationReviewProps): React.ReactElement {
  const [answers, setAnswers] = useState<ClarificationAnswer[]>(
    request.questions.map((question) => ({
      questionId: question.id,
      choiceIds: [],
      text: null,
    })),
  );
  const [field, setField] = useState(0);
  const [option, setOption] = useState(0);
  const [confirming, setConfirming] = useState(false);
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);
  const fields = request.questions.flatMap((question, questionIndex) => [
    ...(question.choices.length
      ? [{ questionIndex, kind: 'choices' as const }]
      : []),
    { questionIndex, kind: 'text' as const },
  ]);
  const active = fields[field];
  const advance = (backwards = false) => {
    setField(
      (current) =>
        (current + (backwards ? fields.length : 1)) % (fields.length + 1),
    );
    setOption(0);
  };
  const submit = async () => {
    if (disabled || savingRef.current || request.status !== 'pending') return;
    if (!confirming) {
      setConfirming(true);
      return;
    }
    savingRef.current = true;
    setSaving(true);
    try {
      const normalized = answers.map((answer) => ({
        ...answer,
        text: answer.text?.trim() || null,
      }));
      if (
        (await onAnswer({ revision: request.revision, answers: normalized })) &&
        mountedRef.current
      )
        onBack();
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
      advance(key.shift);
      return;
    }
    if (key.return) {
      if (!active) void submit();
      else advance();
      return;
    }
    if (!active || key.ctrl || key.meta) return;
    const question = request.questions[active.questionIndex]!;
    if (active.kind === 'choices') {
      if (key.upArrow || key.downArrow) {
        setOption(
          (current) =>
            (current + (key.upArrow ? question.choices.length - 1 : 1)) %
            question.choices.length,
        );
        return;
      }
      if (text === ' ') {
        const choice = question.choices[option]!;
        setAnswers((current) =>
          current.map((answer, index) => {
            if (index !== active.questionIndex) return answer;
            const selected = answer.choiceIds.includes(choice.id);
            const choiceIds = question.multiSelect
              ? question.choices
                  .filter((item) =>
                    item.id === choice.id
                      ? !selected
                      : answer.choiceIds.includes(item.id),
                  )
                  .map((item) => item.id)
              : selected
                ? []
                : [choice.id];
            return {
              ...answer,
              choiceIds,
              text: question.multiSelect ? answer.text : null,
            };
          }),
        );
      }
    } else if (
      !key.upArrow &&
      !key.downArrow &&
      !key.leftArrow &&
      !key.rightArrow
    ) {
      setAnswers((current) =>
        current.map((answer, index) =>
          index !== active.questionIndex
            ? answer
            : {
                ...answer,
                choiceIds: question.multiSelect ? answer.choiceIds : [],
                text:
                  key.backspace || key.delete
                    ? removeReviewCharacter(answer.text ?? '')
                    : appendReviewText(answer.text ?? '', text, 4_096),
              },
        ),
      );
    }
  });

  return (
    <Box flexDirection="column" paddingX={1}>
      <Text bold color="cyan">
        {confirming ? 'Confirm clarification answers' : 'Clarification request'}
      </Text>
      <Text color="gray">
        Agent: {reviewLiteral(request.agentId)} · Session:{' '}
        {reviewLiteral(request.sessionId)}
      </Text>
      <Text color="gray">
        Run: {reviewLiteral(request.runId)} · Revision: {request.revision}
      </Text>
      {request.questions.map((question, questionIndex) => {
        const answer = answers[questionIndex]!;
        const labels = question.choices
          .filter((choice) => answer.choiceIds.includes(choice.id))
          .map((choice) => choice.label);
        const text = answer.text?.trim();
        const activeChoices =
          active?.questionIndex === questionIndex && active.kind === 'choices';
        const activeText =
          active?.questionIndex === questionIndex && active.kind === 'text';
        return (
          <Box key={question.id} flexDirection="column" marginTop={1}>
            <Text bold>
              {questionIndex + 1}. {reviewLiteral(question.question)}
              {question.multiSelect ? ' (choose multiple)' : ''}
            </Text>
            {confirming ? (
              <Text>
                {reviewLiteral(
                  [...labels, ...(text ? [text] : [])].join('; '),
                ) || 'Skipped'}
              </Text>
            ) : (
              <>
                {question.choices.map((choice, index) => (
                  <Text
                    key={choice.id}
                    color={
                      activeChoices && index === option ? 'cyan' : undefined
                    }
                  >
                    {activeChoices && index === option ? '›' : ' '} [
                    {answer.choiceIds.includes(choice.id) ? 'x' : ' '}]{' '}
                    {reviewLiteral(choice.label)}
                  </Text>
                ))}
                <Text color={activeText ? 'cyan' : undefined}>
                  {activeText ? '›' : ' '}{' '}
                  {question.choices.length ? 'Other' : 'Answer'}:{' '}
                  {reviewLiteral(answer.text ?? '') ||
                    '(optional; blank skips)'}
                  {activeText ? '▌' : ''}
                </Text>
              </>
            )}
          </Box>
        );
      })}
      <Box marginTop={1} flexDirection="column">
        {disabled ? (
          <Text color="yellow">
            Submission unavailable while offline, refreshing, or no longer
            pending.
          </Text>
        ) : null}
        {saving ? <Text color="yellow">Saving answers…</Text> : null}
        {confirming ? (
          <Text bold>Enter to confirm answers. Escape to edit.</Text>
        ) : (
          <>
            <Text color={!active ? 'cyan' : undefined}>
              {!active ? '› ' : '  '}Review answers
            </Text>
            <Text color="gray">
              tab next field · shift+tab previous · up/down options · space
              select · enter continue · esc back
            </Text>
          </>
        )}
      </Box>
    </Box>
  );
}
