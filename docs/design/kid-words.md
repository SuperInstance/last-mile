# Kid words: the phrasebook pass

**Status:** done as a brief (the phrasebook pass), then again for the
variants. No code changed.
**Date:** 2026-10-30
**Scope:** `assets/harness/default/phrasebook.toml`, then the nine variants
under `assets/harness/variants/*/phrasebook.toml` (see Variants below). The audience is kids
about 8-12 (`docs/design/sandbox-on-otherhere.md`); the tone is paintball, not
war: nobody dies, nobody is hurt, it's paint.

## What changed and why

28 template lines changed. Every key, every `channel`, and both placeholders
(`{subject}`, `{direction}`) are exactly as they were. An urgent line stayed
urgent and a calm line stayed calm -- only the wording moved. The meaning kept:
a warn still warns, a report still reports, an order still orders.

| Old line | New line | Why |
| --- | --- | --- |
| `hold the line` | `hold your spot` | military idiom |
| `do not let {subject} pass` | `don't let {subject} past` | stiff formal "do not" |
| `understood` | `got it` | formal, three syllables |
| `do not take {subject}` | `don't take {subject}` | stiff formal "do not" |
| `say again` | `say that again` | radio-speak |
| `is {subject} clear` | `is {subject} safe` | military "clear" (= secured) |
| `reporting` | `quick update` | radio-speak |
| `that is the situation` | `just so you know` | military briefing voice |
| `we hold {subject}` | `we have {subject}` | "hold" as army-speak for "have" |
| `shall we push` | `should we go for it` | archaic "shall"; jargon "push" |
| `do we hold {subject}` | `do we have {subject}` | "hold" as army-speak for "have" |
| `who holds {subject}` | `who has {subject}` | "hold" as army-speak for "have" |
| `give me the situation` | `what's going on` | military briefing voice |
| `who holds {subject}` | `who has {subject}` | "hold" as army-speak for "have" |
| `the order is to move` | `time to move` | passive wording, no actor |
| `the order is {subject}` | `the plan is {subject}` | terse briefing voice |
| `heard` | `got it` | radio-speak |
| `copy` | `will do` | radio-speak |
| `danger` | `careful` | war-poster word; same urgency, friendlier |
| `danger at {subject}` | `careful near {subject}` | war-poster word |
| `here is the situation` | `here's what's happening` | military briefing voice |
| `the situation is this` | `this is what's going on` | military briefing voice |
| `we hold {subject}` | `we have {subject}` | "hold" as army-speak for "have" |
| `{subject} is taken` | `we got {subject}` | passive voice, no actor |
| `squad, on me` | `squad, follow me` | military "on me" |
| `enemy at {subject}` | `{subject} is close` | war word as a label |
| `the situation is this` | `here's what's up` | military briefing voice |
| `we hold {subject}` | `we have {subject}` | "hold" as army-speak for "have" |

Untouched on purpose: `move up now`, `take it`, `take {subject} now`,
`move to {subject}`, `head {direction} now`, `go {direction}`, `yes`, `on it`,
`stop`, `look out`, `watch {subject}`, `watch out for {subject}`,
`{subject} is coming`, `where`, `where is {subject}`, `leave {subject}`,
`should we move`, `should we take {subject}`, `did you hear that`,
`are you with me`, `do you see them`, `is it safe`, `do you see {subject}`,
`where is {subject}`, `what is happening`, `what do you know`,
`anyone seen {subject}`, `what is at {subject}`, `what happened`,
`what happened at {subject}`, `we are moving`, `we are taking {subject}`,
`i wonder`, `anyone know`, `they are coming`, `they are at {subject}`,
`everyone, listen`, `squad, {subject} is ours`, `everyone, take {subject}`,
`with you`, `still here`, `they are here`, `anyone seen them`,
`where are you`, `who is at {subject}`, `here is where we are`,
`{subject} is yours`, `{subject} is ours` -- a ten-year-old gets each of these
at once, and they already read paintball, not war.

## Words avoided, and what to use instead

For every future line in the phrasebook (and any new `Bench.*` / `Bolt.*`
family):

| Avoid | Use instead | Note |
| --- | --- | --- |
| contact, engaged | they are close, they see you | the brief's example jargon |
| flank, flanking | go around, sneak around | the brief's example jargon |
| suppress(ing fire) | keep them busy, cover them | the brief's example jargon |
| exfil, extract, RTB | fall back, get out, come back | the brief's example jargon |
| copy, heard, roger, wilco | got it, will do | radio-speak |
| say again, repeat | say that again | radio-speak |
| reporting, sitrep | quick update, here's what's up | radio-speak |
| situation | what's happening, what's going on | briefing voice |
| clear (= secured) | safe, free, open | military sense of "clear" |
| push (attack), advance | go for it, move up | gamer/army jargon |
| shall | should, can | archaic |
| on me | follow me | military formation call |
| hold the line | hold your spot | the idiom, not the verb: "hold your spot" keeps the plain verb |
| we hold X (= we own it) | we have X, we got X | plain possessive |
| the order is ... | time to ..., the plan is ... | briefing voice |
| enemy (as a label) | they, the other team, or just `{subject}` | paintball, not war; `{subject}` already fills in as "enemy 4", so do not double the word |
| danger | careful, look out | same urgency, friendlier |
| passive voice (`X is taken`) | active (`we got X`) | name who did it |

Style rules the new lines follow, for future lines:

1. A breath: about five words or fewer, one idea.
2. Active voice: somebody does the thing.
3. Everyday words a ten-year-old uses at school; contractions are fine
   (`don't`, `what's`, `here's`) -- the loader takes any word without a digit
   in it, and each whitespace-separated word is one beat of the score.
4. No digits in a template, ever: the numbers are the gates' and come in
   through `{subject}` and `{direction}` (the loader refuses a numbered
   template, D37).
5. Keep the line's temperature: urgent stays urgent, calm stays calm. The
   Vox Score marks and the channel (`radio`, `shout`) carry the volume; the
   words carry the meaning.

## Facts sentences that should change (a later job, not this one)

`crates/lm-agents/src/facts.rs` was not touched. Its sentences are read by
the decision models, not by kids, so changing them is a behaviour change with
pinned tests and deserves its own job with its own measurements. In priority
order, the sentences a kid-facing summary would inherit:

1. `"...can see you (clear line of sight)"` / `"...cannot see you (no clear
   line of sight)"` -- the military sense of "clear" again. The plain words
   "can see you" / "cannot see you" already say it; the parenthetical is
   removable. Pinned by `facts_state_numbers_not_grids` and
   `holder_and_stun_wording`.
2. `"the nearest beacon your squad does not hold"` (twice per sentence set) --
   long and doubled-negative; something like `"the closest beacon that is
   free"` is shorter and says the same. Pinned in several tests and in the
   doctrine's own wording, so the rule and the facts must move together.
3. `"held by enemy squad 1"` and `"Enemy 4, last seen ..."` -- the war label.
   Model-facing vocabulary ("enemy") matches the gates' doctrine, so this
   only changes if the whole doctrine renames its subjects, not piecemeal.
   Flag for the same later job.
4. `"is 1 cells away"` -- a pluralisation miss ("1 cells"). Deliberately
   pinned by `facts_state_numbers_not_grids`; a later job can fix the wording
   and the pin together.
5. `"unheld"` -- not a word a person says. `"free"` or `"nobody has it"` reads
   better to a kid and no worse to a model. Pinned by
   `unreachable_beacon_states_no_path` and `holder_and_stun_wording`.

## Variants

The nine doctrine variants -- `advance`, `contest`, `hold`, `raid`, `scout`,
`shared-centre`, `shared-free`, `shared-ghost`, `shared-ghost-fixed` -- each
carried the old default wording verbatim: at the parent of the default's pass
every variant `phrasebook.toml` was byte-identical to the old default. So
every variant got exactly the default's pass: **28 template lines changed per
variant** (252 across the nine), each to the default's new line exactly, so
the voices stay consistent across variants. No key, `channel`, placeholder or
comment moved, and **no line needed its own wording** -- no variant carries a
template the default did not have.

## Notes for whoever picks this up next

- The nine doctrine variants under `assets/harness/variants/*/phrasebook.toml`
  now carry the same pass as the default (see Variants above); they stay test
  doctrine, and the shared-* variants' `facts.toml` files are untouched.
- The golden is untouched by wording: told-facts and the state hash key on the
  utterance header (act, intent, subject), never on the template's words. The
  words live in the round log as events. `lm golden` stays OK 3/3.
- The realizer fallback and the wording bank key on plan and facts, not on
  template text, so no cache is invalidated by this pass.
