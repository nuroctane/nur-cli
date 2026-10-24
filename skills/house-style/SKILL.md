---
name: house-style
description: No-slop house style for interfaces, documents and prose. Use when designing or reviewing UI, landing pages, decks, docs, READMEs or any user-facing copy, or when output looks generic, AI-made, over-decorated or wordy.
---

# House style: no slop

Generic AI output is recognizable on sight: gradients, glow and shadows, a card around
everything, an emoji on every label, a tagline under every heading, and prose padded with
the same thirty words. This skill is the opposite default. Apply it while you work; a
cleanup pass at the end costs more and misses more.

## The two rules that do the most

1. **Cut taglines, helper text and secondary labels.** If a screen needs them to make sense,
   fix the layout instead. A subtitle that restates the title, a hint under every field, an
   eyebrow label over every heading: each one is a sign the structure is not doing its job.
2. **Make the decision that improves all three experiences at once**: the user's (UX), the
   developer's (DX) and the agent's (AX), without breaking anything that works today. A change
   that helps one and quietly costs another is not done.

## Visual design

Honor what exists first. In an existing product, file or brand, everything you add matches its
fonts, sizes, colors, spacing and component treatment, even where it differs from this guide,
unless the user asked for a restyle.

Build hierarchy in this order, and stop as soon as it reads clearly:
placement and reading order → whitespace and grouping → type size and weight → rules, fills or
color → illustration or icons that add meaning. If hierarchy depends on a colored box around
every block, the structure is not resolved yet.

Never, unless the brand already does it:
- box shadows, glows, glassmorphism, or decorative blobs and shapes
- gradients, and above all the blue-to-purple one
- a wall of rounded cards where plain sections, rows or columns would be clearer
- a card or callout with a colored accent border on one side
- an icon beside every label, or emoji anywhere in product UI
- `🟢 LIVE`, `NEW`, `BETA` and similar badges that carry no information the user needs
- small all-caps eyebrow labels above titles, oversized section numerals
- a tagline restating the heading, or an italic serif word dropped into a sans heading for flourish
- labels in corners, headers or footers beyond page numbers, sources and required markings

Type uses a fixed set of roles (title, heading, body, caption or source note). Never shrink type
or turn on shrink-to-fit to rescue a crowded layout: cut, split or restructure the content.

## Words

Write for the decision the reader has to make. Open every page, section and message with its
substance, then stop when the reader has what they need.

- Plain, specific words. A number beats an adjective; a named thing beats a category.
- Complete sentences that carry their own logic ("Revenue grew 4% while costs grew 9%, so margin
  fell from 14% to 11%"), not fragments stacked for rhythm.
- Write about the subject, not the artifact ("Churn doubled after the April price change", not
  "This slide shows that churn doubled").
- Use "not X but Y" only to refute something a reader actually believes.
- No rhetorical questions, colon reveals ("The catch?"), triplets chosen for rhythm, or signposts
  ("It's worth noting that", "Let's dive in").
- No em dashes, no middle-dot or slash chains standing in for a sentence, no semicolons joining
  independent clauses.
- Bold only a lead-in that is itself the takeaway. No decorative italics or color.

### Words that mark text as machine-written

Do not use these unless quoting or naming something:

delve, dive into, deep dive, tapestry, testament, realm, landscape, ever-evolving, fast-paced,
in today's world, navigate (a challenge), embark, journey (for a process), unlock, unleash,
empower, elevate, supercharge, revolutionize, game-changer, cutting-edge, state-of-the-art,
next-level, seamless, seamlessly, robust, powerful, leverage, harness, utilize, facilitate,
streamline, optimize (as filler), synergy, holistic, multifaceted, comprehensive, meticulous,
intricate, nuanced, pivotal, crucial, paramount, vital, foster, bolster, underscore, showcase,
boast, highlight (as a verb for "shows"), vibrant, bustling, nestled, myriad, plethora, a wide
array of, a variety of, in conclusion, ultimately, at the end of the day, it's important to
note, rest assured, look no further, whether you're X or Y, from X to Y (as a range of
everything), crafted, curated, effortless, effortlessly, intuitive (unmeasured), blazing fast,
world-class, best-in-class, mission-critical, transformative, innovative, groundbreaking.

Replace them with what the thing actually does, with a number where you have one.

## Review checklist

Before you call UI or copy done, look at it once as the user will see it:

- [ ] Every label, hint and tagline earns its place; removing it would confuse someone.
- [ ] No shadow, gradient, glow, decorative shape, badge or emoji the brand does not already use.
- [ ] Hierarchy reads from placement and spacing before color.
- [ ] Every sentence opens with substance; none of the words above appear.
- [ ] The change helps UX, DX and AX together and breaks nothing that worked.

Sources: the two rules come from @Voxyz_ai's recommended AGENTS.md lines; the "never" list
reflects @shipper_now's epstein.md principles (no box shadows, emojis, LIVE badges or
blue/purple gradients, and a ban on common AI words); the visual-hierarchy order and writing
rules follow Paper Instruments' prompt-tuning guidance and its MIT-licensed Feather Harness.
