---
name: office-deliverables
description: Professional standards for Word documents, Excel workbooks, PowerPoint decks and PDFs - house style, structure, and the checks that make a deliverable trustworthy. Use with the docx, xlsx, pptx or pdf skills whenever the output is a document someone will read or a model someone will keep using.
---

# Office deliverables

The `docx`, `xlsx`, `pptx` and `pdf` skills say how to produce the files. This skill says what a
professional reader expects from them. Read the reference for the format you are making before
you start, not after:

| Making | Read |
| --- | --- |
| A new or restyled document, deck, workbook or PDF with no client template | `references/design.md` (house style: color, type, spacing, tables, charts, accessibility) |
| A Word document | `references/docx.md` |
| An Excel workbook or financial model | `references/xlsx.md` |
| A PowerPoint deck | `references/pptx.md` |
| A PDF | `references/pdf.md` |

The references come from Paper Instruments' Feather Harness (MIT, see `LICENSE.txt`). Where
they mention `.agents/design.md`, read `references/design.md`. Where they mention Paper's own
packages or `oracle.recalc`, use the matching tool from the `xlsx`, `docx` and `pptx` skills
(for example their LibreOffice recalculation script), and say when you could not recalculate.

## Rules that apply to every format

1. **The client's template, brand and existing file win.** Anything you add to an existing file
   matches its fonts, sizes, colors, spacing and conventions, even where they differ from the
   house style. Restyle only when asked.
2. **One message per unit.** Each slide has an action title that states its point; each
   document answers its question at the start; each sheet has one role (inputs, calculations,
   outputs, checks).
3. **Short and plain.** Plan a word budget from any length the user gives (a 12 pt single-spaced
   page holds about 450 words). Cut content rather than shrinking type or margins. No prose in
   the working sheets of a numerical model.
4. **Restrained design.** Hierarchy from placement and spacing before color; a fixed set of type
   sizes; no gradients, shadows, decorative icons or one-sided accent borders (see the
   `house-style` skill).
5. **Numbers you can trust.** Never replace a formula with its value. Describe numbers as
   recalculated only after an engine recalculated them. Tie key outputs back to their sources
   (sources = uses, opening + movements = closing, segments = reported total).
6. **Look at the result.** Render the pages or slides and check them as the reader will see
   them before calling the work done, then stop: a passed check does not need rerunning until
   the file changes.
7. **Hand off briefly.** Say what was delivered and where, then any decision or limitation the
   user must know. Do not narrate the work or describe the file's contents back to them.
