import { expect, test } from '@playwright/test';
import {
  createWorkspaceFilesPolicy,
  type WorkspaceFilesStep,
} from './fixtures/workspace-files-policy';

// These independent contract literals are not derived from policy output.
const origin = 'http://127.0.0.1:18753';
const steps = [
  { kind: 'list', path: '/api/workspace/files' },
  { kind: 'preview', path: '/api/workspace/file?path=reports%2Fledger.csv' },
  {
    kind: 'download',
    path: '/api/workspace/file/download?path=reports%2Fledger.csv',
  },
] as const;

test('m69_policy_admits_exact_list_preview_download_once_in_order', () => {
  const policy = createWorkspaceFilesPolicy();
  expect(policy.snapshot()).toEqual({ admitted: [], refused: 0 });
  for (const step of steps) {
    expect(policy.admit({ method: 'GET', url: origin + step.path })).toEqual({
      allowed: true,
      kind: step.kind,
      path: step.path,
    });
  }
  expect(policy.snapshot()).toEqual({
    admitted: ['list', 'preview', 'download'],
    refused: 0,
  });
  expect(
    policy.admit({ method: 'GET', url: origin + steps[2].path }).allowed,
  ).toBe(false);
  expect(policy.snapshot()).toEqual({
    admitted: ['list', 'preview', 'download'],
    refused: 1,
  });
});

const refusedRequests = [
  { label: 'POST', method: 'POST', url: origin + steps[0].path },
  { label: 'HEAD', method: 'HEAD', url: origin + steps[0].path },
  { label: 'OPTIONS', method: 'OPTIONS', url: origin + steps[0].path },
  { label: 'lowercase method', method: 'get', url: origin + steps[0].path },
  {
    label: 'public origin',
    method: 'GET',
    url: 'https://example.com/api/workspace/files',
  },
  {
    label: 'localhost alias',
    method: 'GET',
    url: 'http://localhost:18753/api/workspace/files',
  },
  {
    label: 'daemon port',
    method: 'GET',
    url: 'http://127.0.0.1:18752/api/workspace/files',
  },
  {
    label: 'alternate scheme',
    method: 'GET',
    url: 'https://127.0.0.1:18753/api/workspace/files',
  },
  {
    label: 'authority credentials',
    method: 'GET',
    url: 'http://owner@127.0.0.1:18753/api/workspace/files',
  },
  {
    label: 'lookalike origin',
    method: 'GET',
    url: 'http://127.0.0.1:18753.example.com/api/workspace/files',
  },
  { label: 'relative URL', method: 'GET', url: '/api/workspace/files' },
  { label: 'unknown path', method: 'GET', url: origin + '/api/agents' },
  {
    label: 'mutating workspace',
    method: 'PUT',
    url: origin + '/api/workspace',
  },
  {
    label: 'path suffix',
    method: 'GET',
    url: origin + '/api/workspace/files/extra',
  },
  {
    label: 'encoded path',
    method: 'GET',
    url: origin + '/api/workspace/%66iles',
  },
  {
    label: 'dot segment',
    method: 'GET',
    url: origin + '/api/x/../workspace/files',
  },
  {
    label: 'list query',
    method: 'GET',
    url: origin + '/api/workspace/files?path=reports%2Fledger.csv',
  },
  {
    label: 'empty list query',
    method: 'GET',
    url: origin + '/api/workspace/files?',
  },
  {
    label: 'fragment',
    method: 'GET',
    url: origin + '/api/workspace/files#local',
  },
  { label: 'invalid URL', method: 'GET', url: 'http://[' },
];

for (const request of refusedRequests) {
  test(`m69_policy_refuses ${request.label} without consuming an admission`, () => {
    const policy = createWorkspaceFilesPolicy();
    const decision = policy.admit(request);
    expect(decision.allowed).toBe(false);
    if (!decision.allowed) expect(decision.reason.length).toBeGreaterThan(0);
    expect(policy.snapshot()).toEqual({ admitted: [], refused: 1 });
  });
}

for (const stage of [1, 2] as const) {
  const label = steps[stage].kind;
  for (const query of [
    '',
    '?path=reports/ledger.csv',
    '?path=reports%252Fledger.csv',
    '?path=reports%2fledger.csv',
    '?path=reports%2Fother.csv',
    '?path=reports%2Fledger.csv&extra=1',
    '?extra=1&path=reports%2Fledger.csv',
    '?path=reports%2Fledger.csv&path=reports%2Fledger.csv',
    '?path=reports%2Fledger.csv&',
    '?path=reports%2Fledger.csv#fragment',
    '?path=reports%2F..%2Fledger.csv',
    '?path=%',
  ]) {
    test(`m69_policy_refuses ${label} query ${JSON.stringify(query)}`, () => {
      const policy = createWorkspaceFilesPolicy();
      const prior: WorkspaceFilesStep[] = [];
      for (const step of steps.slice(0, stage)) {
        expect(
          policy.admit({ method: 'GET', url: origin + step.path }).allowed,
        ).toBe(true);
        prior.push(step.kind);
      }
      const path = steps[stage].path.split('?')[0]!;
      expect(
        policy.admit({ method: 'GET', url: origin + path + query }).allowed,
      ).toBe(false);
      expect(policy.snapshot()).toEqual({ admitted: prior, refused: 1 });
    });
  }
}

test('m69_policy_rejects_out_of_order_and_duplicate_admissions_without_advancing', () => {
  const policy = createWorkspaceFilesPolicy();
  for (const step of [steps[1], steps[2]]) {
    expect(
      policy.admit({ method: 'GET', url: origin + step.path }).allowed,
    ).toBe(false);
  }
  expect(
    policy.admit({ method: 'GET', url: origin + steps[0].path }).allowed,
  ).toBe(true);
  expect(
    policy.admit({ method: 'GET', url: origin + steps[0].path }).allowed,
  ).toBe(false);
  expect(
    policy.admit({ method: 'GET', url: origin + steps[2].path }).allowed,
  ).toBe(false);
  expect(
    policy.admit({ method: 'GET', url: origin + steps[1].path }).allowed,
  ).toBe(true);
  expect(
    policy.admit({ method: 'GET', url: origin + steps[1].path }).allowed,
  ).toBe(false);
  expect(
    policy.admit({ method: 'GET', url: origin + steps[2].path }).allowed,
  ).toBe(true);
  for (const step of steps) {
    expect(
      policy.admit({ method: 'GET', url: origin + step.path }).allowed,
    ).toBe(false);
  }
  expect(policy.snapshot()).toEqual({
    admitted: ['list', 'preview', 'download'],
    refused: 8,
  });
});

test('m69_policy_snapshot_cannot_mutate_authority_or_share_state_between_instances', () => {
  const first = createWorkspaceFilesPolicy();
  const second = createWorkspaceFilesPolicy();
  expect(
    first.admit({ method: 'GET', url: origin + steps[0].path }).allowed,
  ).toBe(true);
  first.snapshot().admitted.push('preview', 'download');
  expect(first.snapshot().admitted).toEqual(['list']);
  expect(second.snapshot()).toEqual({ admitted: [], refused: 0 });
  expect(
    first.admit({ method: 'GET', url: origin + steps[1].path }).allowed,
  ).toBe(true);
  expect(
    second.admit({ method: 'GET', url: origin + steps[1].path }).allowed,
  ).toBe(false);
});
