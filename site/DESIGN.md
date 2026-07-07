# Glance DESIGN.md

This file is the product's public-site brand contract. Keep it short and exact:
agents and humans should be able to update `site/` from this file without
inventing a second design system.

## Fleet Lock

- Lock: operator lock-in 2026-07-07, `misty-step-936`.
- Tagline and homepage `h1`: `Understand your codebase.`
- Layout: Mural.
- Hero image: `site/assets/hero.jpg`.
- Image provenance: generated for the Misty Step fleet lock with `gpt-image-1`
  using the Misty Step fresco language, staged as `glance-hero.jpg`.
- Background opacity: `0.35`.
- Homepage structure: hero only, one viewport, no scroll.
- CTA: `<a class="ae-button" href="get-started.html">Get started</a>`.

## Chrome

- Header: Lucide `scan-search` mark in `.ae-app-mark`, uppercase wordmark, and
  nav links `features`, `get started`, `changelog`, `github`.
- GitHub link: `https://github.com/misty-step/glance`.
- Footer: mode toggle on the left; right side reads `a Misty Step project`
  with `Misty Step` linked to `https://mistystep.io`, followed by an inline
  GitHub SVG icon link.
- No bare URL text, email, copyright line, or Weave footer link.

## Brand Voice

- Plain-spoken, concrete, and operator-facing.
- Lead with the user outcome, then the proof.
- Avoid marketing fog, mascot language, and decorative claims.
- The differentiator is the honesty gate, not the writing: say "every claim
  is cited and machine-checked," not "AI-powered documentation."

## Lucide Mark

- Icon: `scan-search`
- Reason: already the provisional mark for Glance across the fleet's own
  product tracking (a magnifying glass over scan-corner brackets — fits
  "verified understanding," not just search).
- Rule: the mark is an inline Lucide SVG inside `.ae-app-mark`. No bespoke
  marks, logo images, emoji marks, or colored wordmarks.

## Palette Hooks

Pin `data-ae-theme="moss"` — matches the fleet's other green-accent live
products and reads as calm/trustworthy for a documentation product.

```css
:root {
  --ae-accent: #2643d0;
  --ae-accent-dark: #8c9eff;
}
```

No extra categorical hues needed.

## Screenshot Inventory

| File | Surface | State | Caption |
| --- | --- | --- | --- |
| `site/assets/screenshots/01-cited-page.png` | Generated page for `crates/glance-check/src` | Real self-run, current `main`, post placeholder/empty-subject fixes | A real generated page narrating Glance's own citation checker — with hyperlinked file:line citations, not summaries. |
| `site/assets/screenshots/02-check-gate.png` | `glance check` | Real terminal output against the self-run output above | The honesty gate: all 8 citations on that page verified against the pinned source SHA before the run can ship. |
| `site/assets/screenshots/03-cited-page-detail.png` | Same generated page, scrolled | Real self-run output | The "seams and sharp edges" section and file table — more citations, real function signatures. |

All three captures are from a real leaf-tier run of the current, post-fix
codebase — not the earlier broken self-run in `docs/self-run-2026-07-03.md`,
which predates this week's placeholder-image and empty-subject-sentence
fixes. The run did not reach a root or crate-level page before this lane
stopped it (a single interior/root call stalled past four minutes); the
gallery honestly shows leaf-tier output rather than claiming a root page
that was never generated.

## Release Notes Rule

`site/changelog.html` is user-facing. Glance has no tagged release and no
Landmark export yet — the page says so honestly and points at real recent
commits and site locks instead of inventing a version number.
