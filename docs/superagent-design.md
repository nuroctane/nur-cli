# Superagent design patterns (research only)

Design patterns adapted from the DeepSeek dsh-superagent prototype, a
research prototype explicitly marked "for reference only, not an official
product" and "not production-ready software". No code was imported, and
none of its GitHub/Feishu source, Harness presets, web panel, or
TypeScript implementation is ported here. What transfers is a domain
model for work that arrives from elsewhere and runs without constant
human attention. The working principle: take as little human attention
as possible, give as much value as possible, act only when asked or when
a rule the team wrote allows it, and speak only when someone must act or
can use a result.

## Domain model: Source, Thread, Trigger, Eval, Ticket, Mandate

- **Source** is where work arrives (a chat thread, a repo, a channel).
- **Thread** is the conversation a trigger arrived on.
- **Trigger** is one immutable event. Accepting a trigger twice creates
  one ticket: idempotence is a property of the trigger, not of the
  executor's memory.
- **Eval** has two layers, and they must stay separate. The gate is
  deterministic: who may originate work, with what strength. The
  assessment is model judgement: is this work worth doing, and what does
  it need. A gate that holds state is not a gate; it is a second ticket
  system.
- **Ticket** is a unit of authorized work. Only Eval creates one. It
  defines its own state (open, ended, back to open on a person's message
  unless stopped) instead of following the executor's state. An executor
  that crashed does not close a ticket; a person that stopped it does.
- **Mandate** is what an accept grants, and it is immutable: policy
  version, scope, strength, matched rules, sender identity, and how the
  source received it. Wider scope means a new ticket, not an expanded
  mandate. Only a person's stop withdraws it.
- **Subscription** is many-to-many Ticket-to-Thread fan-out. A matching
  trigger fans out to every live subscribed ticket, with no check of who
  sent it, but delivery grants no new authority: a person's message on a
  subscribed thread resumes an ended ticket under the same mandate, with
  no re-evaluation.
- **Assignment** binds one ticket to one executor instance (pending,
  running, ended, independent of ticket state).
- **Report / Attention.** Assignment reports are `progress`, `waiting`,
  `ready`, `blocked`, `failed`, or `dropped`. A structured **Attention**
  is a question awaiting a person, and it is a field, not an inference:
  inferring "waiting on a person" from a report's free text is explicitly
  rejected. Nur-cli's equivalent today is free-text `BLOCKED:`, which is
  exactly the gap this pattern names.

## Execution rules worth adopting

- One executor session and one directory per ticket (typically a git
  worktree), prepared once, with a time limit on preparation.
- Worker slots with a concurrency limit, plus one extra slot for a
  person's message, so a person is never queued behind background work.
- Background work gets a bounded tail after a turn ends (the prototype
  uses 30 minutes), then jobs are killed and the ticket reports waiting.
- A handoff to a person frees the slot immediately.
- Execution variables contain no credentials. Nur-cli already follows
  this with `$NAME`-only secrets; a mandate record must follow the same
  rule and never carry a value.

## Mapping to nur-cli today, in adoption order

1. **Mandate record.** Attach to a `goal` (or a ticket built on it):
   who asked, scope, strength, policy version, matched rules, sender
   identity. No mandate, no autonomous start.
2. **Eval gate.** Before starting work from an external trigger, run a
   deterministic allowed-sender + scope check, separate from the model's
   assessment of the work itself.
3. **Structured Attention.** Replace free-text `BLOCKED:` with a field
   the system can route: what is being asked, of whom, and what unblocks
   the ticket when answered.
4. **Subscription resume.** A later message on the same thread resumes
   an ended ticket under its mandate, without re-evaluation, and without
   granting new authority.
5. **Slot accounting.** Worker-slot limits with a person-priority extra
   slot, so background work cannot starve a person's message.

## Explicitly not ported

Feishu source/bot, GitHub source implementation, DeepSeek Harness
presets, the web panel, account wrappers, and the TypeScript code
itself. Some designs discussed in the prototype's article are directions
for future development and are not implemented in the prototype; they
are not claimed here either.
