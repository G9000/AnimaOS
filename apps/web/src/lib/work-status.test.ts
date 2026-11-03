import { describe, expect, it } from 'vitest';

import type { ToolStep } from './transcript';
import { workStatus } from './work-status';

function call(name: string, status: ToolStep['status']): ToolStep {
  return {
    stepId: 'run_1:1',
    toolCallId: name,
    name,
    argumentsPreview: '',
    status,
    durationMs: null,
    result: null,
    truncated: false,
    runId: 'run_1',
    helper: null,
  };
}

describe('workStatus', () => {
  it('says what the call in progress does, in plain words', () => {
    expect(workStatus([call('web_search', 'running')])).toBe('Looking that up…');
    expect(workStatus([call('memory_search', 'running')])).toBe(
      'Checking what I remember…',
    );
    expect(workStatus([call('memory_add', 'running')])).toBe(
      'Remembering that…',
    );
    expect(workStatus([call('edit_file', 'running')])).toBe(
      'Making the changes…',
    );
  });

  it('uses the latest running call and names a helper', () => {
    const helper = {
      ...call('spawn_helper', 'running'),
      helper: { label: 'Researcher', agentId: null },
    };
    expect(workStatus([call('read_file', 'running'), helper])).toBe(
      'Asking Researcher…',
    );
  });

  it('falls back to thinking between calls and a plain line for unknown tools', () => {
    expect(workStatus([])).toBe('Thinking…');
    expect(workStatus([call('web_search', 'success')])).toBe('Thinking…');
    expect(workStatus([call('mcp_0123456789abcdef', 'running')])).toBe(
      'Working on it…',
    );
  });
});
