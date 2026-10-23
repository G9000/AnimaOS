import { Text } from 'ink';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { EventBus } from '../event-bus.js';
import { cleanupInk, flushInk, renderInk } from '../test-harness.js';
import { useEventLog } from './use-event-log.js';

afterEach(() => {
  cleanupInk();
  vi.restoreAllMocks();
});

describe('useEventLog elapsed time', () => {
  it('records the final elapsed second even when completion arrives between ticks', async () => {
    const now = vi.spyOn(Date, 'now').mockReturnValue(1_000);
    const eventBus = new EventBus();
    function Probe() {
      const { stats, done } = useEventLog({
        eventBus,
        strategy: 'test',
        isRunning: false,
      });
      return (
        <Text>
          elapsed={stats.elapsed}|done={String(done)}
        </Text>
      );
    }
    const rendered = renderInk(<Probe />);
    await flushInk();
    now.mockReturnValue(3_200);
    await eventBus.emit('swarm:completed', { result: { status: 'success' } });
    await flushInk();
    expect(rendered.lastFrame()).toContain('elapsed=2|done=true');
  });
});
