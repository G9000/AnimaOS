import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import { runTask } from './task.js';

describe('workspace command lifecycle', () => {
  it('preserves a failed command exit code and cleans only its child scratch', async () => {
    const reports = mkdtempSync(join(tmpdir(), 'anima-task-runner-spec-'));
    const report = join(reports, 'result.json');
    const parentTemp = process.env.TMPDIR;
    try {
      const code = await runTask(
        process.execPath,
        [
          '-e',
          `const fs = require('node:fs');
           const os = require('node:os');
           const path = require('node:path');
           fs.writeFileSync(path.join(os.tmpdir(), 'owned'), 'temporary');
           fs.writeFileSync(process.argv[1], JSON.stringify({
             scratch: os.tmpdir(), cwd: process.cwd()
           }));
           process.exit(17);`,
          report,
        ],
        { cwd: reports },
      );
      const result = JSON.parse(readFileSync(report, 'utf8'));
      expect(code).toBe(17);
      expect(result.cwd).toBe(reports);
      expect(result.scratch).not.toBe(parentTemp);
      expect(existsSync(result.scratch)).toBe(false);
      expect(existsSync(report)).toBe(true);
      expect(process.env.TMPDIR).toBe(parentTemp);
    } finally {
      rmSync(reports, { recursive: true, force: true });
    }
  });
});
