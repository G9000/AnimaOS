import type { Traveler } from './traveler';
export type { Traveler } from './traveler';
export type Language = 'ja' | 'zh';
export type NpcId = 'tavi' | 'mira';
export type VillagePhase = 'day' | 'evening' | 'complete';
export type BreadPreference = 'honey' | 'sesame' | null;
export type Expression = 'warm' | 'thoughtful' | 'happy';

export interface Suggestion {
  id: string;
  text: string;
  translation: string;
}
export interface Phrase {
  text: string;
  translation: string;
  reading: string;
}
export interface Dialogue extends Phrase {
  expression: Expression;
  source: 'model' | 'reviewed';
  suggestions: Suggestion[];
}
export interface GameView {
  saveId: string;
  language: Language;
  questStatus: 'pending' | 'collected' | 'delivered';
  traveler: Traveler | null;
  npcId: NpcId;
  phase: VillagePhase;
  inventory: { id: string; name: string; quantity: number }[];
  coins: number;
  breadPreference: BreadPreference;
  visits: number;
  memories: string[];
  journal: Phrase[];
  dialogue: Dialogue;
  runtime: {
    mode: 'live' | 'reviewed';
    engine: 'anima-harness';
    provider: string;
    model: string;
  };
}
export interface TurnInput {
  requestId: string;
  message: string;
  choiceId?: string;
  recoveryOf?: string;
  npcId?: NpcId;
}
