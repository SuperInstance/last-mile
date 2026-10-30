# Prior art for a sandbox that children change by talking

Research companion to `docs/design/sandbox-on-otherhere.md`. Every claim below carries a numbered marker that resolves to a full URL in the reference list. Sources were retrieved and read on 30 October 2026.

## Scope, method, and the limits of the evidence

"Prior art" here means external work that informs the design: what comparable systems did, what worked, what failed, and what that implies for the sandbox. It is not a patent opinion and it is not legal advice.

The evidence is graded. **Primary** sources are research papers, regulatory text, official documentation and first-party statements by the builders of a system, and are preferred wherever a claim is load-bearing. **Secondary** sources are press reporting, used mainly for direct first-hand critical observation (a reviewer describing how a system actually felt) where no primary artefact exists; these are marked as such in the text. One vendor document (Neocortex) describes that vendor's own product and is treated as primary for design intent but not as independent evaluation.

Four boundaries are worth stating before the findings, because they shape what the evidence can and cannot settle:

1. **Children authoring games and children talking to software are different problems.** Scratch, Mario Maker, Kodu and Bloxels are creation environments where the child produces the artifact. AI Dungeon and Ubisoft NEO NPC are runtime systems where a model produces text into a child's ear. The risk surfaces differ: the first fails on engagement and comprehension, the second on safety and trust. Section 1 and Section 2 are kept apart for this reason, and claims do not transfer between them without saying so.

2. **The sandbox's central bet is not that a model writes code.** It is that a bounded, typed change, proposed and then tested, keeps a child in control. The prior art for that is the *interaction pattern* — propose, show, accept, test, undo — not the generation. Section 3 is mostly about the pattern.

3. **Legal exposure is set by deployment, not by intent.** The same product is a different legal object depending on whether a microphone buffer reaches a local recogniser or a cloud service, and whether personal data is processed online. Section 4 reads the rules, then Section 4.4 draws the line. This is the same distinction the design document already flags as risk R6; the external evidence confirms it and sharpens it.

4. **One part of the design has no external precedent.** The sealed prediction — a prediction fixed before the test, run against held-out seeds, with the child adjudicating the verdict — has close relatives in the learning sciences and in agent evaluation, but no published system combines those three elements for children. Section 5.1 says so plainly rather than dressing up an analogy. The honest position is that this is an experiment with a specified failure state, not a pattern with a track record.

---

## 1. Creation tools and the points where children get stuck

### 1.1 Scratch: a practice that survives the tool

Scratch was built to be used without instruction. In a study of 536 projects created by urban youth aged 8 to 18 over eighteen months at a Computer Clubhouse, children learned key programming concepts even without instructional interventions or experienced mentors [1]. The 2009 CACM account gave the tool its canonical description of the "creative thinking spiral" — imagine, create, share, reflect, imagine — and noted an audience of 8 to 16 peaking at age 12, with more than 1,500 new projects uploaded every day [2]. The spiral matters here because it is the loop the sandbox is trying to compress: in Scratch the loop takes an afternoon and a peer audience; the sandbox proposes to make one turn of it take ninety seconds.

The more useful findings are about what children produce, not what they feel. Aivaloglou and Hermans scraped roughly a quarter of a million public Scratch projects — 247,798 in the analysis, 233,491 non-empty, from 109,960 distinct users [3]. They found code clones across sprites in 25.54% of projects, dead code in 28%, large scripts of eighteen blocks or more in 29.77%, and large sprites in 13.68%. Conditionals appeared in 40% of projects and variables in 32%. Loops appeared in 77%, but conditional loops in roughly 14%: procedures and conditional loops, the constructs that separate a program from a sequence of commands, were simply not commonly used.

A controlled experiment by the same group, with 61 novice Scratch programmers split across a non-smelly group and two smelly groups, found that smelly programs produced significantly worse correctness, and that the two smells had different causes. Long Method mainly **decreased system understanding**. Duplication made programs **harder to modify** [4]. Both results transfer directly and uncomfortably. A child who cannot hold the system's behaviour in their head will not attempt a change they cannot predict. A child whose earlier work is full of duplication will find modification progressively harder, which is an argument for a design in which the child edits a small, typed surface rather than an accumulating codebase.

ChatScratch, a CHI 2024 study of 24 beginner children aged 6 to 12, found three obstacles to autonomous Scratch learning: artist's block in project planning, bounded creativity in asset creation, and inadequate coding guidance during implementation [5]. The tool itself was a partial fix — children adopted about 74% of the suggested code and extended it in about 54% of cases, and Dr. Scratch's computational-thinking score rose from 8.13 to 13.21. But the asset-creation bound and the planning block were properties of the environment, not of the assistant. The nearest thing to an answer for asset creation is that Scratch simply does not solve it, and children work around it by importing sprites from the community.

### 1.2 Mario Maker: creators guessing the difficulty of their audience

Mario Maker is the best available natural experiment on what makes people keep making things, because the platform publishes both what creators made and what players did with it.

A 2025 analysis in *Scientific Reports* assembled three user-generated-game datasets — Robozzle with 7,994 levels and 376,341 votes, Trackmania with 138,662 levels and 614,770 votes, and Super Mario Maker with 115,032 levels, 795,313 votes and 32,665,615 player attempts [7]. It found an inverted-U relationship between difficulty and enjoyment in all three, and that players preferred levels whose *expected* difficulty matched the *computed* difficulty, with the peak at zero difference. Success also drove enjoyment: players who completed levels they could finish enjoyed them more [7]. A separate 2025 analysis of a snapshot of 26,609,725 Super Mario Maker 2 levels reports a median level being cleared ten times, played fifty-four times, receiving one like and one boo, and states that makers with more experience tend to design easier levels [8].

The design work in Super Mario Maker has been studied as labour. More than seven million stages had been created, and the paper distinguishes playful work from laborious play, describing designers who hypothesise about player mental states and build elaborate difficulty spikes, alongside trolls who build levels designed to frustrate [9].

The lesson for the sandbox is not about difficulty. It is about measurement. Enjoyment in that study was computed behaviourally — the proportion of players who attempted a level and also endorsed it — rather than asked for [7]. That is the same measurement the sandbox's thumb is already in a position to make, and it is a better instrument than a self-report from a nine-year-old.

### 1.3 Kodu, Bloxels and Dreams: the complexity ceiling

Kodu is the purest statement of the design idea that predates every LLM: a visual programming language in which a child programs an entire game out of icons and game-specific primitives, with a controller as the only input device, built for rapid design iteration [12]. Its companion, StudioK, wrapped the language in a curriculum and added learning analytics so a teacher could monitor student progress, **see when students are stuck or need help**, and give specific feedback [13].

Bloxels takes the same idea into physical space: a board of blocks captured by a camera, where building a game requires no coding at all, marketed at ages nine and up, with a separate code-based board that requires the child to program all behaviour [15]. Bloxels EDU adds a class library and a teacher surface for reviewing student work, and requires parental permission before a child under thirteen can share to the public Infinity Wall [14].

Dreams is the cautionary version. Media Molecule ended live support on 1 September 2023 and stated plainly that there is no way to export creations from Dreams; storage was capped at 5 GB online and 1 GB locally [10]. Players said the same thing in the coverage: the learning curve plus the fact that creations were locked to the console were the biggest complaints, and when you build something and cannot take it with you it makes it not worth it [11].

Taken together, these three tools fail at the same place, and it is a specific place. Kodu is small enough that the ceiling is visible but the surface is narrow. Bloxels removes the ceiling and moves the cost to the codeboard, where the child must program every behaviour. Dreams removes the ceiling entirely and pays for it with complexity and with lock-in. **The idea outgrows the tool, and what happens then is not that the child wants a better tool — it is that the child stops, or the work becomes trapped.**

### 1.4 Minecraft Education: the moment the code destroys the work

This is the most important finding in the section, and it comes from a classroom observation study published through the NSF's Program for Accelerating Academic Learning, using data collected at the RESPECT conference in 2022.

In one case, a seven-year-old girl ran code that destroyed significant portions of her project. She was visibly upset, said she was giving up on Minecraft, and refused to try building the structure again; she was convinced she could not do it properly using the code, and the code builder became an object of scorn [16]. In the second case, a boy avoided using the coding capabilities of Minecraft Education altogether, out of fear that the code might not work properly — it was too risky, and if he tried to use the code something might go wrong and portions of his arena would need to be reconstructed. When offered a copy of his world before testing, he refused. The study's analysis was that the code-testing process did provide useful information he could process and adapt, but that doing that iteration in his actual arena was out of the question [16].

Two findings sit inside that, and both are load-bearing for the sandbox.

The first is that **these children did not abandon the tool because the code was hard. They abandoned it because running the code could destroy the thing they had made.** The tool was fine. The stakes were asymmetric — a successful test gained them a little, a failed test cost them their project — so they rationally refused to run it.

The second is that **offering a backup copy did not fix it.** The study records that the copy was offered and declined. What the child needed was to know *before* running that the run was safe. That is a statement about trust and about ordering, not about backup.

A second body of evidence shows the same environment working when the stakes are structured. A 2025 case study of 204 Grade 10 students across six classes at MAN 9 Jakarta found computational thinking developing through decomposition and iterative debugging, coding skills growing through visual block programming with immediate feedback, and — the detail that matters most here — students describing programmable agents as rule-based systems and attributing errors to flawed code rather than autonomous intent [17]. The finding held only under intentional scaffolding and reflective pedagogy. A French-language exploratory study of 118 elementary students across thirty tasks in ten progressive levels reported gains in motivation, programming and peer collaboration [18], and a 2022 classroom study of twenty fifth-graders found sequences, events, loops and parallelism used well while variables, operators and conditionals were used least, with testing and debugging the most-used and remixing the least-used [19].

The phrase to carry forward is the one from [17]: **children who learn to attribute an outcome to the change rather than to the game's mood.** That is a teachable outcome, and the MAN 9 study is evidence it is teachable. It is also, almost exactly, the thing the sandbox's Glitch Meter exists to produce.

---

## 2. Free-form speech and generative characters

### 2.1 AI Dungeon: the cost of filtering what players say

AI Dungeon is the most instructive failure in this space, and it is instructive because the company published its own account [20].

In 2021 the service faced an ethics challenge after players wrote stories depicting the sexual exploitation of children. Latitude's two options were to implement filtering quickly or to adopt OpenAI's filters, which were far too restrictive — no sex, and no fantasy violence either. They chose to build their own, fast. The consequences were a chain of compounding failures:

- The rushed filter produced **false positives**, blocking innocuous content.
- The system implemented **algorithmic banning**, and it **erroneously banned innocent players**.
- To comply with OpenAI's terms, Latitude had to **manually moderate player stories** — including unpublished private adventures, which the community regarded as an invasion of privacy.
- Community members **discovered unsafe data in the AI Dungeon fine-tune**, which had to be removed and the model retrained.
- The base model **controlled by OpenAI also contained unsafe data**, so the AI continued generating unsafe content and players were banned for text they did not write.

Manual moderation of unpublished content was ended in August 2021. The current state, per the company's own help page, is that story data is encrypted, model data collection is anonymous and opt-in only, and **no human moderates unpublished content** [20].

### 2.2 The walls approach

Latitude's subsequent write-up reframed the whole problem, and it does so in game-design vocabulary that should be familiar here [21].

Their argument is that single-player games already solve this: if there are types of content or actions the developers do not want in a game, they make those impossible — "in Skyrim, it's impossible to kill kids." Those walls protect the players, the company, and the experience, and within them players are free to play, explore and create whatever they want. Their words: "We have the reverse problem. Games like AI Dungeon start with 'you can do anything' and we then need to constrain the AI to prevent it from going places we don't want."

The resulting architecture, in their terms:

- **No moderation of unpublished single-player content, no consequences for users, and no human review** of a user's content when a wall is hit.
- Users can do or say what they want in single-player play, but the AI may decline to generate certain types of content.
- Every AI output is checked, and because several candidate generations are produced, there is usually at least one that can be delivered. Where there is not, **the user gets a message letting them know, with ways to continue their story, and a route to report** so the system can be improved.
- The architecture is **provider-agnostic**: a request that a third-party provider's content policy will not accept is routed to a different model.
- Story data is **encrypted at rest**, decrypted on the device and immediately before being sent to the model.
- False positives are handled with **five lifetime bypasses of thirty minutes each**, plus a report flow; a review of hundreds of user-submitted reports of mistaken blocks found one genuine false positive.

This is the single most important piece of prior art in the report, and it cuts two ways. It confirms the wall metaphor that the sandbox's Vibe Dictionary already uses. It also demonstrates that AI Dungeon's wall is still a **filter on model output** — it is simply a filter with no punishment attached. The sandbox's wall is stronger, because it is on the *input* side: a child cannot express a change outside the 48 blocks at all, so there is no output to filter. The transferable lesson is not the mechanism. It is the sequence of failures that got them there: over-blocking, then punishment, then human review of private content — each of which broke trust — before they arrived at walls with no consequences and always a way forward.

### 2.3 Ubisoft NEO NPC: conversation that changes nothing

Ubisoft's NEO NPC prototype was presented at GDC in March 2024, and the two most informative descriptions of it — the company's own and the press's — are in direct tension.

Ubisoft's account describes a system built on a large language model acting on an action plan, with **characters authored by writers**: a writer shapes the personality, backstory and conversation style, and then continues to tune it once the model starts improvising dialogue [22]. The tool involves guardrail systems, analysis of player input, the 3D environment and textual instructions, and the model's task is framed as impersonating a character [22]. Ubisoft's technical director was candid about the failure modes: nonsense in, nonsense out; a lot of filters to catch toxicity and inappropriate player input; and a specific design decision to distinguish the player *antagonising* an NPC from merely *insulting* them, because over-scoping the toxicity filter made the model stop being collaborative [22]. He also described a bias correction — an attractive female character whose answers veered flirtatious and seductive had to be reprogrammed [22] — and was explicit that these characters do not have free will: they are there to play a role in a story, and they have a narrative arc.

Eurogamer's reporting was harsher on the thing that matters to this design. Ubisoft's scientific director said that LLM mods "were often somewhat 'empty' or 'shallow' in practice," and concluded: "While they can technically say anything, what these NPCs say doesn't impact the game itself, and so what you end up with... is 'roleplay, not gameplay.'" The same piece noted that the distinguishing feature of NEO NPC was that the characters are "ultimately positioned within 'walls' set by humans. So you can suggest scuttling by the guards in a box, but unless that's one of the 20-odd pre-set solutions in this scenario, you can't actually do it." Eurogamer's summary: "All of the infinite conversation on top of that is effectively colour for the sake of your immersion." And: "Very little of the dialogue spoken would pass a human editor" [23].

Two further details from that piece bear directly on the design. Input is voice-to-text — hold a button, speak into a microphone. And Ubisoft itself noted that without voice-to-text as an input, or with a keyboard, the experience would not be preferred by most players, and that removing the voice element would mean reverting to standard game design: "having a set list of dialogue options, maybe some of them hidden until you discover something in game to trigger their availability" [23].

The Verge's hands-on was blunter about delivery: the conversation still "feels a bit stunted," with lag before replies and vocal hesitations and stutters, and The Verge's own attempt to get the NPC to discuss its place in the game went sideways [24].

This is the most valuable prior art in the report for one specific reason. **Ubisoft's vocabulary and the sandbox's vocabulary are the same, and Eurogamer's complaint is the sandbox's opportunity.** Ubisoft built free-form conversation inside human-set walls of roughly twenty pre-set solutions, and the result was judged to be roleplay because the conversation did not change the game. The sandbox's §4 already separates the co-developer (Bench, whose dialogue *is* the game state change) from the rival (Bolt, whose dialogue is social). The prior art says that separation is not a nicety — if the conversation does not alter the state, it is decoration, and a seven-year-old will find that out.

### 2.4 Latency and memory as the binding constraints

The generation quality of these systems is not the limiting factor for a children's product. Two measurements say so.

Microsoft's Muse family of world models was published in *Nature* in February 2025. WHAM was trained on more than a billion images and controller actions captured from Ninja Theory's *Bleeding Edge* — over seven years of continuous human gameplay — at 300×180, generating roughly one image per second. The paper identifies three capabilities as critical for aligning with creative practice: **consistency, diversity, and persistency**, the last meaning that user modifications persist [27].

The real-time extension, WHAMM, was published in April 2025 and makes the constraints explicit [26]:

- Speed: 10+ frames per second, at 640×360, using a MaskGIT rather than an autoregressive setup.
- **Context length: 0.9 seconds of gameplay** — nine frames at 10 fps — which means "the model can and will forget about objects that go out of view for longer than this."
- **Scope: trained on a single part of a single level of Quake II.** "If you reach the end of the level, then the generations freeze because we stopped recording data at that point and restarted the level."
- **Latency: "Making WHAMM widely available for anybody to try at scale has introduced noticeable latency into the actions."**
- Enemy interaction is "a big area for improvement," with combat sometimes incorrect.
- Their own framing of the goal: "Think of it as *playing the model* as opposed to playing the game."

The generative-agents work points the same way. The Stanford paper populated what it calls an interactive sandbox environment inspired by *The Sims* with twenty-five agents driven by a memory stream, retrieval, reflection and planning [25]. Its ablations are the relevant numbers: the full architecture scored a mean TrueSkill of 29.89, dropping to 26.88 without reflection, 25.64 without reflection and planning, 22.95 for a human crowdworker baseline, and 21.21 for a fully ablated model — a Cohen's d of 8.16 across the full architecture and its ablations. The paper also reports **hallucination in 1.3% of 453 responses**, and names three characteristic failures: failed memory retrieval, fabricated embellishments, and inherited overly formal speech. It concedes that its evaluation covers only a short timescale and that robustness remains "largely unknown," with the agents vulnerable to prompt hacking and memory hacking [25].

The design implication is direct. A system with sub-second memory and a frame rate near the limit of real time can support a sandbox where the child changes one typed parameter and sees a deterministic simulation respond. It cannot support a sandbox where the model holds the world in its head, invents the world's content, or is trusted to remember what it did last round. The sandbox's architecture — deterministic simulation, hash-chained replay, held-out seeds — is a response to exactly this constraint, arrived at from the opposite direction.

### 2.5 Agents that write and verify their own changes

The closest published analogue to the sandbox's dictionary is Voyager, an LLM agent that plays Minecraft [28]. It has three components: an automatic curriculum, an ever-growing **skill library of executable code**, and iterative prompting grounded in environment feedback and execution errors. Skills are added to the library only after the agent's own verification confirms them; the library is retrieved by embedding similarity, so skills compose.

The ablations are the load-bearing part. Remove the skill library and the agent plateaus. **Remove self-verification and the number of items discovered drops by 73%.** The GPT-3.5 version "fell off a cliff" at the early game tree [28].

The pattern is the sandbox's Vibe Dictionary and Glitch Meter in another guise: a change is admitted to the corpus only after a verification gate passes, and the gate is the load-bearing part rather than the generation. What does not transfer is the verifier. Voyager's critic is a strong model grading a simulation it can see completely. The sandbox's verifier is a set of held-out seeds on a deterministic simulation — which is *stronger*, because it is repeatable — but its accuracy is unmeasured, which Voyager's is not.

---

## 3. Bounded changes and the shape of agreement

### 3.1 The accept, diff and undo pattern

The pattern the sandbox borrows is well documented in two places, one for adults and one for children.

GitHub's Copilot documentation is explicit about the mechanics of inline suggestions: they "follow human-in-the-loop principles — suggestions are visually distinct from the surrounding content and are only applied when the user explicitly accepts them," a safety system scans prompts and responses to block harmful or insecure content, and "users are responsible for reviewing and validating suggestions before accepting them" [29]. The documentation also contains a warning that is easy to skip and expensive to ignore: content exclusion is a guardrail for the tools GitHub controls, and "it is not guaranteed to be respected by third-party MCP servers" [29].

Roblox ships the same pattern to children, in Roblox Studio, and its documentation is the most useful single source in this report because it is written for the actual audience. The Assistant shows the script's contents and adds the new script instance to the currently selected object in the data model. It warns that "the generated script might not function flawlessly" [30]. It then publishes an explicit table of what can go wrong, including requests that produce code but do not create a new script, add code to a random existing script, or produce parts that sparkle before they are touched — and the guidance that "the fewer details you provide, the more unpredictable the results" [32]. The child presses **Accept**. Changes to the scene are undone with Ctrl+Z or ⌘Z, and scripts are removed from the Explorer [32]. Roblox's own curriculum tells students: "**You must always check the results of its suggestions to make sure they work as intended**" [32]. The loop closes with a playtest and a thumbs up or down, which tells the Assistant whether the code sample worked as the child intended [32]. The final admission is that "Assistant is constantly learning, so it might not always produce the same results for the exact same request" [32].

Roblox's own build guide sequences it as Plan, Build, Playtest, Refine [31]. That is the sandbox's round, minus the fact that Roblox's proposal is unbounded and the sandbox's is typed.

Two things follow. First, the **accept-and-test pattern is validated for children at scale**, by the largest platform serving that age group. Second, the **reliability of the proposal is the weak link**, and the platform that shipped it says so in its own teaching material.

### 3.2 Grammar sets and refusals a child can survive

The other line of prior art is the voice-command set: a bounded grammar with a fixed set of valid phrases, as opposed to free-form interpretation.

The designer of *Plan Be* is unusually direct about it: "the player cannot utter any phrase they want and expect the game to do something. There are various phrases and words that have been written in a file that the game will understand" [33]. Their accumulated advice is a checklist for exactly the failure modes the sandbox's fail-state is designed against [33]:

- Make the grammar something people feel natural saying, and playtest it.
- Manage the active words and phrases with rules; you do not need every dialogue phrase active all the time.
- Think about how you will teach the grammar set if it is large.
- **Make speech failure into a feature** — the character says they did not hear you, or there is static to show the input was not registered.
- Do not expect people to get accustomed to it easily; ease them into it.
- Give players breathing room between the times they have to use their voice.

And the structural point, which is the closest prior art to the sandbox's §4 split: "Talking to a character is a story voice mechanic and I needed a gameplay voice mechanic too." One channel free, one channel bounded, and the distinction made deliberately [33].

Neocortex, a voice-command system for game mods, implements the same division as a shipped product, and its documentation is vendor-primary rather than independent [34]: recognition is **on-device and offline**; the command space is a graph of actors, actions and objects, with example phrases attached to each action; the engine matches meaning rather than exact wording; multiple actions and quantities can be combined in one sentence; and, explicitly, "a typical mod routes short commands to the on-device engine and longer speech to an AI character through the Neocortex API" [34].

That is, in someone else's shipping product, the sandbox's architecture: a small typed vocabulary handled locally, and free speech handled by a model, with the split made before the user has to learn it.

### 3.3 Generated code a child cannot read

MindScratch studied twenty-four fifth-graders aged 10 to 11 working in Scratch and in a multimodal generative variant [6]. The result that matters for the sandbox: **only 13 of the 24 students using plain Scratch achieved the teacher's learning objectives.** And the educators' assessment of the AI-assisted condition was that the generated code "remains difficult to comprehend, especially for beginners" [6].

Generation that a child cannot read is not scaffolding. It is substitution. The sandbox's answer — a prediction the child states, a sealed verdict the child sees, and a thumb that adjudicates — is a mechanism for keeping the child in the loop of the causal chain rather than a spectator of it. §5.1 attaches the learning-science support for that ordering.

---

## 4. Children's data, voice and the law

### 4.1 COPPA: the audio exception and the retention rules

The Children's Online Privacy Protection Act Rule, 16 CFR Part 312, was amended by a final rule the Federal Trade Commission voted to issue in January 2025 and published in the Federal Register on 22 April 2025, with an effective date of 23 June 2025 and a compliance date of 22 April 2026 [36][37]. The amended rule is now in force and past its compliance date.

The provision that bears most directly on the sandbox is § 312.5(c)(9), reproduced here as it stands in the Code of Federal Regulations [35]:

> Where an operator collects an audio file containing a child's voice, and no other personal information, for use in responding to a child's specific request and where the operator does not use such information for any other purpose, does not disclose it, and deletes it immediately after responding to the child's request. In such case, there also shall be no obligation to provide a direct notice, but notice shall be required under § 312.4(d).

Four conditions, and the sandbox's microphone design maps onto all four: the audio file contains the child's voice and no other personal information; it is used solely to answer the child's specific request; it is not disclosed and not used for anything else; and it is deleted immediately after the response. The "no other personal information" condition is the restrictive one in practice, and it is why a transcript kept in a deposit file is not covered by the exception.

Three other changes in the 2025 rule bear on the design [36][37][39]:

- **Separate verifiable parental consent** is required before disclosing a child's personal information to third parties. The Commission was explicit that disclosures for advertising, for monetary compensation, or for **developing or training artificial intelligence technology** are *never* integral to the service, so the separate-consent requirement applies to them.
- **Retention limits.** Personal information may be kept only as long as reasonably necessary; indefinite retention is prohibited; and operators must maintain and disclose a written data retention policy stating the business need for the data and the timeframe for deletion.
- **Biometric identifiers.** The definition of personal information now expressly includes voiceprints. The Commission declined to adopt the broader "data derived from voice data" language that had been proposed, on the grounds that it would capture data that cannot be used to identify an individual.

The Chair's statement gives the reason in one line: "Machine learning is no excuse to break the law. Claims from businesses that data must be indefinitely retained to improve algorithms do not override legal bans on indefinite retention of data" [38].

### 4.2 Amazon Alexa and the refusal of the training justification

The enforcement action that establishes how these rules are applied is FTC and DOJ v. Amazon, brought on 31 May 2023 [40]. The Commission obtained a $25 million civil penalty and a stipulated order. The facts, per the FTC [40] and its accompanying business guidance [41]:

- Amazon **retained children's Alexa voice recordings indefinitely** unless a parent requested deletion, which the Commission held violated § 312.10.
- When parents did request deletion, Amazon deleted files in some databases but **retained transcripts elsewhere**, linked to persistent identifiers tied to the child's account. That partial deletion is itself a violation of the parent's rights under § 312.6.
- Amazon **used the unlawfully retained recordings to improve its speech recognition**, on the reasoning that children's speech patterns differ from adults' and make a valuable training set.
- The order prohibits using such data to create or improve any data product, requires deletion of inactive child accounts, and requires notification of users.
- As a separate FTC Act theory: "for at least a year, Amazon gave 30,000 employees access to Alexa users' voice recordings — even though many of those staffers had no business need for the files" [41]. More than 800,000 children interacted directly with Alexa using their own profile [41].

Three lessons, and the third is the one that is easiest to miss. First, **the training justification does not survive the rule.** Second, **partial deletion is deletion in name only** — a transcript removed from the visible deposit file but retained in a round log, a replay buffer or an append-only ledger is the same violation, and the design must make "deleted" mean deleted from every store. Third, **who can read the data is itself the harm.** Thirty thousand employees with no business need is not a retention problem; it is an access problem, and it is the problem that a teacher or parent reading a child's transcript file creates.

### 4.3 The UK Children's Code

The Age Appropriate Design Code is a statutory code under section 123 of the Data Protection Act 2018. It was laid before Parliament on 11 June 2020, issued on 12 August 2020, and came into force on 2 September 2020, with a twelve-month transition to 2 September 2021 [42].

Its scope is worth quoting precisely, because it determines whether it applies at all: "This code is for providers of information society services (ISS). It applies to you if you provide online products or services (including apps, programs, websites, games or community environments, and connected toys or devices with or without a screen) **that process personal data and are likely to be accessed by children** in the UK. **It is not only for services aimed at children**" [42].

The fifteen standards are [42]:

1. Best interests of the child
2. Data protection impact assessments
3. Age appropriate application
4. Transparency
5. Detrimental use of data
6. Policies and community standards
7. Default settings — high privacy by default
8. Data minimisation
9. Data sharing
10. Geolocation off by default
11. Parental controls
12. Profiling off by default
13. No nudge techniques
14. Connected toys and devices
15. Online tools to exercise rights

And the instruction on partial compliance: "You must implement all of them, to the extent they are relevant to your service, in order to demonstrate your conformance to the code" [42].

The standards most directly applicable here are 7, 8, 10, 12 and 13: high-privacy defaults, data minimisation, geolocation off, profiling off, and no nudge techniques. Standard 5 — detrimental use of data — is the hook for a product whose entire value proposition depends on a child's speech being used to change a game: the use must not be detrimental, and minimisation means the transcript that survives is the shortest one that serves the purpose.

### 4.4 Deployment as the trigger for coverage

Both regimes are triggered by what the service actually does, not by what the design document intends.

COPPA binds operators of commercial websites and online services directed to children under 13, or with actual knowledge that they are collecting personal information from a child under 13 [40]. The Children's Code applies to information society services that process personal data and are likely to be accessed by children [42].

A build in which the microphone buffer is written to no disk, the recogniser is a local process, the transcript is held in a local file, and no personal information leaves the device is therefore in a materially different legal position from the same build with a cloud recogniser — and the design document already says this, as risk R6, and already names the enforcement mechanism: the recognition service's address is a config value defaulting to localhost, and a non-local address is refused without explicit parent opt-in.

The external evidence supports both halves of that. The rule text makes deletion immediate and non-disclosed [35]; the enforcement action makes partial deletion and internal access enforceable [40][41]; the Code makes high-privacy defaults and minimisation a conformity requirement rather than a preference [42]. **The privacy claim in §7 is only as true as the build.** Every one of those consequences is a property of a specific address, a specific file and a specific process, which is exactly the kind of thing that survives a code review and fails an audit.

### 4.5 Parent and teacher surfaces

What a teacher actually needs is known from StudioK, which was built for that purpose: monitor progress, **see when students are stuck or need help**, and give specific feedback [13]. Bloxels EDU provides a class library and a teacher surface for reviewing student work, with parental permission required before a child under thirteen can publish to the public wall [14]. Minecraft Education's parent guide covers classroom management, progress monitoring and assessment tools [45].

Roblox's parental controls are the most complete commercial example of the parent surface, and their defaults encode the same conclusions the Code reaches [43][44]: an age check is required before any chat can be used; parents can turn chat off entirely for users under 13; voice chat with friends is **off for all users under 13**, with parents able to enable it for trusted friends aged 9 to 12; trusted connections are restricted by age band; parents can view and report a child's connections under 13; and screen-time and spending limits are available.

Measured against those, the sandbox's proposed surfaces are already substantially aligned. The parent card proposes block counts tried, adopted and refused, stability, play time and unlocks — counts, not transcripts, with nothing the child said unless retention is explicitly toggled. The teacher mode proposes a metrics grid, the proposal, the sealed prediction, per-seed deltas and the verdict, with three buttons. The external prior art supports this shape and adds two things it does not have: the refusal count as a first-class metric (§5.1, L5), and an explicit route for a parent to review and delete (§4.6).

### 4.6 Practices to avoid

The external record is specific enough to list what not to do.

- **Retaining children's audio or transcripts, or partially deleting them.** Deleted in one store and retained in another is the Amazon violation [40][41].
- **Using children's data to train models.** AI training is never integral, so it needs separate consent that a children's product should not be seeking [36][39]; the FTC has said plainly that this is not an acceptable justification for retention [38][40].
- **Human moderation of a child's unpublished private content.** This is what made AI Dungeon's 2021 collapse, and the resolution was to stop, permanently [20].
- **Punishing the child for an utterance.** AI Dungeon's algorithmic banning of innocent players was a distinct failure from the filtering itself [20]. The sandbox's no-punishment rule is correct and now has direct support.
- **Free-form model output as the game's dialogue for a child.** Eurogamer's judgment on NEO NPC is that unimpeded conversation becomes decoration [23].
- **A refusal with no way forward.** The walls approach ends with a message, some routes to continue, and a report path [21]; *Plan Be*'s rule is to make speech failure into a feature [33].
- **Publishing a child's work by default, or locking it in.** Bloxels requires parental permission for under-thirteen publishing [14]; Dreams' inability to export [10][11] is the canonical lock-in failure.
- **Nudge techniques and default-on anything.** Directly prohibited by the Code [42].
- **Auto-executed proposals.** Roblox's documentation is explicit that plans never run automatically and never self-approve [32].

---

## 5. Lessons for the sandbox design

### 5.1 Ten lessons, each anchored to a design section

Each lesson names the section of `docs/design/sandbox-on-otherhere.md` it applies to, and a verdict: **agree** (the evidence supports the design as written), **change** (the evidence says something different), or **add** (the evidence implies something the design does not yet contain).

**L1 — Constrain the input, not the output. The 48-block dictionary is the right architecture.**
*Verdict: agree. Anchors to §3 (the Vibe Dictionary) and §7.*
AI Dungeon's entire arc is the story of a system that began with "you can do anything" and tried to constrain it afterwards, through false positives, algorithmic bans and human review of private stories [20]. Its conclusion was to build walls instead of filters — walls with no punishment and always a way forward [21]. Ubisoft arrived at the same concept independently, in the same vocabulary, with characters "positioned within 'walls' set by humans" and only about twenty pre-set solutions [23]. The sandbox's Vibe Dictionary is a stricter wall than either, because it is on the input side: a child cannot express a change outside the vocabulary, so there is no output that needs filtering. §3's design should be defended in exactly these terms.

**L2 — The Glitch Meter's primary function is making the test safe to run, not explaining it afterwards.**
*Verdict: change. Anchors to §5 (the Glitch Meter) and §1 (the first five minutes).*
The two children in the Minecraft Education case study did not abandon code because it was hard. They abandoned it because running it could destroy the project they had made, and one refused even when offered a copy of his world before testing [16]. The successful Minecraft Education classroom, by contrast, produced children who attributed errors to flawed code rather than to autonomous intent [17]. The implication is that the sealed prediction's job is to make the child willing to press the button — and that willingness is the fragile asset. If the prediction is wrong too often, the child learns that running the test is dangerous, and the outcome is the same abandonment, arrived at for a different reason. §5 should say plainly that the prediction is a trust device, and that trust is the thing being spent.

**L3 — A refusal must never end the interaction and must never carry a consequence.**
*Verdict: agree, with one addition. Anchors to §7 (the fail-state) and §3.*
The sandbox already specifies that a refusal offers buttons rather than a wall, and that the child is never punished. Both are correct, and both are now supported by the only prior art that got this wrong at scale [20]. The addition AI Dungeon made and the sandbox has not: a **bypass allowance and a report route**. AI Dungeon's design grants five lifetime bypasses of thirty minutes and a path for the user to report a mistaken block, which improves the system [21]; *Plan Be* advises making speech failure into a feature with several ways to continue [33]. §7's fail-state should offer a bounded number of override attempts and a way for the child to flag a wrong prediction, with the flag reaching the teacher surface.

**L4 — Latency is the binding constraint, and it must sit outside the round.**
*Verdict: agree. Anchors to §2 (the core loop and what each step may cost) and §4 (the rival).*
The published numbers are not encouraging for anything model-driven in a real-time loop: a world model at one frame per second [27], a real-time variant at ten-plus frames per second with 0.9 seconds of memory [26], generative agents that hallucinate in 1.3% of 453 responses and forget across a short horizon [25], and an NPC conversation with lag before replies and vocal stutters [24]. The design's instinct to spend the latency budget on the deterministic simulation and keep the model off the critical path is the right one, and §2 should say that the model's contribution is to *choose a block and a value*, with the simulation owning the consequence. The rival, per L1 and the Eurogamer critique [23], must not be the place where the round's time goes.

**L5 — The thumb should be computed as endorsement rate, per block, and reported as a metric.**
*Verdict: add. Anchors to §5 (the thumb) and §7 (the teacher mode).*
Mario Maker's enjoyment measure was behavioural — the proportion of players who attempted a level who also endorsed it — computed across 32.6 million attempts [7]. The sandbox's thumb is the same instrument and should be reported the same way: endorsement rate per dictionary block, so that a block children repeatedly try and rarely endorse is visible as a bad block rather than a popular one. The current design treats the thumb as a gate on the Glitch Meter [§5]; the prior art says it should also be a measurement surface of its own.

**L6 — The 48-block vocabulary is the largest unmeasured risk in the design, and no prior art relieves it.**
*Verdict: agree with the exit already named. Anchors to §3, Open Question 6, and §9 (risks).*
There is no published measurement of per-block accuracy for a closed voice-choice gate. The nearest comparable surface is ChatScratch, which achieved a code retention rate of about 0.74 and an extension rate of about 0.54 on a free-generation surface in a forgiving environment with 24 beginner children [5]. Roblox's own documentation lists what goes wrong when the proposal space is open and the request is underspecified [32]. Open Question 6's exit is the right one and should be treated as a gate on the dictionary's contents rather than as a question to be answered later: score each block on a labelled utterance set, and any block the model cannot reach reliably becomes a button rather than a word. The design should also be explicit that 48 is not a safe number — it is a number that has to be earned per block.

**L7 — The microphone design has a specific legal permission, with four conditions, and the conditions are stricter than "we don't store it."**
*Verdict: agree, with a sharpening. Anchors to §7 (the voice never leaves the machine).*
COPPA § 312.5(c)(9) permits collecting an audio file containing a child's voice, and no other personal information, solely to respond to the child's specific request, with no other use, no disclosure, and **immediate deletion** after the response [35]. Note carefully what it does not cover: a transcript is personal information in its own right, so it is not covered by the exception, and a file retained in a round log, a replay buffer or an append-only ledger is not deleted. The FTC's Amazon action makes both points enforceable, and adds the access dimension — thirty thousand employees with no business need was treated as a harm in itself [40][41]. §7 should state the claim in the form the rule uses: the audio buffer is never written to disk, the transcript lives in exactly one local file, and deletion removes it from every store including the ledger.

**L8 — The parent-facing claims must be stated per deployment, and the build must enforce them.**
*Verdict: agree. Anchors to §7 (risk R6) and §9 (Open Question 8).*
The Children's Code applies to information society services that process personal data and are likely to be accessed by children, and it says explicitly that it is "not only for services aimed at children" [42]. COPPA reaches services with actual knowledge that they collect personal information from under-thirteen-year-olds [40]. Coverage therefore follows the build, and the enforcement mechanism the design already names — a localhost default with a non-local address refused without parent opt-in — is the right one. What should change is the framing of the claim: the parent card should say what this deployment does, not what the product does in general, because a cloud-recogniser variant of the same product is a different legal object. Open Question 8 should be answered in terms of which deployments are in scope.

**L9 — The off-vocabulary request needs a designed landing place, because a closed vocabulary without one reads as a wall.**
*Verdict: add. Anchors to §3 (the dictionary) and §7 (the request pile).*
Every creation tool in Section 1 fails at the same point: the child's idea outgrows the tool. Scratch's bound is asset creation [5]; Dreams' bound is complexity and lock-in [10][11]; Bloxels' bound is having to program all behaviour [15]; Code Builder's bound is that children "lost an understanding of what they were doing" as the abstraction grew [17]; Mario Maker's bound is a difficulty the creator cannot calibrate [7][8]. The sandbox's answer — queue the off-vocabulary request, tell the child it was heard, and keep it visible — has no direct prior art, and it should be described as an invention rather than borrowed. The nearest established pattern is AI Dungeon's ending: a message, some ways to continue, and a route to report [21]; and Roblox's accept-then-playtest loop, in which the child is never shown a dead end [31][32]. The request pile is the difference between a wall and a boundary, and it deserves to be built as a first-class feature with its own teacher-visible counter.

**L10 — Generation a child cannot read is not scaffolding, and the prediction sequence is the antidote.**
*Verdict: change. Anchors to §1 (the first five minutes) and §5.*
In the MindScratch classroom study, only 13 of 24 children using plain Scratch met the teacher's learning objectives, and educators judged the AI-assisted generated code difficult for beginners to comprehend [6]. The alternative that worked — the Minecraft Education classroom — produced children who read agents as rule-based systems and attributed failures to the code rather than to the agent [17]. The prediction-then-test-then-verdict sequence is a mechanism for keeping the child inside the causal chain, and there is real learning-science support for the ordering: asking learners to predict an outcome before observing it produces larger learning gains than asking them to evaluate retrospectively, across geography and soccer-memory tasks [47], in children [46], and in the expectancy-violation case where the prediction is wrong [48].

Two qualifications belong in the same breath. The effect does not require enjoyment to be sacrificed — an educational game that added prediction to its loop reported that learning and enjoyment both improved [46]. And the child's *stated* prediction is not a measurement of belief: children are systematically overoptimistic when asked to predict explicitly, and that optimism bias resists both practice and feedback. So the prediction should be treated as a prompt for the conversation and a moment to make the reasoning explicit, not as a calibrated estimate to score.

### 5.2 Three changes to the design document

**1. Drop any place where the model is asked to produce free-form game content or code. (Change, §3 and §4.)**
Every failure in Section 2 came from the same move — letting the model generate where the system should have chosen. AI Dungeon's base model produced unsafe text that it had not been asked for, and players were banned for it [20]. Neo NPC's free conversation was judged to be decoration because it did not change the game [23]. Roblox's documentation publishes a table of what goes wrong when an underspecified request meets an open generation surface, and instructs users to check every result [32]. The sandbox's own typed change set makes this unnecessary: the model's only job is to choose one block from the dictionary and one value within it, and the Bolted rival's dialogue is written, not generated. This is also the cheapest possible way to keep latency off the critical path, per L4.

**2. Add two metrics to the teacher mode: endorsement rate per dictionary block, and the refused-request count. (Add, §7.)**
The refusal count is currently only a trigger for the fail-state. It should be visible as a measure in its own right, because a high refusal count is the leading indicator of a dictionary that has drifted from how children actually talk — which is exactly the failure the Sandbox's Open Question 6 exit is meant to catch, but caught later than it needs to be. The endorsement rate per block is the Mario Maker measure [7], and it is what turns "the child liked that" into "that block works for children and these six do not." Both belong beside the existing metrics grid, the proposal, the sealed prediction, the per-seed deltas and the verdict.

**3. Add a prediction-trust measure, with a demotion trigger. (Change, §5.)**
Given L2, the sandbox should track the rate at which the child's stated prediction and the sealed prediction's outcome agree, and treat a falling rate as a fault in the Glitch Meter rather than as a fact about the child. When trust falls below a threshold, the Glitch Meter should be demoted to a post-hoc explanation — the child sees the delta, but is no longer asked to predict — and the failure should be reported to the teacher surface. The reason to build this now is that trust loss is invisible until it is total: a child who has quietly stopped pressing the button looks identical to a child who is bored, and the failure is only visible in the metrics after it has cost the whole session. This metric is the sandbox's own invention, and it is the one place where the design's most novel element needs an instrument of its own.

---

## References

[1] Maloney, Peppler, Resnick, Rusk. *Programming by Choice: Urban Youth Learning Programming with Scratch.* SIGCSE 2008. https://eric.ed.gov/?id=ED521157
[2] Resnick, Silver, Maloney, Thrun, Rusk, Kay, Kelleher. *Programming by Choice: Urban Youth Learning Programming with Scratch.* Communications of the ACM 52(3), 2009. https://web.media.mit.edu/~mres/papers/Scratch-CACM-final.pdf
[3] Aivaloglou, Hermans. *Computational Thinking Through Code Reuse.* ICER 2016. https://aivaloglou.org/wp-content/uploads/2019/01/p53-aivaloglou.pdf
[4] Hermans, Aivaloglou. *Program Smells in Code Reuse: A Study of 61 Novice Scratch Programmers.* ICPC 2016. https://aivaloglou.org/wp-content/uploads/2019/01/07503706.pdf
[5] *ChatScratch: A Human-AI Collaborative Tool for Teaching Computational Thinking to Children.* CHI 2024. https://arxiv.org/abs/2402.04975
[6] *MindScratch: A Visual Programming Support Tool for Classroom Learning Based on Multimodal Generative AI.* arXiv:2412.09001. https://arxiv.org/html/2412.09001v1
[7] *Leveling up fun: learning progress, expectations, and success influence enjoyment in video games.* Scientific Reports 15:34153 (2025). https://pmc.ncbi.nlm.nih.gov/articles/PMC12488971/
[8] *Super Mario Maker 2: Level design, difficulty and player success.* arXiv:2507.21078. https://arxiv.org/html/2507.21078v1
[9] *Playful Work and Laborious Play in Super Mario Maker.* Dagstuhl LIPIcs, CSYE 2019. https://www.degruyterbrill.com/document/doi/10.14361/dcs-2019-0207/html
[10] Media Molecule. *Dreams support update.* https://docs.indreams.me/en/whats-happening/news/dreams-support-update
[11] Polygon. *Dreams is ending its live service.* https://www.polygon.com/23678503/dreams-end-of-live-service-server-changes/
[12] Microsoft Research. *Kodu — Visual Programming Language for creating Xbox games.* https://www.microsoft.com/en-us/research/project/kodu/
[13] University of Wisconsin–Madison. *UW–Madison partnership creates educational game development tools* (StudioK). https://news.wisc.edu/uw-madison-partnership-creates-educational-game-development-tools/
[14] Bloxels. *Bloxels EDU: Frequently Asked Questions.* https://www.bloxels.com/education/bloxels-edu-faq
[15] Bloxels. *Try Bloxels.* https://www.bloxels.com/try-bloxels
[16] *Examining the Realities and Nuances of 'Low-Stakes' Interest-Driven Learning Environments.* RESPECT 2022 / NSF PAR. https://par.nsf.gov/servlets/purl/10392969
[17] Meiningdias, Suryaman, Wahyudin. *Exploring the Use of Minecraft Education Edition to Support Computational Thinking, Coding Skills, and AI Literacy: A Case Study in an Indonesian Madrasah Informatics Classroom.* Akademika 14(02), 2025, 151–171. https://jurnal.uia.ac.id/akademika/article/view/5351 — *the publisher's host refused connections from the verification environment; the article's full text was retrieved via the search index on the same date.*
[18] Karsenti, Bugmann, Gros. *Transforming Education with Minecraft? Results of an exploratory study conducted with 118 elementary-school students.* CRIFPE, 2017. https://education.minecraft.net/content/dam/education-edition/software-downloads/Minecraft_Research_Report_Karsenti-Bugmann_2017.pdf
[19] Kutay, Oner. *Coding with Minecraft: A Focus on Computational Thinking.* ACM TOCE, 2022. https://eric.ed.gov/?id=EJ1358816
[20] AI Dungeon. *OpenAI and Filters.* https://help.aidungeon.com/faq/openai-and-filters
[21] Latitude. *The Walls Approach.* https://latitude.io/blog/the-walls-approach
[22] Ubisoft. *How Ubisoft's New Generative AI Prototype Changes the Nature of NPCs.* https://news.ubisoft.com/en-us/article/5qXdxhshJBXoanFZApdG3L/how-ubisofts-new-generative-ai-prototype-changes-the-nature-of-npcs
[23] Eurogamer. *Ubisoft unveils generative AI "NEO NPCs", and the spirit of Peter Molyneux's Milo lives on.* https://www.eurogamer.net/ubisoft-unveils-generative-ai-neo-npcs-and-the-spirit-of-peter-molyneuxs-milo-lives-on
[24] The Verge (Sean Hollister). *Ubisoft let me actually speak with its new AI-powered video game NPCs.* https://www.theverge.com/2024/3/19/24105748/nvidia-neo-npc-prototypes-gdc-2024
[25] Park, O'Brien, Cai, Morris, Liang, McCallum. *Generative Agents: Interactive Simulacra of Human Behavior.* arXiv:2304.03442. https://arxiv.org/abs/2304.03442
[26] Microsoft Research. *WHAMM! Real-time world modelling of interactive environments.* April 2025. https://www.microsoft.com/en-us/research/articles/whamm-real-time-world-modelling-of-interactive-environments/
[27] Kanervisto et al. *World and human action models towards gameplay ideation.* Nature 638, 656–663 (2025). https://www.nature.com/articles/s41586-025-08600-3
[28] Wang, Hu, Li, Chen, Zhu. *Voyager: An Open-Ended Embodied Agent with Large Language Models.* arXiv:2305.16291. https://arxiv.org/abs/2305.16291
[29] GitHub Docs. *Responsible use of GitHub Copilot features.* https://docs.github.com/en/copilot/responsible-use-of-github-copilot-features
[30] Roblox Creator Hub. *Assistant for Studio.* https://create.roblox.com/docs/assistant/guide
[31] Roblox Creator Hub. *Build your first game with Assistant.* https://create.roblox.com/docs/ai/build-with-assistant
[32] Roblox Creator Hub. *AI Assistant* (curriculum tutorial). https://create.roblox.com/docs/tutorials/curriculums/building/code-with-assistant
[33] *Speech Recognition and Storytelling in Plan Be.* Game Developer (GDC). https://www.gamedeveloper.com/design/speech-recognition-and-storytelling-in-plan-be
[34] Neocortex. *On-device voice commands for game mods.* https://neocortex.link/blog/on-device-voice-commands-for-game-mods — *vendor documentation, primary for design intent only.*
[35] eCFR. *16 CFR 312.5 — Verification of parental consent.* https://www.ecfr.gov/current/title-16/chapter-I/subchapter-C/part-312/subpart-C/section-312.5
[36] Federal Trade Commission. *16 CFR Part 312: COPPA — Final Rule Amendments.* https://www.ftc.gov/legal-library/browse/federal-register-notices/16-cfr-part-312-coppa-final-rule-amendments
[37] Federal Trade Commission. *COPPA Rule Final Rule (16 CFR Part 312), Federal Register, 22 April 2025.* https://www.ftc.gov/system/files/ftc_gov/pdf/coppa_sbp_1.16_0.pdf
[38] Federal Trade Commission. *Statement of Chair Lina M. Khan Re: COPPA Amendments*, 16 January 2025. https://www.ftc.gov/system/files/ftc_gov/pdf/statement-of-chair-lina-m-khan-re-coppa-amendments-1-16-2025.pdf
[39] Latham & Watkins. *FTC Publishes Updates to COPPA Rule.* https://www.lw.com/en/insights/ftc-publishes-updates-to-coppa-rule
[40] Federal Trade Commission and Department of Justice. *FTC and DOJ Charge Amazon with Violating Children's Privacy Law by Keeping Kids' Alexa Voice Recordings Forever and Undermining Parents' Deletion Requests*, 31 May 2023. https://www.ftc.gov/news-events/news/press-releases/2023/05/ftc-doj-charge-amazon-violating-childrens-privacy-law-keeping-kids-alexa-voice-recordings-forever
[41] Federal Trade Commission. *Out of the mouths of babes? FTC says Amazon kept kids' Alexa voice data forever — even after parents ordered deletion*, 31 May 2023. https://www.ftc.gov/business-guidance/blog/2023/05/out-mouths-babes-ftc-says-amazon-kept-kids-alexa-voice-data-forever-even-after-parents-ordered
[42] Information Commissioner's Office. *Age appropriate design: a code of practice for online services — About this code.* https://ico.org.uk/for-organisations/uk-gdpr-guidance-and-resources/childrens-information/childrens-code-guidance-and-resources/age-appropriate-design-a-code-of-practice-for-online-services/about-this-code/
[43] Roblox. *Parental controls.* https://about.roblox.com/parental-controls
[44] Roblox. *Safety FAQ.* https://about.roblox.com/safety-faqs
[45] Minecraft Education. *Parent Resource Guide.* https://education.minecraft.net/content/dam/education-edition/software-downloads/Minecraft_Education_Parent_Resource_Guide.pdf
[46] Colantonio, Heuser, Schymanski, Bzdok, Rittmann, Möhrle, Brod. *Predicting Learning: Understanding the Role of Executive Functions in the Benefits of Prediction for Children's Learning.* Topics in Cognitive Science, 2024. https://www.uni-trier.de/fileadmin/fb1/prof/PSY/SUK/Topics_in_Cognitive_Science_-_2024_-_Colantonio_-_Predicting_Learning_Understanding_the_Role_of_Executive_Functions_in.pdf
[47] Brod, Hasselhorn, Bunge. *When generating a prediction boosts learning: The element of surprise.* 2018. https://www.pedocs.de/volltexte/2020/16102/pdf/Brod_When_generating_a_prediction_boosts_learning_2018_A.pdf
[48] Brod. *Explicitly predicting outcomes enhances learning of expectancy-violating information.* 2022. https://www.uni-trier.de/fileadmin/fb1/prof/PSY/SUK/Brod_2022.pdf
