# Glance Image Briefs

Glance page specs never carry raw image URLs or final provider prompts. A model
may emit only `image_request` with:

- `intent`: what the illustration should explain.
- `emphasis`: a short list of concepts to emphasize.

Rust composes the final image prompt from deterministic run data: the real
top-level directory inventory from the source snapshot, flow edges declared in
the page spec, and the fixed house style suffix:

`clean labeled architecture illustration, Misty Step palette, no decorative clutter`

Eligibility:

- Root hero pages are eligible by default when an illustration clarifies the
  repository shape.
- Interior pages are eligible only when `flow_diagram` is insufficient for an
  important structure or interaction.
- Leaf pages never request generated images.

The renderer centers image figures, constrains them responsively, and keeps a
usable fallback when rendering is skipped, over budget, or provider-failed.
