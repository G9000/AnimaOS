/** Owner-authored fixtures. Enabling a suite authorizes bounded model calls. */
export interface SkillEvaluationSuite {
  enforced: boolean;
  agentId: string;
  fixtures: SkillEvaluationFixture[];
  minImprovement: number;
  trialsPerFixture: number;
  maxOutputTokens: number;
  trialTimeoutMs: number;
}
export interface SkillEvaluationFixture {
  id: string;
  heldout: boolean;
  required: boolean;
  prompt: string;
  oracle: { kind: 'exactText' | 'canonicalJson'; expected: string };
}
export interface SkillEvaluationSuiteRecord {
  workspace: string;
  revision: number;
  suite: SkillEvaluationSuite;
}
export interface SkillEvaluationIdentity {
  workspace: string;
  slug: string;
  suiteRevision: number;
  suiteDigest: string;
  baselineHash: string | null;
  candidateHash: string;
  draftId: string;
  draftDigest: string;
  modelConfigDigest: string;
  grader: string;
}
export interface SkillEvaluationTrial {
  fixtureId: string;
  repetition: number;
  variant: 'baseline' | 'candidate';
  response: string | null;
  error: string | null;
  /** Native core accounting; zero usage without usageReported is unknown. */
  usage: {
    prompt_tokens: number;
    completion_tokens: number;
    total_tokens: number;
    cached_prompt_tokens: number;
    reasoning_tokens: number;
  };
  usageReported: boolean;
  usageComplete: boolean;
  durationMs: number;
}
/** Response-only evidence. A passing report grants no approval or permissions. */
export interface SkillEvaluationReport {
  id: string;
  identity: SkillEvaluationIdentity;
  agentId: string;
  model: string;
  provider: string | null;
  status:
    'running' | 'passed' | 'failed' | 'inconclusive' | 'interrupted' | 'stale';
  reservedAtMs: number;
  finishedAtMs: number | null;
  retiredAtMs: number | null;
  trials: SkillEvaluationTrial[];
  error: string | null;
  responseOnly: boolean;
  usageComplete: boolean;
}
