import { randomUUID } from 'node:crypto';

export type EventType =
  | 'agent:spawned'
  | 'agent:started'
  | 'agent:completed'
  | 'agent:failed'
  | 'agent:terminated'
  | 'agent:message'
  | 'task:started'
  | 'task:completed'
  | 'task:failed'
  | 'tool:before'
  | 'tool:after'
  | 'agent:tokens'
  | 'swarm:created'
  | 'swarm:message'
  | 'swarm:completed'
  | 'swarm:stopped'
  // Synthetic event the daemon emits when a SSE consumer falls behind the
  // broadcast buffer. Payload: `{ missed: number }`. Surfaced by the TUI as
  // a gap marker so users know the trace lost frames.
  | 'swarm:lagged';

export interface Event<T = unknown> {
  id: string;
  type: EventType;
  agentId?: string;
  timestampMs: number;
  data: T;
}

export type EventHandler<T = unknown> = (
  event: Event<T>,
) => void | Promise<void>;

export interface IEventBus {
  on<T = unknown>(type: EventType, handler: EventHandler<T>): () => void;
  emit<T = unknown>(type: EventType, data: T, agentId?: string): Promise<void>;
  clear(): void;
}

export class EventBus implements IEventBus {
  private listeners = new Map<EventType, Array<EventHandler<unknown>>>();

  on<T = unknown>(type: EventType, handler: EventHandler<T>): () => void {
    const list = this.listeners.get(type) ?? [];
    list.push(handler as EventHandler<unknown>);
    this.listeners.set(type, list);

    return () => {
      const current = this.listeners.get(type);
      if (current) {
        const idx = current.indexOf(handler as EventHandler<unknown>);
        if (idx >= 0) current.splice(idx, 1);
      }
    };
  }

  async emit<T = unknown>(
    type: EventType,
    data: T,
    agentId?: string,
  ): Promise<void> {
    const event: Event<T> = {
      id: randomUUID(),
      type,
      agentId,
      timestampMs: Date.now(),
      data,
    };

    const list = this.listeners.get(type);
    if (!list || list.length === 0) return;

    for (const handler of list) {
      try {
        await (handler as EventHandler<T>)(event);
      } catch (err) {
        console.error(`[event-bus] Error in handler for ${type}:`, err);
      }
    }
  }

  clear(): void {
    this.listeners.clear();
  }
}
