---
title: Cozy Village Language RPG
author: Unassigned
last_edited: 2026-10-25
version: 1
status: draft
scratchboard: null
---

# Cozy Village Language RPG v1

| Field | Value |
| --- | --- |
| Author | To be confirmed |
| Version | 1 |
| Status | Draft for product review |
| Scratchboard | Not scheduled; product draft only |
| Created | 2026-10-25 |
| Last edited | 2026-10-25 |

> Move into a cozy village, make friends, and practice a language through conversations that remember you.

## Context

Build a small language-learning RPG powered by Anima's reusable Rust agent engine. Players explore a village, help its residents, and develop relationships through conversations generated during play. The first commercial test is a short playable story and a paid pilot.

The product hypothesis is that people will return to practice because they care about the characters and want to discover what happens next. Fresh dialogue supports this experience; the game also needs clear objectives, memorable characters, and consequences players can understand. Demand, retention, learning value, and willingness to pay remain hypotheses to test.

### First players and languages

The initial audience is adults who enjoy cozy games and can already use greetings and short phrases in the language they are learning, but struggle to hold a conversation. Sessions should fit into 15 to 20 minutes. The initial release practices reading and written conversation; it does not assess speaking or pronunciation.

Optimize the same village for **Japanese and Mandarin Chinese** as the first target languages. These are the selected product priorities; demand for this particular game still needs customer validation.

Use English for menus, quest instructions, translations, and explanations in the first pilot. This creates two supported learning routes, both using one world and story. A save belongs to one target language; another target language starts a separate save.

The proposed Mandarin presentation uses Simplified Chinese with optional pinyin. Japanese uses normal Japanese writing with optional kana readings. Readings and word meanings can be revealed when needed. Check their accuracy alongside the generated sentences. Both routes must support normal Japanese and Chinese text entry and legible character fonts.

An LLM makes wider language support possible, but each supported route still requires checks for natural phrasing, difficulty, explanations, and valid alternative answers. Availability in a model is not a release quality check.

## What This Version Delivers

The first version is a single-player **2.5D RPG played directly in a desktop browser**. Players control a character, explore the village, approach residents, and complete quests. The proposed presentation combines a compact 3D village with animated 2D pixel-art character sprites and a fixed angled overhead camera. Conversations use large illustrated portraits and framed dialogue, following the supplied visual references.

| Element | First version |
| --- | --- |
| Presentation | Detailed pixel-art appearance with 3D depth, controllable 2D sprites, an angled overhead exploration camera, and illustrated portrait dialogue |
| World | One village with a square and shop, an inn, and a garden |
| Characters | Four recurring NPCs with distinct voices, memories, and motives |
| Story | Three short in-game days preparing a shared village supper |
| Activities | Explore, ask questions, deliver items, make choices, and help residents |
| Conversation | Typed, generated responses grounded in the current scene |
| Learning | Adjustable hints, optional corrections, translations, and a phrase journal |
| Progress | Quest completion, relationship changes, inventory, and a few coins |
| Persistence | Resume the same character, story, and learning progress after returning |
| First offer | A free opening chapter and a paid pilot continuing the same story |

The shop is a meeting place and quest setting. Players may help with an order or obtain an item. Owning a business, setting production schedules, hiring workers, and balancing an economy are outside this version.

### Visual direction from the supplied references

The references define two connected views of the same game:

- **Exploration, reference 2:** a richly dressed village with warm lantern light, timber shopfronts, textured stone paths, plants, small signs, and pixel-art characters. Use readable silhouettes and depth to make a compact location feel inhabited.
- **Conversation, references 1 and 3:** a large illustrated anime-style character portrait, an atmospheric location backdrop, a speaker name, a framed text panel, and clear interaction choices. A small set of portrait expressions communicates reactions.

Approaching an NPC reveals an interaction prompt. Starting a conversation opens the portrait view for that character and location. Ending the conversation returns the player to the same exploration position, with any quest or inventory changes visible.

The dialogue view includes a typed reply field, optional suggested replies, and language help. Suggestions can be edited. Translations, kana readings, pinyin, and corrections appear on request without covering the main interaction. Japanese and Chinese text use legible fonts and generous spacing; decorative framing stays outside the reading area.

Use the welcoming lighting and greenery of the village reference to support the cozy tone, with expressive portraits and illustrated interiors supplying atmosphere. Keep sprites, environments, portraits, and interface ornament consistent in palette and character identity. The final painting and pixel-art treatments remain to be established through a visual prototype.

### First playable milestone

Before expanding to the full story, build one compact 2.5D shopfront scene with a simple 3D environment, a controllable 2D character sprite, one NPC with a portrait dialogue view, and one complete quest. The player can approach the NPC, converse in Japanese or Mandarin, obtain or deliver a quest item, and see a reward or relationship change. Returning to the game restores progress and the NPC remembers the completed interaction.

This milestone must connect the playable world to an actual NPC agent running through Anima's Rust core. It demonstrates exploration, conversation, a game consequence, and continuity together. Player movement and the camera remain responsive while an NPC response is being generated.

### Four characters

Names are working names for the draft.

| Character | Personality and motive | Language situations |
| --- | --- | --- |
| Mira, the innkeeper | Welcoming, practical, wants everyone included at supper | Introductions, preferences, invitations |
| Tavi, the shopkeeper | Curious, particular about promises, needs help with an order | Quantities, prices, requests, clarification |
| Ren, the gardener | Quiet, proud of their produce, reluctant to ask for help | Directions, descriptions, offers of help |
| Bea, the courier | Sociable, often rushed, wants to make a good impression | Messages, plans, times, explaining a problem |

Each NPC is a separate logical agent. They may share a language model while keeping their identity, relevant memories, and permitted knowledge distinct. The game introduces a few deliberate information-sharing events between characters; it does not continuously simulate unrestricted background conversations.

## What Users See

### The play loop

1. Enter the village with one clear immediate objective.
2. Explore and start a conversation with a resident.
3. Communicate a request, learn something, or make a choice.
4. See an observable result: an item, a new destination, a promise, or a relationship change.
5. Use a familiar phrase in a different situation.
6. Finish a short chapter, review useful phrases, and return for the next part.

The world pauses while the player is away. Calendar time does not spoil crops, lose quests, or punish missed visits. Later chapters unlock through story progress and can be played on another visit without a mandatory real-world wait.

### The opening chapter

The player arrives and meets Mira at the inn. Mira asks them to collect bread from Tavi for the village supper. The player asks for directions, introduces themselves at the shop, and explains the request. Tavi has two possible orders, so the player must clarify the recipient and quantity. Returning the correct order completes a visible favor and introduces the supper preparations.

Mira later refers to that favor. Tavi remembers whether the player clarified the order or asked for help. This first chapter should demonstrate a useful conversation, a completed quest, and a remembered interaction within 15 to 20 minutes.

The second chapter introduces a missing basket and a small misunderstanding between residents. The third brings the residents together for supper, resolving a promise made earlier. Each chapter has one main quest with at most three steps. Different conversational approaches can reach valid outcomes; the story does not require one exact sentence.

### Learning during play

NPCs normally answer in the target language using one to three short sentences suited to the player's selected difficulty. Character personality must remain recognizable at simpler language levels.

Players can reveal a word meaning, request a hint, view a translation, or ask for an example reply. Suggested replies can be edited before sending. Menus and help explain what to do without requiring knowledge of the target language.

Meaningful communication advances the quest even when grammar is imperfect. Important errors receive a brief, optional correction or a natural reformulation. A correct sentence must not be marked wrong merely because it differs from an expected phrase. If intent is unclear, the NPC asks a simple clarification question.

Each chapter practices three functions, such as introducing oneself, asking for directions, and clarifying an order. A small reviewed vocabulary set guides generation. The next chapter deliberately reuses earlier language in a new context.

The phrase journal saves three to five useful expressions from the player's actual encounters, with meanings and a short example. Hints and translations do not reduce relationship rewards. The pilot measures short-term task performance and return behavior; it does not claim fluency or proven long-term learning outcomes.

## Rules and Constraints

### Character and world consistency

- Normal conversations are generated during play within authored characters, scenes, and quest boundaries. Recovery lines and reviewed example phrases may be prepared in advance.
- A character's memory distinguishes a witnessed event from something the player merely claimed. Characters receive only the world facts and shared events they are allowed to know.
- Game rules determine inventory, money, quest states, and consequences. A generated sentence cannot create an item, complete an unfulfilled quest, or silently change a price.
- NPCs can offer only available interactions. They cannot promise an unsupported location, mechanic, or future feature.
- Ambiguous requests that would spend coins or give away an item require clarification. Repeating a request or retrying after a connection failure cannot duplicate rewards or charges.
- Saves preserve story state, relevant memories, and learning progress together. A later visit must not pair a completed quest with a character who acts as if it never happened.

### Response quality and recovery

Both learning routes must pass a small human-reviewed scenario set before being advertised as supported. Review covers target-language quality, English explanations, readings, paraphrases, intentional mistakes, and difficulty. The model's own assessment is not the sole quality check.

The game acknowledges a submitted message immediately. If generation fails or stalls, players can retry without losing the action or choose a reviewed recovery reply that keeps the current quest playable. Dialogue is paced for short exchanges, with model calls concentrated on player interactions and relevant story events.

### Runtime fit

Reuse the Rust core and embeddable harness for agent execution and the memory primitives for character continuity. Game-specific world rules, learning progression, presentation, and save integration remain new work.

The recommended approach is a Three.js browser client for the 3D environment and 2D sprites, web conversation panels, and a dedicated Rust game server embedding the reusable agent core. An angled orthographic camera is the proposed starting view. Three.js supplies rendering; movement, collisions, interaction triggers, and the RPG rules remain game-specific work. The server manages authoritative quest outcomes, character memories, saves, and model requests. Players open the hosted game in a browser. A technical design will define this integration before implementation.

## First Customer Test

### Offer hypothesis

Propose a **US$9 one-time pilot pass** for the three-chapter story in one chosen target language. The opening chapter is free and ends with a completed favor before presenting the continuation offer. The full story is approximately 45 to 60 minutes across visits. The price and duration are starting hypotheses, not established willingness to pay or a delivery estimate.

The proposed paid allowance is 200 player message turns across that story, including the free chapter. Hints and translations do not consume additional player turns. Remaining allowance is visible, with advance notice near the limit. After the allowance, saved material remains available and reviewed conversation choices allow the story to finish. There are no automatic overage charges. Final offer terms must be visible before payment.

Collect payment only when the advertised three chapters and all supported routes are playable. Begin with a small cohort so actual usage cost and support needs can be observed before expanding access. Record model usage for dialogue, hints, translations, memory work, and retries, plus payment fees and refunds.

### Recruitment and evidence

Recruit 30 qualified demo players, aiming for fifteen per target language, through relevant language-learning and cozy-game communities. Use a short gameplay clip showing the opening favor and a later remembered interaction. The invitation should describe the actual playable scope and ask players to try it. Recruitment and publishing are proposed work; this draft does not initiate outreach.

Track the path from invitation or landing page to first useful conversation, opening-chapter completion, purchase, and a later visit. Record recruitment source, target language, and help language. Evaluate each route and language separately so an aggregate result cannot hide an unusable experience.

Invite short feedback after the first chapter: what the player wanted to do next, where they became stuck, whether the characters felt distinct, and whether they would pay for the continuation. Actual purchases and return visits carry more weight than stated interest or a waitlist signup.

## Success Metrics

These are proposed decision thresholds for a small pilot. Report counts with percentages; results are directional and do not establish market fit.

| Metric | Proposed target | How to measure |
| --- | --- | --- |
| First useful interaction | At least 24 of 30 demo starters achieve a conversational objective within five minutes | Start and objective events; built-in hints allowed |
| Opening chapter completion | At least 18 of 30 finish without facilitator intervention | Quest and chapter events; log all assistance |
| Paid demand | At least 6 of 30 demo starters purchase at US$9 within seven days of their start | Completed payments, with refunds reported separately |
| Paid continuation | At least 60% of purchasers complete the next chapter within seven days of purchase, in any session | Purchase and chapter events; report numerator and denominator |
| Short-term language transfer | At least 60% of chapter completers handle two of three new situations with no more than a keyword hint | Matched opening and closing prompts; human review, not verbatim repetition |
| Language quality | At least 95% acceptable across at least 30 reviewed exchanges per target language, including readings and English help; zero quest-blocking or materially misleading corrections | Qualified reviewer rubric and issue log; review all progression failures |
| Character continuity | All scripted checks correctly recall witnessed events and respect private knowledge | Opening-to-later-chapter scenario review in each target language |
| Responsiveness | At least 90% of generated replies complete within eight seconds with ten concurrent players | Timed end-to-end sessions, including generation failures |
| Variable delivery cost | AI costs plus payment fees and refunds stay below 25% of gross receipts for the paid cohort | Include that cohort's free chapter, hints, retries, and memory work; separately report all non-paying demo costs |

Report immediate full-story completion and later-day visits separately. For exploratory return analysis, include only purchasers who ended a session with unfinished content. This short story cannot establish sustained retention, and finishing it in one sitting is a valid successful outcome.

The cost threshold is an operating hypothesis. Also report total acquisition, review, art, hosting, and support spending separately; passing the variable-cost check does not establish profitability. Review pilot results after every participant has had the full seven-day purchase window and every purchaser has had seven days to continue.

If players stall before completing a favor, revise the interaction before adding more content. If they finish the free chapter but do not pay, revisit the story promise or offer. If purchasers leave the continuation unfinished, inspect where interest or usability drops. If one language underperforms, inspect its route quality and recruitment source before interpreting the result as lower demand.

## Out of Scope

Full farming simulation, shop management, production chains, real-time combat, multiplayer, romance systems, an open-ended generated world, native mobile or console releases, continuous activity while players are away, and a general-purpose agent editor.

Voice conversation, pronunciation assessment, target languages beyond Japanese and Mandarin, additional help languages, and formal proficiency certification are outside this first pilot. Language progress is tracked through the scoped written activities.

## Decisions for Product Review

The selected direction is a cozy language-learning **browser RPG mixing 2D and 3D**, with generated NPC conversations. Japanese and Mandarin are the starting languages. The game begins with the first playable milestone above before expanding to the three-chapter pilot.

The supplied references establish a detailed pixel-art exploration view and an illustrated portrait conversation view. The three-chapter story, written interaction, adult beginner audience, English help, Simplified Chinese presentation, US$9 offer, and 200-turn allowance are proposed choices for this draft. Three.js, 2D character sprites within a stylized 3D village, and a fixed angled overhead camera are recommended implementation and presentation choices. Review these before implementation planning. A public game name, final asset treatment, final pricing, and production schedule have not been selected.

## References

- [Visual reference 1: illustrated interior, character portrait, and interaction menu](C:/Users/leoca/Downloads/b05588ba58c46cd5981a122a91b1c7d2.jpg)
- [Visual reference 2: pixel-art shopfront, lighting, and exploration view](C:/Users/leoca/Downloads/fbcce44f81ecac19320291ab0ee2892e.jpg)
- [Visual reference 3: character portrait and framed dialogue](C:/Users/leoca/Downloads/b6449bd8e1866b945c50fed081b49833.jpg)
- [PRD index](README.md)
- [Reusable Rust core](../../packages/core-rust/README.md)
- [Embeddable agent harness](../../packages/core-rust/crates/anima-harness/src/lib.rs)
- [Memory and relationship primitives](../../packages/core-rust/crates/anima-memory/src/lib.rs)
- [Three.js camera guide](https://threejs.org/manual/en/cameras.html) and [billboard guide](https://threejs.org/manual/en/billboards.html) — support for the proposed mixed 2D and 3D presentation.
- [Making a game with Three.js](https://threejs.org/manual/en/game.html) — rendering-library boundary and game systems that need to be supplied separately.
- [Mossyrune game overview](https://mossyrune.com/play/how-it-works) — overlapping language-adventure proposition, as described by its publisher.
- [Call Me Sensei](https://www.callmesensei.app/) — overlapping AI-character and language-learning proposition, as described by its publisher. These references establish product overlap, not sales, profitability, or learning effectiveness.
