# 12: Agents, not bots: self-harnessing and the compute economy

Your direction (2026-10-28):
> Set the Thor models up to grow, learn and bootstrap themselves. They are agents, not bots: like pi agents that can re-harness themselves between rounds. The game needs to play from the start. Every agent gets a compute budget and an API budget, in-game and after-game, and choosing how to spend compute, storage and memory allowances is part of the game.

## Pi as the model for the harness
[Pi](https://allthings.how/pi-coding-agent-the-minimal-terminal-harness-you-extend-yourself/) (Mario Zechner) is a minimal coding agent: four tools (read, write, edit, bash), a system prompt under about 1,000 tokens, and everything else built as **extensions the agent can write itself** ([ZenML writeup](https://www.zenml.io/llmops-database/building-pi-a-minimal-extensible-coding-agent-framework), [Pragmatic Engineer](https://newsletter.pragmaticengineer.com/p/building-pi-and-what-makes-self-modifying)).

What carries over: **a tiny fixed core plus a self-written, inspectable extension layer.** An agent isn't a fixed prompt. It's a small kernel that owns and edits its own harness.

## The agent, as data
```
Agent {
  identity:   name, squad, role, voice, persona seed
  harness:    (versioned and content-addressed; the agent can edit it)
    prompts/        system-1 prompt, squad-talk style, tonal lexicon (09)
    reflexes/       sandboxed scripts that run every tick (the scripting layer)
    tools/          small functions it can call (queries over its score view)
    policy.toml     how it spends its budgets (see below)
  memory:     episodic (round logs it chose to keep), semantic (vector DB entries), lexicon
  lineage:    parent harness hash, change notes, scores per version
}
```
The **bootstrap harness** (version 0) is the initial state we write: sensible reflexes, a squad-talk prompt, and situation summaries from the score (option A). The game plays from the first round, and everything after that is the agents' own work.

## The between-round "re-harness" phase
```
round N ends
  → each agent gets: its log slice, outcome metrics (05), human 👍/👎 and tags (09, 11)
  → it runs a pi-style session inside its sandbox, with its after-game budget
      read logs / write and edit its harness files / run headless replays to test changes
  → it proposes harness vN+1 with change notes
  → gates: compiles, passes the sandbox checks, passes golden smoke replays
  → (optional) human review: approve / veto / comment
round N+1 starts with the new harness hashes in the round header (08: version everything)
```
- Squads can also hold a **squad-level re-harness**: shared doctrine and lexicon, with the leader as editor.
- **The same scripting layer the spectator vibe-codes** (the normal-mode option B) is the layer agents edit. Humans and agents work on the same surface.
- Bad versions get **rolled back** automatically if scores regress across N rounds. The lineage tree is kept, so you can see how each agent evolved.

## The compute economy (budgets as gameplay)

| Allowance | Metered by | Spent on |
|---|---|---|
| **Tick compute** (reflex) | WASM **fuel** metering (e.g. wasmtime fuel): instructions per tick | Reflex scripts. Burn too much and the script is cut off that tick, like CROBOTS/RoboWar energy. |
| **System-1 tokens** (Thor) | The Thor scheduler, tokens per round, rate-limited per tick | Fast decisions, squad talk, situation summaries |
| **System-2 calls** (cloud) | Router, credits per round | Mid-round re-planning, deep analysis |
| **After-game compute** | Router plus sandbox | The re-harness session: LLM calls, headless replays |
| **Context allowance** | Prompt builder, max tokens per decision | How much score history and memory to include |
| **Storage** | Vector DB and asset quotas | What memories, logs and lexicon entries to keep |

**Choices agents make (that's the game):**
- Spend System-1 tokens on chatter vs. save them for a late-round crunch?
- Call System 2 mid-round now, or bank credits for a deeper after-game review?
- Keep raw round logs (expensive storage) or only distilled lessons?
- Lean on cheap reflex scripts (fuel) or expensive model calls (tokens)?
- Squads can **pool or trade** budget: the leader decides allocation, which is a natural leadership mechanic.

**Retro precedent: programming games.** *Core War* (1984): programs fight in shared memory under cycle limits. *CROBOTS* (1985) and *RoboWar* (1990s Mac): robot code ran under per-tick instruction or energy budgets, and players rewrote code between matches. *Robocode* (2001). Ours is the same genre, except **the programmers are the agents themselves.**

## Currency: token allotments and an exchange, not dollars (your refinement)

Most capacity isn't paid per call. It's **subscription quota** (currently Claude, MiniMax, Kimi, z.ai) and **local models** on the Thor. So:

**Cloud: a multi-currency purse with an exchange.**
- Each round, the **bank** (the router) allots each squad or agent a quota of tokens *per provider*, carved from what's actually left in each subscription's usage window.
- An **exchange table** converts between providers. For example, 1k Claude tokens = 3k z.ai tokens, reflecting scarcity and capability. You set it, and later it can float with remaining quota (scarcity pricing).
- Agents choose which provider to spend for which thought. Hoarding and trading are allowed.
- ⚠ **Check each provider's terms.** Some subscription plans cover only their own apps or coding tools, and automated use may require API keys. The router should support both subscription-backed and API-key-backed providers.

**Local models: a fair-share queue, not a purse.**
- Local models loaded for the round (decision LLM, summarizer, TTS, etc.) have no token price. Access is **queued by usage**: the agent that has used a model *least this round* goes first, and the heaviest user goes last.
- This creates the strategy you described: **save your local-model priority for late in the round**, so you get fast thinking above your scripts exactly when it matters, while chatty agents early on queue behind everyone.
- **Retro precedent:** the 4.3BSD decay-usage scheduler (1980s Unix). A process's priority dropped as it consumed CPU. Fair-share schedulers (Kay & Lauder, 1988) did the same per user group. Ours resets each round and can optionally decay mid-round. That's a tunable game parameter.
- Squad-level option: usage counts per squad as well as per agent, so one squad can't flood the Thor.

## Hard rules (enforced by the system, never trusted to agents)
1. **Real limits live in the router**, outside any agent's reach: per-provider quota windows, any API-key dollar caps, and the exchange table. Agents only ever see game allotments.
2. **Sandboxing.** Agent-written code runs in WASM (or an equivalent sandbox) with no network, no filesystem outside its harness directory, and fuel and memory limits. Harness edits are files in a versioned store, not live process mutation.
3. **Deterministic replay is preserved.** Harness versions are hashed into the round header. Model outputs are logged as events (08).
4. **Fairness.** Equal starting budgets per squad (a configurable game parameter). Budget accounting appears in the log and on the spectator HUD.
5. **The game never waits.** An exhausted budget means reflex fallback (08, lesson 7), never a stall.

## Milestone placement
- **M1:** the agent-as-data structure, the bootstrap harness, budget metering and HUD (tokens, fuel, credits), hard dollar caps, harness versioning in the round header.
- **M2:** a pi-style re-harness session per agent in the sandbox, with gates and rollback, plus human approve/veto.
- **M3:** squad-level re-harness, budget pooling and trading, lineage browser in Studio mode.
