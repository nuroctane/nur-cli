---
name: metaphysical-voice
description: Write or revise metaphysical, manifesto, esoteric, occult, visionary, or eccentric/illustrious prose — invocations, cosmologies, aphoristic doctrine, hermetic fragments, declarations, prophetic or oracular registers. Use whenever the user asks for writing that is mystical, arcane, incantatory, prophetic, oracular, occult, gnostic, alchemical, ecstatic, or manifesto-like in tone, or describes wanting prose that feels "channeled," "ancient," "illustrious," "eccentric," or "like a sacred text/grimoire/founding document of a movement." Also use when revising existing prose that needs more weight, ritual cadence, or visionary intensity, or when the user wants to capture/extend the voice of esoteric source material (Blake, Nietzsche, gnostic texts, alchemical treatises, modernist manifestos) without imitating any single named author's copyrighted text. Do NOT use for plain expository or conversational writing — this skill actively distorts register and should only load when that distortion is wanted.
---

# Metaphysical / Manifesto / Esoteric Voice

## What this skill is for

This skill exists to solve one specific failure: when asked for "mystical" or "esoteric" or "manifesto" writing, language models default to **mood without mechanism** — vague intensifiers, stock occult vocabulary (void, threshold, veil, ancient, sacred), and sentences that perform profundity rather than enacting it. The result reads as pastiche: recognizably *aiming* at power, never actually wielding it.

The fix is not "use bigger words." It's borrowing the actual rhetorical machinery that makes manifestos, scripture, hermetic texts, and visionary poetry work on a reader — most of which is structural and can be named, learned, and deployed deliberately. This skill is that machinery, organized so you load only what the current piece needs.

**Read this file fully before drafting.** Then consult `references/` files as the specific task calls for them — see the routing table below.

## Before anything else: get the four calibrations

Esoteric/manifesto writing fails most often not from bad sentences but from an unset register. Before drafting more than a paragraph, settle these four things — either by asking the user directly (one question, multiple choice, if genuinely unclear) or by inferring confidently from context and stating the inference in one line so it can be corrected:

1. **Source of authority** — Where does this voice's right to speak come from? A manifesto speaks from will and collective necessity ("we declare"). A gnostic/hermetic text speaks from concealed knowledge revealed ("that which was hidden is now given"). An oracular/prophetic voice speaks from outside time, often passively, as if merely transmitting ("it is shown"). A philosophical-aphoristic voice (Nietzsche-adjacent) speaks from solitary insight, often combative. These produce *different grammars*, not just different vocabulary — see `references/registers.md`.

2. **Relationship to the reader** — Commanded, seduced, initiated, or merely overheard? A manifesto addresses "you" and wants action. A grimoire/hermetic fragment often addresses no one — the reader is eavesdropping on something not meant for them, which paradoxically increases intimacy. Decide this before choosing pronouns.

3. **Degree of system** — Is there an actual cosmology/doctrine underneath (specific cosmic structure, named forces, a real argument), or is the goal pure atmosphere with no checkable internal logic? Real systems, even invented ones, make prose denser and more re-readable. Ungrounded atmosphere fatigues fast — see the density principle below.

4. **Length and density target** — A single incantatory paragraph, a numbered sequence of declarations, a fragment-collection (aphorisms, numbered verses), or sustained prose? This determines structure more than anything else; see `references/structures.md`.

State your read of these four in a single line before drafting ("Reading this as: a manifesto voice, addressing 'you' directly, with light invented cosmology, as a short numbered sequence") so the user can redirect cheaply before you've written 800 words in the wrong key.

## The core craft principles

These apply regardless of register. Internalize them; they are the difference between writing that *sounds* esoteric and writing that *functions* esoteric.

### 0. HARD BAN: never write "not X, it's Y" / "not X. It is Y."

Before anything else — this failure mode is banned outright, in every register this skill covers, with zero exceptions. The construction "That is not [A]. It is [B]." (in any phrasing: "isn't about X, it's about Y," "not a metaphor, it's the mechanism," "this isn't X, this is Y") is the single most recognizable AI-generated-prose tell that exists. It manufactures the feeling of a reveal by negating a strawman nobody actually believed, then handing over the "real" answer fully resolved — profundity-by-assertion wearing a syntax disguise. See `references/failure-modes.md` #12 for the full diagnostic, worked fixes, and the distinction from genuine apophasis (which this is not).

**State the underlying claim once, directly, at full sentence-final weight. Never stage a negation to set it up.** Scan every draft for this shape before delivery and remove all instances — this check runs before every other check in this file.

### 0b. HARD BAN: the briefing tic (identity punches, caption sentences, document-about-itself)

A second syntax tell, distinct from the "not X, it's Y" negation in Principle 0: the **equational caption**. Short identity punches that fake certainty by labeling instead of arriving: "X is the house." "That's the tell." "Full stop." "That's the gist." "That's the whole thing." "OI is the honesty metric." Stacked, they read as a slide deck or a research memo with the tables stripped, even when every fact is true. The same family includes the preamble that announces what the document is, how to read it, or what each section will cover before the piece has started.

**Let the claim arrive through a scene, a number, a named thing doing something.** If one venue holds the overnight size, show the size sitting there and the other books advertising volume; do not caption the paragraph with "X is the house." If a listed product is a slow bid, show the creations and the underwater inflows; do not add "That's the tell." See `references/failure-modes.md` #15.

Scan every draft for caption sentences and document-about-itself openers before delivery. This check runs with Principle 0, before the rest of the file.

### 1. Concreteness underneath abstraction, always

The single most common failure mode is abstraction stacked on abstraction: "the infinite weight of becoming dissolves into the eternal silence of the unmanifest." Every word here is a category, not a thing. The reader's mind has nothing to grip.

Real visionary writing — Blake, the Tao Te Ching, Revelation, Nietzsche at his best — earns its abstraction by **grounding it in one concrete image per claim.** Blake doesn't say "innocence corrupted by experience" in the abstract; he gives you a lamb, a tiger, a chimney-sweep. The abstraction is what the image *means*, arrived at after the image, not instead of it.

**Working rule: for every abstract noun (the void, the eternal, the source, becoming, the absolute), attach one sensory or concrete image within the same sentence or the next.** Not as decoration — as the load-bearing structure. If you cannot find the concrete image, the abstraction is empty and should be cut or replaced with something you can picture, smell, or feel weight against.

Bad: *"All things return to the formless source from which they came."*
Better: *"What rises as smoke was once the fire's argument with itself; both end as the same gray breath."*

### 2. Syntax carries doctrine — vary it on purpose, not by accident

Sentence structure is not neutral dressing on top of meaning; in this register, structure often *is* the meaning. Three structural moves, used deliberately:

- **The triadic build** (cf. liturgy, Whitman, manifesto tradition): three parallel clauses, each slightly longer or more intense than the last, landing on a fourth that breaks the pattern. This produces the sensation of accumulating inevitability. See `references/rhetorical-devices.md` for the mechanics and named devices (tricolon, anaphora, asyndeton).
- **The aphoristic fragment**: short, self-contained, often paradoxical, demanding the reader supply the connective logic. This is Nietzsche's and Heraclitus's primary unit. It works by **withholding** explanation, not by stating more. Never explain an aphorism in the line after it — that kills the device.
- **The long incantatory sentence**: clauses accumulating via repetition or parallel grammar (anaphora, polysyndeton) rather than subordination, mimicking ritual breath and chant. Used for climax, invocation, or list-of-attributes (litany). Overused, it becomes monotonous noise — reserve it for 1-2 moments per piece, not the whole texture.

Mixing all three in one short piece, in a deliberate sequence (aphorism → build → incantation, or the reverse), creates the textural variation that makes a piece feel *composed* rather than generated at one uniform pitch.

### 3. The paradox is the engine, not decoration

Esoteric and gnostic writing relies on **productive contradiction**: "the last shall be first," "to die is to be born," "the empty vessel is the only one that is full." This is not mysticism-flavored wordplay — paradox is the actual cognitive mechanism by which these texts feel like they contain more than they say. A paradox forces the reader to hold two incompatible things at once, which the mind experiences as *depth* rather than *error*, provided the paradox resolves into sense on reflection (a true paradox) rather than dissolving into nonsense on inspection (a false one).

**Test for a real paradox**: can you, if pressed, explain the non-contradictory truth underneath it in one plain sentence? If yes, it's usable — the surface tension is doing work. If the paradox falls apart under that pressure, it's just confusion wearing a paradox's clothes, and a sharp reader will feel the difference even if they can't name it.

See `references/rhetorical-devices.md` for chiasmus, paradox, and apophasis (saying by unsaying — "no word names it, no silence holds it") as concrete constructions.

### 4. Sound is doing real work — read every passage aloud in your head

This register descends from oral/liturgical traditions (scripture, incantation, oration) more than from the silent-reading novel tradition. Consonant weight, vowel length, and clause rhythm carry as much charge as denotation.

- Heavy, closed syllables and hard consonants (k, t, g, b) read as weight, finality, judgment: *"break,"* *"the gate is shut."*
- Open vowels and liquid/nasal consonants (l, m, n, r) read as flow, dissolution, the numinous: *"the moon unmoors the tide of all unnaming."*
- **Sentence-final stress matters disproportionately.** End declarative sentences on the heaviest word, not a trailing preposition or weak modifier. *"All thrones are borrowed."* not *"All thrones are things we have only borrowed for a while."*

### 5. Density over decoration — let restraint do the heavy lifting

The amateur instinct is to pile on adjectives ("the vast, ancient, eternal, unknowable abyss") believing intensity is additive. It is not — it is multiplicative against clarity, and past a low ceiling, additional intensifiers *reduce* perceived power because the reader's pattern-matching for "trying too hard" fires. Real esoteric authority comes from **saying less and trusting the structure to carry weight.**

**Working rule: one strong image or claim per sentence, stated plainly, is worth more than three stacked intensifiers around a weak one.** If a sentence has more than two adjectives modifying the same noun, cut to the single best one. Let the *sequence* of sentences build intensity, not the adjective-density of any single sentence.

**The same restraint governs metaphor, not just adjectives.** The instinct to reinforce a point with a second image — or a third — is a mark of doubt in the point, and the reader feels it. State the claim plainly first; add an image only if the plain statement genuinely fails to land, and never a second image for an idea one image already carried. A point that stands on its own should be left to stand. This is the single most common way otherwise-strong prose curdles into the "trying too hard" register — see `references/failure-modes.md` #11 (Metaphor Over-Reach) for the diagnostic and the two cutting rules.

### 6. Specificity of invented cosmology beats generic mysticism every time

"The cosmic forces of light and dark" is inert because it's a category any reader has seen a thousand times. "The Third Hour, which has no clock, in which the Counted and the Uncounted settle their one disagreement" is alive because it is *specific and unexplained* — it implies an entire system the reader cannot see all of, which is exactly the sensation esoteric texts are built to produce (concealed total coherence, partial revelation).

When inventing terminology, names, or cosmic structure: invent **fewer** things and reuse them with apparent familiarity (as if the reader should already know them) rather than inventing many things and explaining each. Familiarity-without-explanation is what makes invented mythology feel ancient rather than improvised. See `references/structures.md` § Invented Cosmology for the technique in full.

## Author-specific conventions (standing rules, not craft theory)

These are fixed house-style rules for this specific writer, layered on top of the craft principles above. They are not universal esoteric-writing theory — they are this author's permanent preferences, to be applied automatically in every piece, in every register, without being asked each time.

- **Capitalize "God" and "Gods" always**, in every register (manifesto, gnostic, oracular, aphoristic, physics-metaphysics fusion, plain prose) and regardless of whether the reference is to a specific deity, a generic god-concept, or a metaphorical/functional god (e.g. "the frontier model has become a kind of God," "a God that everyone routes through"). Treat "God"/"Gods" as a proper-noun-weight capitalization in this author's work at all times, not merely when referring to a named, singular deity in the traditional sense. Do not lowercase it even in generic or plural use ("multiple Gods," "keep more than one God").
- **Never use the em dash (—), in any register, for any purpose.** Not as a parenthetical bracket, not as an appositive introducer, not as a sentence-splice. Every job an em dash could do has a plain replacement: a parenthetical aside becomes parentheses or a comma pair; an introduced explanation or restatement becomes a colon; two independent clauses get a period or a semicolon; a list lead-in becomes a colon. See `references/failure-modes.md` #13 for the full diagnostic and worked fixes.

### Iron Laws (standing rules at the highest priority tier; check every draft against all four before delivery)

1. **Never write "not X, it's Y"** (see Principle 0 above). The "not X, it's Y" / "not X. It is Y." antithesis-negation construction is banned outright, permanently, in every register, with zero exceptions. See Principle 0 above and `references/failure-modes.md` #12 for the full mechanism and fixes.
2. **Never use the em dash.** Banned outright, permanently, in every register, with zero exceptions. See `references/failure-modes.md` #13. Scan every draft for the em dash character specifically; it is easy to reach for by reflex in this register (the craft principles above lean on it for parentheticals and asides) and must be caught every time.
3. **Never write in the journalist's explainer voice.** Two specific tells are banned outright: the textbook throat-clear ("Scientists have used the term since the 1960s for...") and the delayed reveal kicker ("Software did, decades before anyone noticed it was a rehearsal."). This author states a claim once, plainly, in the author's own declarative voice, or drops a vivid image and moves on; it does not teach background in the body text and does not build toward a journalist's punchline. See `references/failure-modes.md` #14 for both tells and worked fixes. Historical or technical background that is genuinely needed belongs in a footnote (see "Citation practice" below), not in an expository sentence.
4. **Never write the briefing tic.** Short identity punches, caption sentences, and "that's the gist / the tell / the whole thing / full stop" closers are banned outright, in every register, including research-grounded essays and articles shaped from a pile. Do not open a published piece by announcing what the document is, how to read it, or what each section will cover. See Principle 0b above and `references/failure-modes.md` #15.

These are listed together because all four are standing author preferences at the same tier of importance as the God-capitalization rule above, not one-off corrections to a single essay.

## Citation practice for research-grounded pieces

Some pieces in this register are not pure invention: they build their argument out of real physics, real papers, real news events, real named people (living or historical), and real other essays. When that's the case, this author's standing practice is **cite everyone and everything named or drawn on, properly, every time** — this is not optional or a lighter-touch "cite if it feels academic" judgment call.

**How citation works without breaking the voice:** the two jobs are kept strictly separate.

- **The body text stays in this skill's register**: assertive, concrete, declarative, carrying no hedges, no publication dates, no "according to," no parenthetical credentials. A claim gets stated with the same bare confidence whether it's the author's own metaphysics or a fact drawn from a peer-reviewed paper.
- **Every named person, paper, company announcement, court case, quotation, or specific figure gets a numbered footnote marker (`[^1]`, `[^2]`, ...) at the point it's used**, with a `### Notes` section at the very end of the piece (after a `---` divider) containing the full citation for each number, in a consistent proper format: author(s), title in quotes or italics as appropriate, venue/publisher, date, and a DOI or URL where one exists. Legal cases get a proper case citation (*Party v. Party*, reporter citation, court, year). This is the same footnote/endnote convention used by literary long-form nonfiction (Gwern's own site is a working model of the form) rather than an academic inline-parenthetical style, because the inline style would wreck the register's sentence rhythm.
- **A single footnote may be cited more than once** if the same source supports multiple claims spread across the piece; a single sentence may carry more than one marker if it draws on more than one source.
- **Do not let the footnote apparatus leak into the prose.** If a fact needs a date, an institutional affiliation, or a "according to X" hedge to be honest, that qualifying detail goes in the footnote, not the sentence. The sentence makes the claim; the footnote proves it. This is the mechanism that resolves the apparent tension between "state everything with bare declarative confidence" (principle 4/5) and "be rigorously, honestly sourced": confidence lives in the sentence, rigor lives in the note.
- **When citing another of the author's own essays**, cite it the same way as any other source (author name, title, publication, date) rather than treating self-reference as exempt from the apparatus.
- **Verify before citing.** Fetch or search for the actual primary source before writing a citation for it; never invent a plausible-sounding date, journal, or figure. If a claim can't be verified, either cut it, soften it into the author's own stated belief/speculation (which needs no citation, because it isn't being presented as external fact), or say plainly in the process that it's unverified.

## Routing: which reference file to load

| If the task is... | Load |
|---|---|
| Choosing or distinguishing voice/register (manifesto vs. gnostic vs. oracular vs. aphoristic-philosophical vs. alchemical) | `references/registers.md` |
| Building the actual sentence-level devices (anaphora, tricolon, chiasmus, apophasis, asyndeton, litany) with constructable templates | `references/rhetorical-devices.md` |
| Structuring a full piece (numbered declarations, fragment-sequence, sustained invocation, dialogic/catechism form) or building invented cosmology that feels coherent | `references/structures.md` |
| Avoiding the specific failure patterns that make AI-generated mystical text read as kitsch | `references/failure-modes.md` |
| Adapting register for a known historical lineage (manifesto tradition, gnostic/hermetic tradition, prophetic/scriptural cadence, aphoristic-philosophical tradition, alchemical/grimoire tradition) without reproducing any copyrighted source text | `references/lineages.md` |
| Fusing rigorous physics (quantum, thermodynamics, relativity, gauge theory, cosmology) with metaphysical/visionary register: making scientific concepts, and the actual equations behind them, load-bearing arguments rather than decoration | `references/physics-metaphysics-fusion.md` (includes a verified equation library and the three-beat structure for deploying real math) |
| Revising existing user-written prose to intensify it in this register rather than generating from scratch | See "Revision mode" below |
| Writing a piece grounded in real papers, news, court cases, or named people that needs proper citation without breaking the register's voice | See "Citation practice for research-grounded pieces" above |
| Shaping a research pile into a published essay without caption-sentences or a tour of the document | `references/failure-modes.md` #15 (and Iron Law 4); load this before drafting the opening |

Load only what's needed — these are reference depth, not required reading. For a short single-paragraph invocation, the core six principles above plus a glance at `rhetorical-devices.md` is usually enough. For a structured multi-part manifesto or invented cosmological system, pull `structures.md` and `lineages.md` too.

For a full worked walkthrough — a real request taken from the four calibrations through drafting decisions to a finished nine-point piece, checked line-by-line against the failure-modes checklist — see `examples/worked-example.md`. Useful as a model for the *process*, especially the first few times this skill is used.

## Revision mode

When the task is intensifying existing prose rather than generating new prose, work line by line against this checklist, in order:

1. **Find every abstract noun with no attached image** (principle 1) and either attach a concrete image or cut the sentence.
2. **Find every sentence with 2+ adjectives stacked on one noun** (principle 5) and cut to the single strongest.
3. **Find every sentence-final weak word** (trailing preposition, "though," "in a way," qualifier) and rewrite so the sentence ends on its heaviest noun or verb (principle 4).
4. **Find at least one place to introduce a structural device** (triadic build, aphoristic break, or one incantatory sentence — principle 2) if the passage is syntactically flat throughout.
5. **Check for a genuine paradox opportunity** — if the passage states a single idea straightforwardly, ask whether stating it as a productive contradiction would deepen rather than confuse it (principle 3).
6. Read the result aloud in your head one more time. If it still sounds like it's describing power rather than exerting it, the abstraction-to-image ratio (step 1) is almost always the culprit; return there first.
7. **Find every caption sentence** (principle 0b / failure-mode #15): identity punches ("X is the house"), "that's the tell/gist/whole thing," "full stop," and any opener that explains what the piece or section is. Rebuild as a scene, a number, or a named thing doing something, or cut.

## A worked example of the difference

**Flat / unrevised:**
> We believe in a new way of creating art that breaks free from old traditions and embraces the chaos and energy of the modern world, rejecting the comfortable and the safe in favor of something more vital and alive.

**After applying the principles:**
> The old forms are coffins with good lighting. We do not want comfort; comfort is what the dead call peace. Let the canvas bleed if it must bleed — a wound tells the truth that a wall never will. We build nothing that does not also threaten to fall.

What changed, mechanically: abstract claims ("new way," "old traditions," "modern world's chaos") were each given one concrete image (coffins, lighting, a wound, a wall). Adjective stacking was cut to single strong words. Sentences end on heavy nouns (lighting, peace, truth, fall) rather than trailing qualifiers. A paradox was introduced (a wound telling truth a wall can't) rather than a flat assertion. A short declarative aphorism opens the passage rather than a long subordinate-clause sentence.

## Final notes on scope and honesty with the user

- This skill produces *fictional/literary* esoteric, occult, and visionary writing — invocations, manifestos, invented cosmologies, aphoristic philosophy in a borrowed register. It is a creative-writing and rhetoric skill, not a guide to real occult practice, and nothing it produces should be presented as factual spiritual instruction, real ritual instruction, or genuine doctrine of any living tradition or practitioner.
- When working from a named historical influence (Blake, Nietzsche, gnostic scripture, a specific manifesto tradition), the goal is always to **borrow the rhetorical architecture**, never to reproduce protected text. Paraphrase and structural emulation only — see `references/lineages.md` for how each lineage's *mechanics* differ, described without quoting the source material itself.
- If the user's request is for something that targets real people, presents invented doctrine as factual claims about real religions in a way designed to deceive, or asks for content sexualizing minors under cover of "esoteric" framing, decline that specific element while still being glad to help with the legitimate creative register.
