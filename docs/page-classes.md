# Generated page class contract

Glance pages are self-contained HTML documents. A provider may choose the visual
style, but these semantic hooks are stable because context assembly and checks
depend on them.

## Document

- `.glance-page` on the page body or main document shell.
- `.glance-header` for the page title and source metadata.
- `.glance-section` on every contract section.
- `.glance-section-title` on visible section headings.
- `.glance-cited` on elements that make factual claims and carry
  `data-glance-cite`.

## Required sections

Every page must include one element for each section below. Use both the class
and `data-glance-section` value so parsers have a stable target.

| Section | Required selector |
| --- | --- |
| What this is | `.glance-section[data-glance-section="what-this-is"]` |
| Role in the whole | `.glance-section[data-glance-section="role-in-the-whole"]` |
| Composition | `.glance-section[data-glance-section="composition"]` |
| Seams and contracts | `.glance-section[data-glance-section="seams-contracts"]` |
| Where it can hurt you | `.glance-section[data-glance-section="where-it-can-hurt-you"]` |

Root pages must also include:

| Section | Required selector |
| --- | --- |
| Flows | `.glance-section[data-glance-section="flows"]` |
| Data model | `.glance-section[data-glance-section="data-model"]` |
| Failure-edge index | `.glance-section[data-glance-section="failure-edge-index"]` |

## Composition hooks

- `.glance-composition` wraps the child directory list.
- `.glance-child-link` is the link to a child `index.html`.
- `.glance-child-role` is the one-line role text for that child.

## Earned extras

- `.glance-diagram` marks the single allowed inline SVG diagram.
- `.glance-interactive` marks the single allowed self-contained interactive.

Do not add decorative widgets. A diagram is valid only when shape carries
meaning that prose cannot carry. An interactive is valid only when operating it
teaches behavior that static prose cannot teach. The default is neither.
