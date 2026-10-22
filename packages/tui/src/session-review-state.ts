import type { Approval, ApprovalDecisionInput } from '@animaos/sdk';

/** Render model-authored text without granting it terminal control. */
export function reviewLiteral(value: string): string {
  return value.replace(
    /[\u0000-\u0009\u000b-\u001f\u007f-\u009f\u00ad\u061c\u200b-\u200f\u202a-\u202e\u2060-\u206f\ufeff]/g,
    (character) =>
      `\\u{${character.charCodeAt(0).toString(16).padStart(4, '0')}}`,
  );
}

export function mergeReviewRecords<
  T extends { id: string; revision: number; status: string },
>(current: T[], incoming: T[]): T[] {
  const records = new Map(current.map((record) => [record.id, record]));
  for (const record of incoming) {
    const previous = records.get(record.id);
    if (
      !previous ||
      record.revision > previous.revision ||
      (record.revision === previous.revision && previous.status === 'pending')
    ) {
      records.set(record.id, record);
    }
  }
  return [...records.values()];
}

export function approvalInputError(
  approval: Approval,
  input: ApprovalDecisionInput,
  now = Date.now(),
): string | null {
  if (approval.status !== 'pending')
    return 'This request is no longer pending.';
  if (approval.expiresAtMs <= now)
    return 'This request has expired. Refresh the queue.';
  if (input.revision !== approval.revision)
    return 'This request changed. Review the new revision.';
  if (Array.from(input.note ?? '').length > 1_000)
    return 'Notes may contain at most 1,000 characters.';
  if (input.decision === 'allow_session' || input.decision === 'allow_always') {
    if (approval.argumentsTruncated)
      return 'Scoped grants are unavailable because arguments were truncated.';
    const matcher = input.matcher ?? approval.suggestedMatcher;
    if (!approval.matcherKinds.includes(matcher.kind))
      return 'This matcher kind is not supported by the tool.';
    if (matcher.kind !== 'any' && !matcher.value.trim())
      return 'The matcher value cannot be empty.';
    if (Array.from(matcher.value).length > 512)
      return 'Matcher values may contain at most 512 characters.';
  }
  return null;
}

export function appendReviewText(
  current: string,
  input: string,
  limit: number,
): string {
  return Array.from(current + input)
    .slice(0, limit)
    .join('');
}

export function removeReviewCharacter(current: string): string {
  return Array.from(current).slice(0, -1).join('');
}
