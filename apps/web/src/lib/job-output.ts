import type { AgentJob, AgentJobAttempt } from '@animaOS-SWARM/sdk';

function validOutput(output: NonNullable<AgentJob['finalOutput']>) {
  return (
    Number.isSafeInteger(output.generation) &&
    output.generation > 0 &&
    /^[a-f0-9]{64}$/.test(output.sha256)
  );
}

export function reviewMatchesAttempt(attempt: AgentJobAttempt) {
  const { review, finalOutput } = attempt;
  if (!review) return false;
  if (!finalOutput)
    return (
      review.finalOutputGeneration === undefined &&
      review.finalOutputSha256 === undefined
    );
  return (
    validOutput(finalOutput) &&
    review.finalOutputGeneration === finalOutput.generation &&
    review.finalOutputSha256 === finalOutput.sha256
  );
}

export function hasCurrentJobOutput(job: AgentJob) {
  if (job.status !== 'completed' || job.pendingDelegations) return false;
  const attempt = job.attempts.find((value) => value.attempt === job.attempt);
  if (!job.finalOutput) return !attempt?.finalOutput;
  return (
    !!attempt?.finalOutput &&
    validOutput(job.finalOutput) &&
    attempt.finalOutput.generation === job.finalOutput.generation &&
    attempt.finalOutput.sha256 === job.finalOutput.sha256 &&
    attempt.result === job.result
  );
}

export function currentJobReview(job: AgentJob) {
  if (!hasCurrentJobOutput(job)) return null;
  const attempt = job.attempts.find((value) => value.attempt === job.attempt);
  return attempt && reviewMatchesAttempt(attempt) ? attempt.review : null;
}

export function jobReviewKey(job: AgentJob) {
  return JSON.stringify([
    job.id,
    job.attempt,
    job.finalOutput?.generation,
    job.finalOutput?.sha256,
  ]);
}
