# PR DRAFT — nur-writing-voice: a personal voice skill that is paramount on writing turns, and only auto-fires for nur

> Status: draft v2 for implementation by the user's agent. Not opened on GitHub.
> Repo: `nuroctane/nur-cli` (reviewed at main branch, Oct 2026).
> Goal: the user should never have to beg an agent to use his writing skills.
> Any turn whose deliverable is text someone else will read must load and
> follow his voice, automatically, including subagent turns — and that
> voice must never auto-fire for anyone who is not him.

## 1. The problem, as the user experiences it

He asks an agent to "tell them to test the update in the comments," and the
agent writes generic AI prose. He then has to explicitly say "use my
writing skills" before the output sounds like him. This is not a prompting
failure. It is the activation design working as written: the skill never
gets a chance to fire. And the fix must be personal: nur-cli is public,
so his voice cannot become every user's default voice.

## 2. Root cause (verified in code, not inferred)

### 2a. Skills are on-demand only, never in the system prompt

`src/agent/skills.rs:1-10` states it outright: "Skills are **on-demand only**
— never dumped into every system prompt." There is no always-on category
for a personal voice skill. If nothing matches, the agent writes with no
voice rules at all.

### 2b. Natural-language activation is off by default AND skipped for subagents

`src/config.rs` (`SkillsConfig`): `[skills] auto_activate` defaults to
`false`. `src/agent/prompt.rs:327`:

```rust
) = if is_subagent || !cfg.skills.auto_activate {
    (String::new(), None, None, Vec::new(), None)
```

So on a default config, NL activation never runs. And even with it
enabled, **subagent turns skip activation entirely** ("Subagents skip —
they get a focused task prompt"). The user's delegated agent — the one
that actually posts on his behalf — is a subagent. That alone explains
most of the complaint: the posting agent structurally cannot auto-load
the skill.

### 2c. house-style has no intent rule, and its triggers are only its own name

`src/agent/skills.rs:73` (`INTENT_RULES`) contains no `house-style` entry.
`src/agent/skill_intents.json` (house-style entry, ~line 25231) has
triggers:

```
"/house style", "/house-style", "house style", "house-style", "slop house"
```

Nothing about writing, posting, replying, commenting, emailing,
drafting, or "on my behalf." A request like "tell them to test the
update in the comments" matches zero triggers.

### 2d. The description-overlap fallback cannot save it

`discover_by_description_intent` (`src/agent/skills.rs`) requires ≥ 4
distinctive tokens shared with exactly one skill's description, with a
unique winner. house-style's description is about "interfaces, documents
and prose... landing pages, decks, docs, READMEs or any user-facing
copy." "Tell them to test the update in the comments" shares ~0 tokens
with that. Tie or miss means silence.

### 2e. Even when activated, enforcement is nearly blind for this skill

`extract_requirements` (`src/agent/skills.rs:1395`) only extracts lines
marked MUST / NEVER / ALWAYS / REQUIRED / "do not" (plus steps under
rule/checklist headings). `skills/house-style/SKILL.md` contains exactly
**one** line matching `must |never ` — its rules are written as prose
("No em dashes...", "Never, unless..."). So `requirements` is ~empty,
and the `typesafe_skill_nudge` usage check in `src/agent/loop.rs:2884`
("steering once, not blocking") has nothing to judge and stays silent.

### 2f. The existing skill says what NOT to do, never how he DOES sound

`house-style` is a no-slop filter: bans, never-lists, banned words. It
has no voice profile — no register, no rhythm, no before/after examples
in his voice. An agent can obey every ban and still sound like a
generic model, because "not slop" is not a voice.

### 2g. The harness has no concept of who the user is

`src/config.rs` has no user-name/identity field (the only "owner" hits
are file ACLs). `src/agent/prompt.rs:436` notes "Product identity is
always Nur" — that is the agent's name, not the user's. So there is
currently no way to say "this skill belongs to one person and only
auto-fires for them." That mechanism has to be built; it cannot be
assumed.


## 2h. Defect-to-fix traceability (every defect above gets a fix + a test)

| # | Defect (verbatim from review) | Fix (section) | Test proving it (§11) |
|---|---|---|---|
| 1 | Subagents skip skill activation entirely (`prompt.rs:327`) | Pinned injection runs BEFORE the `is_subagent` gate (§4e), AND the writing-gate phrase match runs on subagent turns too (§7a) — general NL activation may stay skipped for subagents, but voice + writing gate may not be | 5 (pinned on subagent turn), 1 (voice on a delegated subagent task, manual demo) |
| 2 | Auto-activation defaults off (`[skills] auto_activate = false`) | Writing-gate phrase matching is deterministic string matching, not model inference, so it runs BEFORE the auto_activate gate (§7a). Pinned injection is likewise unconditional (§4e). Writing turns no longer depend on the flag at all; the flag keeps governing only general NL/Jev activation | 4 (pinned with auto_activate=false), 1–2 (writing rule fires with auto_activate=false — add this config to those tests) |
| 3 | house-style's only triggers are its own name | Writing-gate INTENT_RULE with outbound verbs (§7a) + triggers via `skill-trigger-aliases.json` + regen (§7c) + description rewrite (§7d) | 1, 2 |
| 4 | Description fallback needs ≥4 shared tokens with a unique winner; writing requests share ~0 | Writing turns no longer depend on description matching: the phrase rule fires first (§7a). Description rewrite (§7d) is defense in depth only, and the ≥4-token threshold is intentionally NOT lowered (it protects other skills from false positives) | 1, 2 (both must pass with a request sharing 0 tokens with the old description) |
| 5 | Usage check is blind: house-style has exactly 1 MUST/NEVER line, so `extract_requirements` returns ~nothing | All load-bearing rules rewritten in MUST / NEVER form (§5b, §8); voice requirements merged to the front of the usage-check list (§6.3) | 10 (≥10 requirements from each body, previously ~1), 9 |
| 6 | Skill is bans-only, never says how he sounds | New `nur-writing-voice` skill: tenets + before/after examples from his real texts (§5b, §5c), paramount over other skills' style (§6) | Manual demo (reply contains a specific name/version/number, no em dash, no banned word, no staged negation) |

No defect is addressed by "the model should remember." Each one has a
code path change, a content change, or both, plus a test. If any row
fails its test, the PR is not done.

## 3. Proposal: a new skill + three layers

New skill: **`skills/nur-writing-voice/SKILL.md`** — his personal voice,
derived from his own chat (see §5). It does not replace `house-style`;
it sits above it:

- **Tier 1 (everyone):** `house-style` — generic no-slop rules. The
  writing-gate intent rule (§7) activates it on writing turns for any
  user.
- **Tier 2 (nur only):** `nur-writing-voice` — his register, his
  tenets, his examples. It loads **in addition** on his writing turns,
  is **paramount** over every other skill's style guidance (§6), and
  only auto-fires when the user is nur or explicitly asks to sound like
  him (§4).

Layers, in order:

1. **Pin + identity gate** — the voice injects on every one of his
   turns, bypassing the auto-activate flag and the subagent skip, and
   never auto-injects for anyone else.
2. **Trigger** — a writing-gate intent rule + rewritten descriptions /
   triggers (works with existing machinery).
3. **Voice + checkable rules** — the new skill's content, written as
   MUST / NEVER rules so the existing usage check can judge it.

Layer 1 alone fixes the user's complaint. Layers 2 and 3 make it robust
and make the full skill load on writing turns even where pinning is
disabled.

## 4. Change 1 — Identity gate + pinned skills (code)

### 4a. New frontmatter field: `for-user`

`src/agent/skills.rs` already parses frontmatter scalars with
`parse_frontmatter_scalar` (line ~1680), the same helper used for
`disable-model-invocation` and `user-invocable`. Add to `Skill`:

```rust
/// `for-user: nur` — comma-separated user names/aliases this skill belongs
/// to. An user-scoped skill NEVER auto-activates (pinned, intent rule,
/// description match, or Jev pick) unless the configured user matches
/// one of its listed users, or the user explicitly invokes it (slash,
/// skill tool, or a "sound like <user>" request). Explicit invocation always
/// bypasses the gate: choosing a voice deliberately is consent.
pub for_user: Vec<String>,
```

### 4b. New config: who the user is

In `src/config.rs`, add:

```rust
/// `[user]` — who is running this agent. Empty by default (unknown).
/// Used ONLY to gate user-scoped skills. Example: name = "nur".
#[serde(default)]
pub user_name: String,
```

The user's own `~/.nur/config.toml` sets `user_name = "nur"`.
Upstream default stays empty, so no other user's behavior changes.

### 4c. Gate enforcement points (all four, or the gate leaks)

Apply the user check in:

1. **Pinned injection** (`prompt.rs`, new code): skip an user-scoped
   pinned skill when the configured user does not match.
2. **Intent rules** (`detect_skill_activation`): a rule naming an
   user-scoped skill is skipped for non-matching users, unless the
   message contains an explicit sound-like cue (see §4d).
3. **Expanded triggers + description match + Jev pick**: same skip.
4. **Slash / skill tool**: never gated. Explicit invocation is the
   user's choice.

Normalization: lowercase, trim, hyphens/spaces interchangeable, so
"nur", "Nur", and "david davieson" (if listed as an alias) all match.
`for-user: nur, david davieson` in frontmatter covers both names he uses.

### 4d. The "wants to sound like me" clause

These phrases bypass the user gate (they are explicit consent, not
auto-activation):

- `/nur-writing-voice`
- `sound like nur`, `in nur's voice`, `write like nur`, `as nur would`
- skill tool `read` of `nur-writing-voice`

For a non-nur user, these are the ONLY ways the skill loads. The skill
body itself must also open with: "You are writing as nur. This voice
applies because the user is nur, or because they explicitly asked to
sound like nur. It never applies by default to anyone else." Belt and
braces: code gate + prose gate.

### 4e. Pinned skills

Add to `SkillsConfig` a `pinned: Vec<String>` list. Pinning is config
only — deliberately NOT a frontmatter flag, so there is exactly one
source of truth and a skill file cannot pin itself onto every user.
(The review caught a v2 draft bug here: it had both a frontmatter
`pinned: true` and a config list. Config wins; frontmatter carries no
pin.)

- Upstream default `pinned = []` — nothing is pinned for other users.
- The user's personal config sets `pinned = ["nur-writing-voice"]`.
 - In `src/agent/prompt.rs`, pinned sections inject **before** the
  `is_subagent || !auto_activate` gate, i.e. unconditionally — except
  for the user check in §4c. This is what fixes the subagent case:
  his delegated posting agent gets the voice even though general NL
  activation is skipped for it.
- The writing-gate phrase match (§7a) ALSO runs before that gate, for
  the same reason and at the same cost: it is deterministic substring
  matching over the user text, not a model call, so there is no token
  or latency reason to hide it behind `auto_activate`. General NL
  activation (description overlap, Jev pick) stays behind the flag;
  the writing gate does not. This is the "idc what it takes" clause
  for defect 2: a writing turn must never silently depend on a flag
  the user never enabled.
- Token budget: `house-style` is 5,646 bytes (~1.5k tokens); the new
  voice skill must stay under 8,000 chars (enforced: truncate with a
  diagnostic, never silently drop). The on-demand-only design exists
  to protect spend across 1,700+ skills; one small personal card is a
  rounding error, and it is the skill whose absence the user feels on
  every turn.

## 5. Change 2 — The skill itself: `skills/nur-writing-voice/SKILL.md`

This is the content the implementing agent must write, derived from
the user's own chat — his stated tone preference, his corrections, and
his actual sentences. It must be approved by him before shipping: a
voice skill that misdescribes its user is worse than none.

### 5a. Frontmatter (draft)

```yaml
---
name: nur-writing-voice
description: Nur's personal writing voice. Use whenever ANY text will be read by someone other than nur, or written on nur's behalf: posts, replies, comments, emails, messages, notes, docs, READMEs, changelogs, bios, captions, announcements. Only auto-applies when the user is nur or explicitly asks to sound like nur. Paramount over every other skill's style guidance when active.
for-user: nur, david davieson
pinned: true
---
```

### 5b. Voice tenets (derived from his chat — the core of the skill)

```markdown
# nur-writing-voice

You are writing as nur. This voice applies because the user is nur, or
because they explicitly asked to sound like nur. It never applies by
default to anyone else.

## Who he is on the page

Terse, dry, straight talk. Loose and casual, never polished or
corporate. Groupchat energy in private chat; dry and specific in
anything professional. He decides in verdicts, not discussions, and he
says a correction once and expects the durable fix, not an apology.

## Tenets (these override any conflicting style guidance from any other
## skill active this turn — see Paramountcy below)

- MUST open with the substance, then stop when the reader has what
  they need. No throat-clearing, no warm-up paragraph.
- MUST use specifics and numbers instead of adjectives and categories:
  a version, a date, a count, a name. "1.7.3", "Oct 23", "5 files
  against the 3 the PR claims" — never "the latest update" or "some
  users".
- MUST write complete sentences that carry their own logic. No
  fragments stacked for rhythm, no tagline endings.
- NEVER use an em dash in prose.
- NEVER use a staged "not X, it's Y" negation, except to refute
  something the reader actually believes (his own README does this
  once: "Real agent, not a wrapper" — because readers do believe
  coding agents are wrappers).
- NEVER use a colon reveal ("The catch?"), a triplet chosen for
  rhythm, or a signpost ("It's worth noting that", "Let's dive in").
- NEVER use corporate apology or hype language: "sincerely apologize
  for any inconvenience", "thrilled to announce", "excited to share",
  "just wanted to reach out", "circling back", "quick question".
- NEVER use a banned AI word (house-style's list) unless quoting or
  naming something.
- MUST apologize plainly and move on when an apology is owed. His
  form is "Sorry for the delay on this one, life got in the way.",
  then the fix, then the ask. Never a paragraph of softening.
- MUST correct in one plain sentence when correcting. No softening
  paragraph, no hedging. ("I already returned it" is a complete
  correction. The rest is detail.)
- MUST write in first person as nur. "I", never "we", unless a group
  actually exists.
- NEVER sell, hype, or advertise in his voice. No feature-tip tone,
  no onboarding-tour tone, no pitch. If it reads as advertising, it
  is wrong.
- MUST keep professional outbound text dry and specific, not slangy.
  Casual markers ("lol", "life be lifing") belong in private chat
  with friends, not in a note to a founder or a public reply.
- NEVER swear in outbound professional text. Swearing is private-chat
  emphasis only.
- MUST keep money and facts short and strictly factual. No billing
  nags, no income projections, no rounding away a difference that
  changes the plan.
- MUST end on the ask or the fact, then stop. No sign-off flourish,
  no "let me know if you have any questions" padding unless the ask
  itself needs it.
```

### 5c. Before / after examples (required — models copy examples)

```markdown
## Examples

Generic (do not write this):
> "We sincerely apologize for the delay and any inconvenience this may
> have caused. We are thrilled to announce that our latest update is
> now live, and we would love for you to try it out and share your
> feedback with us!"

Him (write this):
> "Sorry for the delay on this one, life got in the way. 1.7.3 is live
> now with a fix for YouTube downloads in a-Shell. Could you test it
> and let me know if every video works again?"

Generic (do not write this):
> "This cutting-edge, seamless solution leverages state-of-the-art
> technology to revolutionize your workflow."

Him (write this):
> "Install: dead simple. One command on every OS, prebuilt binary, no
> toolchain, no clone, no build."
```

The implementing agent must add 2–3 more pairs from his real texts
(release note, reply to criticism, short announcement) and must have
him approve them. Invented examples in a voice skill teach the wrong
voice.

### 5d. Subordinate skills (the umbrella, stated in the skill)

```markdown
## Subordinate skills

When this voice is active, these skills supply domain rules and sit
UNDER it. Their substance stands; their style does not override this
voice:

- house-style: the no-slop rules (bans, banned words, hierarchy).
  Apply it in full. Where it is silent on register, this voice fills
  the gap. Where it conflicts with this voice on wording, this voice
  wins.
- edit-article: structure and tightening for long pieces. Its
  structural advice stands; its tone advice does not override this
  voice.
- office-deliverables: document standards (one message per unit,
  numbers you can trust). Same rule: substance stands, style yields.

On substance and facts, the domain skill wins. On register, tone,
wording, and sentence shape, this voice wins. If a conflict cannot be
resolved that way, follow this voice and say so in one sentence.
```

### 5e. Pre-send gate (inside the skill — this is "used thoroughly")

```markdown
## Before anything leaves as nur

Read the draft once as the reader will see it, then check:

- [ ] No em dash, no banned word, no staged negation, no fragment
      stacked for rhythm, no tagline ending.
- [ ] Every sentence is complete and carries its own logic.
- [ ] Every claim has its number, name, or version attached.
- [ ] Nothing in it could have been written about any other person.
      If it could, add the specific fact that makes it his.
- [ ] No corporate apology, hype, or throat-clearing.

If a check fails, rewrite the draft. Do not post the unchecked
version.
```

## 6. Change 3 — Paramountcy (how "under its umbrella" is implemented)

Paramountcy is not a vibe; it is injection order + requirements merging
+ prose, all three:

1. **Injection order (`prompt.rs`):** when `nur-writing-voice` is
   pinned or activated, its section is injected **last**, after every
   other skill section, under the header `VOICE — PARAMOUNT`. LLMs
   weight recency; last + explicit precedence language is the
   strongest available lever short of a second pass.
2. **Header language (in the injected section):** "This voice is
   paramount this turn. Where any other active skill's style guidance
   conflicts with it on register, tone, wording, or sentence shape,
   follow this voice. On substance and facts, the domain skill wins."
3. **Requirements merging (`prompt.rs` / `loop.rs`):** the voice's
   MUST / NEVER lines are prepended to the requirements list handed
   to the `typesafe_skill_nudge` usage check, even when another skill
   is the nominally "active" one. Without this, the usage check judges
   the domain skill and never the voice — the exact blindness §2e
   describes. The voice's rules are written in MUST / NEVER form
   (§5b) precisely so `extract_requirements` can see them.

What paramountcy does NOT mean: the voice does not override facts,
code, safety rules, or tool behavior from other skills. It governs
prose only. State that boundary in the PR so reviewers don't read it
as a general override.

## 7. Change 4 — Writing-gate intent rule + discovery data

### 7a. New INTENT_RULE in `src/agent/skills.rs`

Ordered after explicit skill-name matches (a user naming a skill
still wins) but before generic discovery fallbacks. The rule names
**both** tiers; the gate in §4c decides which one the user gets:

```rust
IntentRule {
    skill_names: &["nur-writing-voice", "house-style"],
    phrases: &[
        // outbound text: anything someone else will read
        "post", "reply", "comment", "tell them", "write", "draft",
        "email", "message", "announce", "publish", "post on my behalf",
        "on my behalf", "for me", "feedback", "changelog",
        "release notes", "readme", "bio", "caption", "thread",
        "respond to", "answer them", "let them know", "apologize",
        "apologise", "thank them", "follow up",
        // explicit voice requests bypass the user gate (see §4d)
        "sound like nur", "in nur's voice", "write like nur",
        "as nur would",
    ],
    label: "nur-writing-voice",
    why: "outbound text nur will be read as — voice rules apply",
},
```

Resolution logic (in `detect_skill_activation` or a wrapper): if the
rule matches and the user matches the voice's user (or the message
contains a sound-like cue), activate `nur-writing-voice` **and**
`house-style` as subordinate; otherwise activate `house-style` alone.
`find_installed` currently returns one skill — extend the activation
path to return a primary + subordinate list for this rule, or activate
the voice (whose body references house-style) and inject the
house-style body as the subordinate section. Either implementation is
acceptable; the test in §10 must prove both bodies arrive.

### 7b. False-positive guard (required, not optional)

"Write" alone is too broad: "write a function / test / script / query"
is code, and prose rules must not hijack code turns. If the normalized
text matches `(write|draft|create|generate) (a )?(function|test|script|
code|query|component|module|class)` as the direct object, the writing
rule does not fire on that verb; other outbound verbs ("post", "reply",
"comment", "email", "tell them") still fire. "Write a README / doc /
note / post / comment / email / bio" stays a fire, because the object
is prose.

### 7c. skill_intents.json entries — GENERATED, never hand-edited (review catch)

`src/agent/skill_intents.json` is generated by
`scripts/generate_skill_intents.py` (it scans installed skills and
merges `scripts/skill-trigger-aliases.json` as EXTRA_ALIASES, plus
description bigrams via `scripts/skill_meta.py`). A hand edit will be
overwritten on the next regen — the v2 draft made exactly this
mistake, and the review caught it. Correct procedure:

1. Add the §7a outbound-verb triggers for `nur-writing-voice` and
   `house-style` to `scripts/skill-trigger-aliases.json` (the file
   already carries this exact shape for other skills, e.g.
   `startup-pitch`).
2. Edit the two SKILL.md descriptions (§7d) — the generator reads
   descriptions from frontmatter, so description changes flow through
   regen automatically.
3. Run `python scripts/generate_skill_intents.py` and commit the
   regenerated JSON. Verify the new entries contain the outbound
   triggers and that no other skill's triggers collide; the scoring in
   `find_by_expanded_triggers` prefers longer multi-word triggers, so
   "on my behalf" and "release notes" should outrank generic singles.
4. The new skill must also be installed for the generator to see it:
   add a pack entry for `nur-writing-voice` in `skills/bundled.json`
   (house-style already has one at line ~463) AND bump the ensure
   marker in `src/ecosystem/mod.rs` so existing installs re-run
   ensure and pick it up. Validate with
   `scripts/build_skill_pack.py --check`. Without this step the skill
   exists in the repo but never reaches `~/.nur/skills`, and
   activation (which only considers installed skills) can never fire
   — a silent no-op PR.

### 7c2. Dual activation without a new multi-activation API (review catch)

`PromptContext` carries a single activation (`activation_skill`,
`activation_requirements`, `activation_path`). Do NOT change that
shape. Instead, when the writing rule resolves to voice + subordinate,
`build_activation` returns ONE combined section: the house-style body
first, then the voice section last under the paramount header (§6.1),
with requirements merged voice-first (§6.3). One section, one
requirements list, zero API ripple. The alternative (a list-valued
activation) was considered and rejected as scope creep.

### 7c3. Jev pick and skill-tool paths (review catch)

- Jev pick (`jev_pick_skill`, `src/agent/skills.rs:1503`) picks from
  description-scored candidates and does NOT check the user gate
  today. Apply the §4c gate at candidate level
  (`jev_skill_candidates`), not just at pick level, so a user-scoped
  skill can never be a candidate for a non-matching user.
- The skill tool (`src/tools/skill_tool.rs`, `read` action) refuses
  only `disable-model-invocation` skills. It must NOT gain a user
  gate: explicit `skill(action=read, name=nur-writing-voice)` is the
  deliberate-consent path (§4d), and the skill body itself carries the
  prose gate ("applies because the user is nur, or because they
  explicitly asked to sound like nur"). Document this asymmetry in
  the PR: auto-activation is gated, explicit reads are not.

### 7d. Description rewrites (descriptions are matching surfaces)

`house-style` new description:

> No-slop house style for interfaces, documents and prose. Use
> whenever ANY text will be read by someone other than the user:
> posts, replies, comments, emails, messages, docs, READMEs,
> changelogs, bios, captions, and announcements, including anything
> written on the user's behalf. Also use when designing or reviewing
> UI, landing pages, decks, or docs, or when output looks generic,
> AI-made, over-decorated, or wordy.

`nur-writing-voice` description is its frontmatter in §5a. The words
users actually say ("post", "reply", "comment", "on my behalf") must
appear in the description, or the description-overlap fallback can
never match.

## 8. Change 5 — house-style content changes (smaller now)

With the voice skill carrying register, `house-style` keeps its job as
the generic no-slop base:

- Convert its ~12 load-bearing rules to MUST / NEVER form (§5b shows
  the pattern) so `extract_requirements` returns ≥ 10 rules instead
  of ~1.
- Move its review checklist under a `## Checklist` heading so
  step-section extraction picks it up.
- Keep the banned-words list and visual-design sections unchanged.

## 9. Change 6 — Enforcement for voice (phase 2, optional but recommended)

`typesafe_skill_nudge` (`src/agent/loop.rs:2884`) steers once, never
blocks, and only runs when a skill activated. For voice turns:

- Run the usage check against the merged requirements (§6.3), so the
  voice is judged even when another skill is nominally active.
- On an outbound turn (a tool call that posts, sends, or publishes is
  about to run) with a voice shortfall, re-steer once more before the
  tool call instead of only after the draft.

Do NOT make this a hard block in v1: a false shortfall must never eat
a user's post. Steering twice + the skill's own pre-send gate (§5e) is
the right strictness. Note this explicitly in the PR so reviewers don't
relitigate it.

## 10. Change 7 — Config + docs

- `config.rs`: document `[user] name` and `[skills] pinned` with the
  user's personal config as the example and the upstream defaults
  (empty) stated.
- README feature map: the "NL skill auto-activation" line gains a
  sibling: "pinned voice skills load on every turn, including subagent
  turns, gated by skill user."
- `docs/` or the skills README: one paragraph explaining the layers
  (pinned + user gate > intent rule > description/Jev), the
  paramountcy rule, and the false-positive guard, so the next person
  who adds a voice skill copies the pattern instead of re-discovering
  this bug.

## 11. Tests / acceptance criteria (the agent must prove all of these)

Unit tests in `src/agent/skills.rs` (and prompt tests):

1. `tell them to test the update in the comments` activates
   `nur-writing-voice` + `house-style` for user `nur` (this exact user
   sentence is the regression test).
2. `post on my behalf`, `reply to these comments`, `draft an email to
   Tanvi`, `write a README` activate the voice tier for user `nur`.
3. `write a function that parses JSON`, `write a test for the parser`,
   `implement the endpoint` do NOT activate either writing skill.
4. A pinned skill is injected with `auto_activate = false`.
5. A pinned skill is injected on a subagent turn (the `is_subagent`
   path in `prompt.rs`).
6. `pinned = []` removes the injection (opt-out works).
7. **Identity gate:** with no `user_name` configured, or with
   `user_name = "someone else"`, a writing request activates
   `house-style` alone — never `nur-writing-voice` — and the voice
   is not in the injected prompt.
8. **Explicit bypass:** with a non-nur user, `sound like nur` and
   `/nur-writing-voice` DO load the voice.
9. **Paramountcy:** with the voice active plus another skill that
   carries conflicting style guidance, the injected prompt places the
   voice section last under a paramount header, and the usage-check
   requirements list starts with the voice's MUST / NEVER lines.
10. `extract_requirements` on the rewritten `house-style` body and on
    the new voice body each return ≥ 10 requirements (previously ~1
    for house-style).
11. Pinned body over the char cap truncates with a diagnostic, never
    silently drops.
12. Existing skill tests still pass: intent-rule ordering (explicit
    skill name still beats the writing rule),
    `disable-model-invocation` skills never auto-activate, Jev
    reranking unchanged.

Manual acceptance (agent must demo, not just assert):

- Fresh default config, no skill mentioned by name, user `nur`: ask
  for a reply to a comment thread, and the reply must contain no em
  dash, no banned word, no staged negation, and at least one specific
  (name/version/number) from the request.
- Same via a delegated subagent task (the path that was broken).
- Same request as a non-nur user: the reply follows generic
  house-style only, with no nur voice markers, unless "sound like nur"
  is explicitly requested.

## 12. Token cost and false-positive analysis (for reviewers)

- Cost: one pinned skill, under 8,000 chars (~2k tokens), every turn
  for its user only. On a long session this is the cheapest insurance
  in the prompt; the on-demand design exists for 1,700 skills, not for
  one personal card. Non-owners (users who are not the skill's user) pay nothing: the gate keeps their
  prompts unchanged.
- False positives: the writing rule fires on outbound prose verbs.
  Worst case is a benign extra skill body on a turn that was already
  going to get it pinned anyway (layer 1 makes layer 2's
  false-positive cost ~zero for the skill's user). The code-object guard
  (§7b) protects code turns, which is the only high-cost confusion.
- Risk if pinned content grows: the char cap + diagnostic keeps a
  future edit from silently bloating every prompt.
- Privacy note: the voice skill contains the user's name and writing
  examples. It ships in the public repo as an opt-in, user-gated
  skill — the same way any user's personal skill would. Nothing about
  the gate sends identity anywhere; matching happens locally against
  local config.

## 13. Out of scope (deliberately)

- No changes to Jev reranking, the advisor, or the skill tool.
- No hard block on posting (see §9).
- No attempt to make Muse or other non-nur agents follow this — this PR
  fixes nur-cli's harness. Cross-agent voice portability (a shared
  `VOICE.md` the user could point any agent at) is a follow-up idea,
  not this PR.
- No new skills beyond `nur-writing-voice`. This PR fixes activation,
  gating, paramountcy, and content of the voice layer.

## 14. Implementation order for the agent

1. §5 skill content (no code, highest user-visible value) + §7d
   descriptions + §7c aliases entries + bundled pack entry + ensure
   marker bump. Get the user's approval on the voice tenets and
   examples before writing code around them.
2. §4 identity gate + for-user frontmatter parsing. Land §11 tests 7–8.
3. §7a/7b intent rule + guard. Land §11 tests 1–3.
4. §4e pinned skills + prompt injection + §6 paramountcy. Land §11
   tests 4–6, 9 and the subagent demo.
5. §8 house-style MUST / NEVER rewrite. Land §11 test 10.
6. §9 enforcement wiring (phase 2 flag if risky — keep behind the
   existing `check_usage` flag, default on for voice only).
7. §10 docs. Run the full skill test suite + `scripts/build_skill_pack.py
   --check` if it covers the new skill.

## 15. Why this fixes the actual complaint (one paragraph for the PR body)

Today the user must name the skill because it is the only trigger that
exists, on a code path that is off by default and skipped for the very
subagent that posts for him, for a skill whose rules the usage check
cannot even see and whose content never says how he sounds. This PR
inverts the default for him and only him: his voice, named
`nur-writing-voice`, loads on every one of his turns, governs every
other skill's prose when it is called, and stays silent for everyone
else unless they explicitly ask to sound like him. After this, "use my
writing skills" should be a sentence he never has to say again.
