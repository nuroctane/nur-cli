---
name: research-spike
description: Run a time-boxed research spike that ends in findings, not code. Use when asked to research, spike, evaluate, or compare an approach, library, or design before building it.
---

# Research spike

A spike answers a question. It does not ship production code, and it
does not grow into an implementation by accident.

1. **Frame the question.** One question, one time box, and the decision
   the answer will feed. Write them down before searching. If the
   question cannot be stated in one sentence, it is two spikes.
2. **Survey, then dig.** Start broad: `web_search` and `web_fetch` for
   the documented options, then `sgrep` and `read_file` for how the
   current codebase already handles the problem. A spike that only reads
   the web will recommend something the codebase cannot use; a spike
   that only reads the codebase will miss the better option.
3. **Compare on the same axes.** For each option: what it does, what it
   costs (dependencies, license, maintenance), what it breaks, and what
   it requires of this codebase. License matters: an AGPL component is
   not a drop-in for a differently licensed project, and a component
   with no native build for the target platform is not a candidate.
4. **Write findings, change nothing.** Deliver `docs/research/` findings
   (or the path the user named) with: the question, the options compared
   on the same axes, the recommendation, and the open questions that
   would change it. No production code changes, no dependency additions,
   no config edits. A spike that edits production code has stopped being
   a spike.
