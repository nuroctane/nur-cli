---
name: interior
description: "Micro-interactions for React from interior.dev (ddoemonn/interior) - finished half-second-after-click craft: headless hooks + copyable components, material depth, reserved space, abandonment-safe gestures. Use when building buttons, overlays, lists, drag, async UI, or when the user says interior / micro-interaction / half-second / jump-restart / make it feel finished."
---

# Interior - finish the half-second after a click

Upstream: [ddoemonn/interior](https://github.com/ddoemonn/interior) · docs: [interior.dev](https://interior.dev) · design language: `DESIGN.md` in that repo.

There is **no npm package**. Every component is one file under `components/interior/` that you **copy** into the project. Each file exports:

1. a headless hook (`useX`) - all behaviour, zero class names
2. a styled component (`X`) on that hook - keep or reskin

Only dependency: [`motion`](https://motion.dev).

If `nur plugins install interior` has been run, the clone lives at `~/.nur/plugins/interior/` - prefer reading those files over reinventing.

## Premise

Everybody ships these widgets at ~80%. The missing 20% is always the same: a **jump**, a **restart**, or motion that **ignores the person watching**. Trust is won and lost in the half-second after a click. Finish that half-second.

Voice: short, declarative, a little dry. Describe the problem the motion removes - never sell the animation.

## Materials (site chrome vs copied components)

Three layers of real depth:

| Layer | Role | Light | Dark |
|-------|------|-------|------|
| bezel | page frame / background | `#EFEEEA` | `#141312` |
| panel | lifted card / content | `#FFFFFF` | `#1D1D1A` |
| well | recessed inputs / chips | `#F6F6F4` | `#252522` |

Site chrome may use design tokens / `.mat-*` utilities. **Shipped interior components must use literal hex + Tailwind stone + the exact ink shadows** so they work after copy-paste into any Tailwind project. Never hand-write a random `box-shadow`.

Ink shadow on light (never plain black on warm surfaces):

```
panel  shadow-[0_1px_2px_rgba(28,25,23,0.06),0_4px_10px_-8px_rgba(28,25,23,0.45)]
float  shadow-[0_28px_56px_-24px_rgba(24,22,20,0.45)]   # modals
well   shadow-[inset_0_1px_2px_rgba(28,25,23,0.07)]
```

Dark mode may use black shadows (absence of light). A plain `border` never carries elevation.

## Hard laws (obey these)

1. **Nothing moves unless something happened.** No ambient loops that pretend at life.
2. **Physical processes obey physics** - spring/settling matches the interaction, not taste.
3. **Reserve space before content arrives** - invisible twin / skeleton of final size. No layout jump.
4. **Every abandon path is first-class** - blur, Escape, pointer cancel, route change, tab sleep. Keyboard is a second complete implementation, not a fallback.
5. **`prefers-reduced-motion`** - information still arrives; only the trip is skipped.
6. **State without disabling** - prefer busy/pending/aria over greying out controls that trap focus.
7. **Overlays** - portal + scroll lock + `inert` on background + stack order. One Escape dismisses the top layer.
8. **Async** - grace period before spinner, minimum visible time once shown, settle, rollback on failure.
9. **Radii nest** - inner radius ≤ outer; page panels ~`rounded-[20px]` with bezel gutter `p-3 sm:p-5`.
10. **Discrete cells beat continuous bars** when counting steps / strength / progress segments.

## Component catalog (copy from upstream)

Under `components/interior/`:

accordion · blur-up-image · collapsible-banner · command-palette · context-menu · copy-button · drawer · dropdown · expanding-search · filter-grid · floating-label · hide-on-scroll · hold-to-confirm · icon-morph · inline-validation · lightbox · like-burst · live-activity · load-more · loading-button · logo-marquee · long-press · modal · new-items-pill · otp-input · pagination · password-strength · poll-results · popover · presence-avatars · press-depth · progress-bar · reading-progress · reorder-list · ripple · scroll-spy · segmented-control · show-more · skeleton-swap · slider-detents · snap-carousel · sortable-table · sticky-header · streaming-text · swipe-deck · tabs · tag-input · task-steps · text-reveal · tooltip-group · tree-view · typing-indicator · value-flash · wizard-steps

## Workflow

1. Match the interaction to a catalog component (or compose hooks).
2. Copy the source file; keep the hook API; restyle with the host design system if needed.
3. Verify: no jump on load/label change, abandon paths, reduced motion, reserved space, focus rings.
4. When inventing a new micro-interaction, read upstream `DESIGN.md` sections 11-22 before writing.

## Anti-patterns

- CSS-only “polish” that ignores cancel/blur/Escape
- Spinners that flash for 50ms or layout-shift in at 300ms
- `disabled` buttons that steal the obvious next action during async
- Decorative motion with no user-initiated cause
- Copying site tokens (`.mat-panel`, `--accent`) into a portable component file
