---
title: Cozy Village Language RPG
author: Unassigned
last_edited: 2026-10-25
version: 1.1
status: draft
scratchboard: null
---

# Cozy Village Language RPG v1.1

> Product proposal, retained as a draft after relocation into `poc/village`. The broader three-chapter paid pilot is future work. See the [current PoC documentation](README.md) for the delivered local chapter and setup; original visual references below remain machine-local source pointers.

| Field | Value |
| --- | --- |
| Author | To be confirmed |
| Version | 1.1 |
| Status | Draft for product review |
| Scratchboard | Not scheduled; product draft only |
| Created | 2026-10-25 |
| Last edited | 2026-10-25 |

> Move into a cozy village, make friends, and practice a language through conversations that remember you.

## Context

Build a small language-learning RPG powered by Anima's reusable Rust agent engine. Players explore a village, help its residents, and develop relationships through conversations generated during play. The first commercial test is a short playable story and a paid pilot.

The product hypothesis is that people will return to practice because they care about the characters and want to discover what happens next. Fresh dialogue supports this experience; the game also needs clear objectives, memorable characters, and consequences players can understand. Demand, retention, learning value, and willingness to pay remain hypotheses to test.

The experience to test is becoming familiar with a small community: a resident recognises a preference, follows up on a promise, or changes a small part of the scene because of an earlier conversation. Each chapter should complete a useful favor and leave one specific reason to speak with a resident again. The first playable must test this personal payoff alongside the technical integration.

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

Use the welcoming lighting and greenery of the village reference to support the cozy tone, with expressive portraits and illustrated interiors supplying atmosphere. Keep sprites, environments, portraits, and interface ornament consistent in palette and character identity. Establish the treatment with one finished shopfront, one player sprite, one NPC sprite and portrait with a small expression set, and one dialogue panel before extending the village. The reference images must be attached or placed at shared project paths before visual review; their current local Windows paths are not portable.

### First playable milestone

Before expanding to the full story, build one compact 2.5D shopfront scene with a simple 3D environment, a controllable 2D character sprite, Tavi's portrait dialogue view, and one complete quest: collect Mira's bread order. A short note introduces Mira's request, so the milestone requires no second speaking NPC or playable location. The player approaches Tavi, clarifies the recipient and quantity, and receives the correct basket in inventory as the collection objective completes.

During the same conversation, Tavi mentions a new bread recipe they hope people will enjoy at the village supper. The player can express a preference between two prepared breads, ask a question, request help, or leave the choice open. This optional exchange adds no quest step. On the next visit to the shop, Tavi remembers the collection and the preference, and a small plate on the counter visibly shows the bread set aside for the player. If no preference was chosen, both options remain available. The callback is triggered by the visit and does not require simulation while the player is away. Assistance and either choice receive the same quest reward and welcome.

This milestone must connect the playable world to an actual NPC agent running through Anima's Rust core. It demonstrates exploration, conversation, an authoritative game consequence, a personal choice, and continuity after closing and reopening the game. Generation must not block rendering or interface controls. During the portrait conversation, movement is deliberately paused and typing goes only to the reply field; ending the conversation restores exploration controls.

Implement and review one language route end to end before adding the second, starting with the route that has the strongest reviewer access. Both Japanese and Mandarin remain required for the advertised paid pilot. Reuse the world and interface, while reviewing phrasing, readings, explanations, difficulty, and acceptable answers separately.

Before expanding, observe target players completing the interaction without facilitator guidance. Record whether they recognise the personal callback, choose to ask an optional follow-up, and can describe something Tavi wants. Fix blockers and unclear payoffs before adding locations or characters. These observations guide the next build; they do not establish lasting attachment.

### Four characters

Names are working names for the draft.

| Character | Personality and motive | Language situations |
| --- | --- | --- |
| Mira, the innkeeper | Welcoming, practical, wants everyone included at supper | Introductions, preferences, invitations |
| Tavi, the shopkeeper | Curious and particular about promises; needs help with an order and is quietly nervous about sharing a new bread recipe at supper | Quantities, prices, requests, clarification |
| Ren, the gardener | Quiet, proud of their produce, reluctant to ask for help | Directions, descriptions, offers of help |
| Bea, the courier | Sociable, often rushed, wants to make a good impression | Messages, plans, times, explaining a problem |

Each NPC is a separate logical agent. They may share a language model and character definition, while keeping memories and relationships isolated by player save and permitted character knowledge. The game introduces a few deliberate information-sharing events between characters; it does not continuously simulate unrestricted background conversations.

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

The player arrives and meets Mira at the inn. Mira asks them to collect bread from Tavi for the village supper. The player asks for directions, introduces themselves at the shop, and explains the request. Tavi has two possible orders, so the player must clarify the recipient and quantity. Returning the correct order completes the favor and makes the bread visible among the supper preparations.

Within the order conversation, Tavi reveals that they are trying a new bread recipe and hope the residents will like it. They ask which of two prepared breads the player prefers. The player can answer briefly, request help, ask about the recipe, or leave the choice open. This optional exchange adds a personal detail without adding another objective. A preference changes a later supported response and visual detail, while either answer preserves the same main quest outcome.

Mira acknowledges the completed favor. On the player's next visit to the shop, Tavi naturally refers to the collection and any expressed preference, with the reserved bread visible on the counter. The callback can happen during an optional revisit in the same session or after reopening the save, without a real-world wait. The free chapter remains explorable within its allowance after the favor is completed. Its continuation prompt points to the upcoming supper and Tavi's hopes for the recipe. Design the main favor and an optional callback to fit within 15 to 20 minutes.

The second chapter introduces a missing basket and a small misunderstanding between residents. The third brings the residents together for supper, resolving a promise made earlier and showing the residents' response to Tavi's recipe. Each chapter has one main quest with at most three steps. Optional personal exchanges stay inside existing conversations or revisits. Different conversational approaches can reach valid outcomes; the story does not require one exact sentence.

### Learning during play

NPCs normally answer in the target language using one to three short sentences suited to the player's selected difficulty. Character personality must remain recognizable at simpler language levels.

Players can reveal a word meaning, request a hint, view a translation, or ask for an example reply. Suggested replies can be edited before sending. Menus and help explain what to do without requiring knowledge of the target language.

For the first reply, show a clear English conversational goal and a visible way to reveal editable examples. Free typing remains available. Test whether players can discover the help without intervention. Confirming Japanese or Chinese input composition must not submit an unfinished message, and typing must not trigger exploration controls.

Meaningful communication advances the quest even when grammar is imperfect. Important errors receive a brief, optional correction or a natural reformulation. Keep explicit grammar explanations in the help interface so characters retain their own voices. A correct sentence must not be marked wrong merely because it differs from an expected phrase. If intent is unclear, the NPC asks a simple clarification question.

Record whether an objective was completed through independently written text, an edited suggestion, an unchanged suggestion, or a reviewed recovery choice, together with any help used. All can advance the story and receive the same relationship reward. Use these distinctions to interpret learning evidence, not to grade the player during the encounter. Prominent character callbacks should concern preferences, choices, and promises rather than the amount of learning help requested.

Each chapter practices three functions, such as introducing oneself, asking for directions, and clarifying an order. A small reviewed vocabulary set guides generation. The next chapter deliberately reuses earlier language in a new context.

The phrase journal saves three to five useful expressions from the player's actual encounters, with meanings and a short example. Hints and translations do not reduce relationship rewards. The pilot measures short-term task performance and return behavior; it does not claim fluency or proven long-term learning outcomes.

## Rules and Constraints

### Character and world consistency

- Normal conversations are generated during play within authored characters, scenes, and quest boundaries. Recovery lines and reviewed example phrases may be prepared in advance.
- A character's memory distinguishes a witnessed event from something the player merely claimed. Characters receive only the world facts and shared events they are allowed to know.
- Game rules determine inventory, money, quest states, and consequences. A generated sentence cannot create an item, complete an unfulfilled quest, or silently change a price.
- NPCs can offer only available interactions. They cannot promise an unsupported location, mechanic, or future feature.
- Ambiguous requests that would spend coins or give away an item require clarification. Commit an action's world changes, witnessed-event record, and request identifier together before displaying success. Retrying the same request returns its original committed result without duplicating rewards or charges.
- Saves preserve story state, relevant memories, and learning progress together, isolated by player save and each character's permitted knowledge. Committed game events take precedence over generated memory summaries. Refresh stale summaries before they can contradict a restored save; dialogue must agree with the committed outcome.

### Response quality and recovery

Both learning routes must pass a small human-reviewed scenario set before being advertised as supported. Review covers target-language quality, English explanations, readings, paraphrases, intentional mistakes, difficulty, ambiguous orders, resumed sessions, and recovery. The model's own assessment is not the sole quality check. The initial sample is a release check for these scenarios, not a reliability estimate for all possible conversations.

The game acknowledges a submitted message immediately. If generation fails, or no usable reply is available after eight seconds, offer retry and a reviewed recovery choice without losing or duplicating the action. Retry and recovery must reconcile the same pending request and committed outcome. A delayed generated response cannot overwrite an accepted recovery choice. Dialogue is paced for short exchanges, with model calls concentrated on player interactions and relevant story events. Observe perceived waiting as well as measuring completion time; an acknowledgement or spinner is not conversational content.

### Runtime fit

Reuse the Rust core and embeddable harness for agent execution and the memory primitives for character continuity. The first playable must verify what can be reused in practice. Game-specific world rules, learning progression, presentation, and save integration remain new work. The game server owns committed outcomes and the authoritative event record; NPC context and memory summaries derive from those events.

The recommended approach is a Three.js browser client for the 3D environment and 2D sprites, web conversation panels, and a dedicated Rust game server embedding the reusable agent core. An angled orthographic camera is the proposed starting view. Three.js supplies rendering; movement, collisions, interaction triggers, and the RPG rules remain game-specific work. The server manages authoritative quest outcomes, character memories, saves, and model requests. Players open the hosted game in a browser. A technical design will define this integration before implementation.

## First Customer Test

### Offer hypothesis

Propose a **US$9 one-time pilot pass** for the three-chapter story in one chosen target language. The opening chapter is free and ends with a completed favor before presenting the continuation offer. The full story is approximately 45 to 60 minutes across visits; purchase adds two chapters, approximately 30 to 40 minutes of new story. The price and duration are starting hypotheses, not established willingness to pay or a delivery estimate.

Propose **40 generated conversation turns for the free opening**. Purchase unlocks the remaining two chapters and raises the allowance to **200 turns total in the same save**, including turns already used in the opening. These limits are offer hypotheses to test. One turn is a submitted player message that receives a usable generated NPC response. Failed requests and retries of the same message do not consume additional turns. Hints, translations, and reviewed recovery choices do not consume conversation turns.

Remaining allowance is accessible from the dialogue interface, with advance notice near the limit. Observe whether the counter discourages optional conversation. At the free limit, reviewed choices allow the opening chapter to finish. At the paid limit, reviewed choices allow all three chapters to finish. After the final supper, any remaining turns may be used in that existing scene to discuss established events and preferences; NPCs cannot create new quests, rewards, or locations. The story does not reset and the allowance does not replenish. Once it is exhausted, the completed save and phrase journal remain available. There are no automatic overage charges. Final allowance, access, and continuation terms must be visible before payment.

Collect payment only when the advertised three chapters and all supported routes are playable. Begin with a small cohort so actual usage cost and support needs can be observed before expanding access. Record model usage for dialogue, hints, translations, memory work, and retries, plus payment fees and refunds.

### Recruitment and evidence

Recruit 30 qualified demo players, aiming for fifteen per target language, through relevant language-learning and cozy-game communities. Use a short gameplay clip showing the opening favor and a later remembered interaction. The invitation should describe the actual playable scope and ask players to try it. Recruitment and publishing are proposed work; this draft does not initiate outreach.

Track the path from invitation or landing page to first useful conversation, opening-chapter completion, purchase, and a later visit. Record recruitment source, target language, and help language. Evaluate each route and language separately so an aggregate result cannot hide an unusable experience.

Invite short feedback after the first chapter: what the player wanted to do next, where they became stuck, whether the characters felt distinct, and whether they would pay for the continuation. Actual purchases and return visits carry more weight than stated interest or a waitlist signup.

Also ask which resident the player would choose to speak with next and what they would want to ask. Observe optional follow-up conversations after the objective is complete, voluntary revisits, recognition of a personal callback, and whether the player can describe a resident's concern. Record whether further conversation was possible with the remaining allowance. Use these observations to explain interest and drop-off alongside purchase and return behavior; they do not establish lasting attachment.

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
| Responsiveness | At least 90% of submitted conversation turns produce a complete, usable generated reply within eight seconds with ten concurrent players | Report median and 90th-percentile times to the first complete readable sentence and final usable reply, separately by language; failures and timeouts count as misses, and acknowledgements do not count as dialogue |
| Variable delivery cost | AI costs plus payment fees and refunds stay below 25% of gross receipts for the paid cohort | Include that cohort's free chapter, hints, retries, and memory work; separately report all non-paying demo costs |

Report immediate full-story completion and later-day visits separately. For exploratory return analysis, include only purchasers who ended a session with unfinished content. This short story cannot establish sustained retention, and finishing it in one sitting is a valid successful outcome.

Collect the opening language baseline before introducing the practiced material. Closing situations test the same communication functions with changed details. Report final task success, assistance used, and paired changes separately. These are short-term observations; the pilot cannot attribute improvement to the game alone. Keep the character-interest observations qualitative and report concrete counts alongside them.

The cost threshold is an operating hypothesis. Also report total acquisition, review, art, hosting, and support spending separately; passing the variable-cost check does not establish profitability. Review pilot results after every participant has had the full seven-day purchase window and every purchaser has had seven days to continue.

If players stall before completing a favor, revise the interaction before adding more content. If they finish the free chapter but do not pay, revisit the story promise or offer. If purchasers leave the continuation unfinished, inspect where interest or usability drops. If one language underperforms, inspect its route quality and recruitment source before interpreting the result as lower demand.

## Out of Scope

Full farming simulation, shop management, production chains, real-time combat, multiplayer, romance systems, an open-ended generated world, native mobile or console releases, continuous activity while players are away, and a general-purpose agent editor.

Voice conversation, pronunciation assessment, target languages beyond Japanese and Mandarin, additional help languages, and formal proficiency certification are outside this first pilot. Language progress is tracked through the scoped written activities.

## Decisions for Product Review

The selected direction is a cozy language-learning **browser RPG mixing 2D and 3D**, with generated NPC conversations. Japanese and Mandarin are the starting languages. The game begins with the first playable milestone above before expanding to the three-chapter pilot.

The supplied references describe a detailed pixel-art exploration view and an illustrated portrait conversation view. The three-chapter story, written interaction, adult beginner audience, English help, Simplified Chinese presentation, US$9 offer, 40-turn free opening, and 200-turn total allowance are proposed choices for this draft. The bounded conversation after the final supper uses only the existing scene and remaining allowance. Three.js, 2D character sprites within a stylized 3D village, and a fixed angled overhead camera are recommended implementation and presentation choices. Review these before implementation planning. A public game name, final asset treatment, final pricing, and production schedule have not been selected.

### Revision 1.1

- Added a personal concern for Tavi, an optional preference, and a visible callback in the existing shop scene.
- Defined the first playable's character-interest observations and one-language-first implementation sequence.
- Clarified language-help presentation, input behavior, and assistance-aware interpretation of learning results.
- Tightened per-save memory, committed outcomes, retry recovery, and responsiveness measurements.
- Specified a proposed free allowance, purchase upgrade, and bounded use of remaining turns after the story.
- Flagged the visual references for portable sharing and updated the Three.js documentation links.

## References

- [Visual reference 1: illustrated interior, character portrait, and interaction menu](C:/Users/leoca/Downloads/b05588ba58c46cd5981a122a91b1c7d2.jpg)
- [Visual reference 2: pixel-art shopfront, lighting, and exploration view](C:/Users/leoca/Downloads/fbcce44f81ecac19320291ab0ee2892e.jpg)
- [Visual reference 3: character portrait and framed dialogue](C:/Users/leoca/Downloads/b6449bd8e1866b945c50fed081b49833.jpg)
- [PoC documentation index](README.md)
- [Reusable Rust core](../../../packages/core-rust/README.md)
- [Embeddable agent harness](../../../packages/core-rust/crates/anima-harness/src/lib.rs)
- [Memory and relationship primitives](../../../packages/core-rust/crates/anima-memory/src/lib.rs)
- [Three.js camera guide](https://threejs.org/manual/pages/cameras.html) and [billboard guide](https://threejs.org/manual/pages/billboards.html): support for the proposed mixed 2D and 3D presentation.
- [Making a game with Three.js](https://threejs.org/manual/pages/game.html): rendering-library boundary and game systems that need to be supplied separately.
- [Mossyrune game overview](https://mossyrune.com/play/how-it-works): overlapping language-adventure proposition, as described by its publisher.
- [Call Me Sensei](https://www.callmesensei.app/): overlapping AI-character and language-learning proposition, as described by its publisher. These references establish product overlap, not sales, profitability, or learning effectiveness.
