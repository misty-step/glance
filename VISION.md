# VISION — glance

Glance turns a repository into a place you can walk through instead of a pile
you must read. It recursively generates one rich HTML page per directory —
leaf to root — each page narrating what that room is, what it's composed of,
how it participates in the whole, and where it can hurt you, with every claim
cited to source. The root page is the front door of a deployed site that is
always an accurate representation of master.

Descended from the original glance (phrazzld/glance, Go, glance.md summaries):
same recursive soul, re-founded for the agent era — stronger models, richer
pages, verification, and a durable home. It also inherits the mission of the
atlas experiment (archived 2026-07-03): understanding as the product, evidence
as the discipline.

## The form factor (operator-ratified 2026-07-03)

- **The sister repo.** Each source repo gets a mirror repo (`<name>-glance`)
  with the identical directory structure, containing ONLY generated HTML (one
  index.html per directory + metadata). Version-controlled, durable,
  non-ephemeral: the distillation of the source at every point in its history.
  Nothing generated is ever committed into the source repo (a scar, not a
  guess: in-repo generated files have broken real builds).
- **PR mirroring.** A pull request in the source triggers regeneration of the
  changed subtree and opens a mirrored PR in the sister repo; the source PR
  gets a comment linking it. Merge to source master ⇒ sister master updates ⇒
  the deployed site updates. The site of record always reflects master.
- **Leaf to root, hash-gated.** Only directories whose content-hash changed
  regenerate; changes percolate up the ancestor path only. Cost scales with
  the diff, not the repo.

## The page contract

One self-contained HTML page per directory, house-templated (Misty Step
aesthetic kit), sections:
1. **What this is** — opinionated prose, not a file listing. Architecture
   thinking, not literal summary.
2. **Role in the whole** — how this room serves the building (written with
   parent/sibling context).
3. **Composition** — the children, each with a one-line role, linking to
   their pages (the recursion made navigable).
4. **Seams & contracts** — interfaces in/out, invariants, env/config touched.
5. **Where it can hurt you** — failure edges, asymmetries, sharp lines.
6. **Citations everywhere** — every claim carries file:line, hyperlinked to
   the source at the pinned SHA. A page renders no uncited claims.
Earned extras (not defaults): ONE diagram (generated SVG or image) when shape
carries meaning; ONE interactive/animated explainer when behavior carries
meaning (the operable-miniature and trace primitives proved 2026-07-03).
Root and package-level pages additionally carry **cross-cutting sections**
the tree cannot: flows (user/data), the data model, the failure-edge index —
because directories encode organization, not behavior.

## The honesty gate (deterministic, non-negotiable)

After generation, `glance check` verifies every citation: the file exists and
the cited lines exist at the pinned source SHA. A page with a broken citation
fails the run — it does not ship. This is the lightweight descendant of
atlas's verify: narration may be model-made; its anchors are machine-checked.
(No LLM judges anywhere in the gate.)

## Model economics (tiers, not vibes)

Depth-tiered generation: leaves run cheap fast models; interior packages run
mid-tier; the root and cross-cutting pages run frontier models. Budgets are
per-repo per-day, enforced by the runner; a run reports its spend. Triggering
favors merges over commits; PR regeneration covers changed subtrees only.

## Faces (five-faces law, phased)

Core + CLI first (`glance run/check/diff/serve-local`). The deployed sites
are the UI face. API/MCP arrive when a consumer exists; the skill face
teaches agents to READ glance sites as pre-edit context. A later phase may
add an embedded Q&A face over the generated corpus (operator-flagged,
explicitly deferred).

## Fleet citizenship

Runs are triggered through bitterblossom (source-repo webhooks); health and
run failures report to canary; releases via landmark; site chrome uses the
aesthetic kit; work tracked in powder. Rust, tested, gated — standard
agent-readiness from day one.

## Non-goals

- Not a code browser or IDE — the site links INTO source; it does not render
  trees of code.
- Not an architecture-police or lint gate on the source repo.
- Not a universal visualization playground: the ten-prototype lab
  (2026-07-03) taught that containers without questions read as cute. Pages
  answer the standing questions (what is this / how does it flow / what
  breaks); novel interactives must earn their place per-page.
- No in-source-repo writes, ever.

## What excellent looks like (6 months)

Every misty-step repo has a live sister site the operator actually consults
during real work — PR review starts from the mirrored glance PR; onboarding
an agent to a repo starts with its glance root; staleness is impossible by
construction (regeneration is event-driven, citations are machine-verified,
and canary alarms when a site falls behind master).
