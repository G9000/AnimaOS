import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { formAgency, loadWorkspace } from './loader.js';

function withWorkspace(file: unknown, check: (dir: string) => void) {
  const dir = mkdtempSync(join(tmpdir(), 'anima-loader-'));
  try {
    writeFileSync(join(dir, 'anima.yaml'), JSON.stringify(file));
    check(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

it('loads peer agents with per-agent providers and workspace defaults', () => {
  withWorkspace(
    {
      name: 'Team', provider: 'openai', model: 'default-model',
      agents: [
        { name: 'Ada', system: 'Research', provider: 'anthropic', model: 'a-model' },
        { name: 'Bo', system: 'Write' },
      ],
    },
    (dir) => {
      const workspace = loadWorkspace(dir);
      expect(workspace.agents[0]).toMatchObject({ provider: 'anthropic', model: 'a-model' });
      expect(workspace.agents[1].provider).toBeUndefined();
      expect(workspace).toMatchObject({ provider: 'openai', model: 'default-model' });
    }
  );
});

it('refuses a file without agents, including an old orchestrator file', () => {
  withWorkspace(
    { name: 'Old', orchestrator: { name: 'Boss', bio: 'b', system: 's' } },
    (dir) => expect(() => loadWorkspace(dir)).toThrow(/at least one agent/)
  );
});

it('forms a temporary agency with the chosen lead for one launch', () => {
  withWorkspace(
    { name: 'Team', agents: [{ name: 'Ada', system: 'a' }, { name: 'Bo', system: 'b' }] },
    (dir) => {
      const workspace = loadWorkspace(dir);
      const byDefault = formAgency(workspace);
      expect(byDefault.lead.name).toBe('Ada');
      expect(byDefault.members.map((agent) => agent.name)).toEqual(['Bo']);
      const ledByBo = formAgency(workspace, { lead: 'bo', strategy: 'round-robin' });
      expect(ledByBo).toMatchObject({ strategy: 'round-robin', lead: { name: 'Bo' } });
      expect(ledByBo.members.map((agent) => agent.name)).toEqual(['Ada']);
      expect(() => formAgency(workspace, { lead: 'Cy' })).toThrow(/No agent named/);
      expect(() => formAgency(workspace, { strategy: 'boss' })).toThrow(/Unknown strategy/);
    }
  );
});
