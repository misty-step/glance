use std::path::{Path, PathBuf};

use glance_core::snapshot_tree;
use glance_gen::{
    Callout, CalloutKind, Callouts, CitationRef, Component, FileRow, FileRowKind, FileTable,
    FlowDiagram, FlowEdge, FlowNode, Hero, InlineNode, Narrative, PageKind, PageSpec,
    RenderContext, StatChip, render_page_spec,
};
use serde_json::json;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../glance-core/tests/fixtures/mini-source")
}

#[test]
fn spec_validation_rejects_bad_order_and_missing_file_table() {
    let missing_table = PageSpec {
        catalog_version: glance_gen::CATALOG_VERSION.to_owned(),
        title: "Bad page".to_owned(),
        components: vec![Component::Hero(Hero {
            title: "Bad page".to_owned(),
            summary: vec![InlineNode::Text {
                text: "No table.".to_owned(),
            }],
            stats: vec![
                StatChip {
                    label: "files".to_owned(),
                    value: "1".to_owned(),
                },
                StatChip {
                    label: "tier".to_owned(),
                    value: "leaf".to_owned(),
                },
            ],
            image_request: None,
        })],
    };

    let error = missing_table
        .validate_for_kind(PageKind::Leaf)
        .expect_err("file_table is required");
    assert!(error.to_string().contains("file_table"));

    let disclosure_before_table = PageSpec {
        catalog_version: glance_gen::CATALOG_VERSION.to_owned(),
        title: "Bad page".to_owned(),
        components: vec![
            Component::Hero(Hero {
                title: "Bad page".to_owned(),
                summary: vec![InlineNode::Text {
                    text: "Starts correctly.".to_owned(),
                }],
                stats: vec![
                    StatChip {
                        label: "files".to_owned(),
                        value: "1".to_owned(),
                    },
                    StatChip {
                        label: "tier".to_owned(),
                        value: "leaf".to_owned(),
                    },
                ],
                image_request: None,
            }),
            Component::Narrative(Narrative {
                heading: "Story".to_owned(),
                paragraphs: vec![vec![InlineNode::Text {
                    text: "Story before table.".to_owned(),
                }]],
            }),
            Component::Disclosure(glance_gen::Disclosure {
                heading: "Full context".to_owned(),
                children: Vec::new(),
            }),
            Component::FileTable(FileTable { rows: Vec::new() }),
        ],
    };

    let error = disclosure_before_table
        .validate_for_kind(PageKind::Leaf)
        .expect_err("disclosures are last");
    assert!(error.to_string().contains("disclosure"));
}

#[test]
fn renderer_outputs_nav_theme_story_citations_and_file_table_order() {
    let snapshot = snapshot_tree(fixture_root(), "fixture-sha").expect("snapshot");
    let spec = fixture_spec();

    let html = render_page_spec(
        &spec,
        &RenderContext {
            snapshot: &snapshot,
            directory: Path::new("."),
            source_sha: "fixture-sha",
            prompt_version: "test-prompt",
            kind: PageKind::Root,
        },
    )
    .expect("rendered html");

    let hero = html.find(r#"data-glance-component="hero""#).expect("hero");
    let narrative = html
        .find(r#"data-glance-component="narrative""#)
        .expect("narrative");
    let file_table = html
        .find(r#"data-glance-component="file_table""#)
        .expect("file table");
    let callouts = html
        .find(r#"data-glance-component="callouts""#)
        .expect("callouts");
    let disclosure = html
        .find(r#"data-glance-component="disclosure""#)
        .expect("disclosure");
    assert!(hero < narrative);
    assert!(narrative < callouts);
    assert!(callouts < file_table);
    assert!(file_table < disclosure);

    assert!(html.contains(r#"data-glance-catalog-version="glance-catalog-001""#));
    assert!(html.contains(r#"data-glance-directory=".""#));
    assert!(html.contains(r#"href="docs/index.html""#));
    assert!(html.contains(r#"href="src/index.html""#));
    assert!(html.contains(r#"data-theme-choice="system""#));
    assert!(html.contains("glance-citation-popover"));
    assert!(html.contains(r#"data-glance-cite="README.md:1-3""#));
    assert!(html.contains(r#">fixture repository</a>"#));
    assert!(!html.contains("[README.md:1-3]"));
    assert!(html.contains("pub fn answer() -&gt; u32"));
}

#[test]
fn renderer_draws_flow_svg_and_composes_structured_image_prompt() {
    let snapshot = snapshot_tree(fixture_root(), "fixture-sha").expect("snapshot");
    let mut spec = fixture_spec();
    if let Component::Hero(hero) = &mut spec.components[0] {
        hero.image_request = Some(glance_gen::ImageRequestSpec {
            intent: "Show the generated site shape.".to_owned(),
            emphasis: vec!["source tree".to_owned(), "citation gate".to_owned()],
        });
    }

    let html = render_page_spec(
        &spec,
        &RenderContext {
            snapshot: &snapshot,
            directory: Path::new("."),
            source_sha: "fixture-sha",
            prompt_version: "test-prompt",
            kind: PageKind::Root,
        },
    )
    .expect("rendered html");

    assert!(html.contains(r#"class="glance-flow-diagram""#));
    assert!(html.contains("glance-flow-pulse"));
    assert!(html.contains(r#"data-glance-image-prompt=""#));
    assert!(html.contains("top-level dirs: docs, src"));
    assert!(html.contains("edges: source -&gt; generated site"));
    assert!(html.contains("clean labeled architecture illustration"));
}

#[test]
fn spec_validation_recurses_into_disclosure_children() {
    let mut spec = fixture_spec();
    let Some(Component::Disclosure(disclosure)) = spec.components.last_mut() else {
        panic!("fixture keeps disclosure last");
    };
    disclosure.children = vec![Component::Narrative(Narrative {
        heading: "Invalid child".to_owned(),
        paragraphs: vec![vec![InlineNode::Cite {
            text: "bad cite".to_owned(),
            path: "src/lib.rs".to_owned(),
            lines: "not-lines".to_owned(),
        }]],
    })];

    let error = spec
        .validate_for_kind(PageKind::Root)
        .expect_err("invalid nested citation is rejected");

    assert!(error.to_string().contains("invalid citation"));
}

#[test]
fn spec_validation_rejects_non_root_hero_image_request() {
    let mut spec = fixture_spec();
    let Some(Component::Hero(hero)) = spec.components.first_mut() else {
        panic!("fixture keeps hero first");
    };
    hero.image_request = Some(glance_gen::ImageRequestSpec {
        intent: "Show the interior shape.".to_owned(),
        emphasis: Vec::new(),
    });

    let error = spec
        .validate_for_kind(PageKind::Interior)
        .expect_err("interior hero image request is rejected");

    assert!(error.to_string().contains("root pages"));
}

#[test]
fn spec_runtime_deserialize_rejects_unknown_and_missing_required_fields() {
    let mut unknown_field = serde_json::to_value(fixture_spec()).expect("fixture json");
    unknown_field
        .as_object_mut()
        .expect("page object")
        .insert("model_notes".to_owned(), json!("not in the catalog"));
    assert!(
        serde_json::from_value::<PageSpec>(unknown_field).is_err(),
        "provider-only fields must not survive runtime parsing"
    );

    let mut missing_signatures = serde_json::to_value(fixture_spec()).expect("fixture json");
    missing_signatures["components"][4]["rows"][0]
        .as_object_mut()
        .expect("file row object")
        .remove("signatures");
    assert!(
        serde_json::from_value::<PageSpec>(missing_signatures).is_err(),
        "file_table rows must provide signatures explicitly"
    );

    let mut missing_disclosure_children =
        serde_json::to_value(fixture_spec()).expect("fixture json");
    missing_disclosure_children["components"][5]
        .as_object_mut()
        .expect("disclosure object")
        .remove("children");
    assert!(
        serde_json::from_value::<PageSpec>(missing_disclosure_children).is_err(),
        "disclosures must provide children explicitly"
    );
}

#[test]
fn spec_validation_rejects_empty_file_table_roles() {
    let mut spec = fixture_spec();
    let Some(Component::FileTable(table)) = spec
        .components
        .iter_mut()
        .find(|component| matches!(component, Component::FileTable(_)))
    else {
        panic!("fixture includes file table");
    };
    table.rows[0].role.clear();

    let error = spec
        .validate_for_kind(PageKind::Root)
        .expect_err("empty roles are rejected");

    assert!(error.to_string().contains("role"));
}

fn fixture_spec() -> PageSpec {
    PageSpec {
        catalog_version: glance_gen::CATALOG_VERSION.to_owned(),
        title: "Mini Source".to_owned(),
        components: vec![
            Component::Hero(Hero {
                title: "Mini Source".to_owned(),
                summary: vec![
                    InlineNode::Text {
                        text: "Mini Source is a ".to_owned(),
                    },
                    InlineNode::Cite {
                        text: "fixture repository".to_owned(),
                        path: "README.md".to_owned(),
                        lines: "1-3".to_owned(),
                    },
                    InlineNode::Text {
                        text: " used to prove Glance behavior.".to_owned(),
                    },
                ],
                stats: vec![
                    StatChip {
                        label: "files".to_owned(),
                        value: "3".to_owned(),
                    },
                    StatChip {
                        label: "tier".to_owned(),
                        value: "root".to_owned(),
                    },
                ],
                image_request: None,
            }),
            Component::Narrative(Narrative {
                heading: "The story".to_owned(),
                paragraphs: vec![vec![InlineNode::Cite {
                    text: "The README names the fixture role.".to_owned(),
                    path: "README.md".to_owned(),
                    lines: "1-3".to_owned(),
                }]],
            }),
            Component::FlowDiagram(FlowDiagram {
                nodes: vec![
                    FlowNode {
                        id: "source".to_owned(),
                        label: "source".to_owned(),
                        kind: "tree".to_owned(),
                    },
                    FlowNode {
                        id: "site".to_owned(),
                        label: "generated site".to_owned(),
                        kind: "page".to_owned(),
                    },
                ],
                edges: vec![FlowEdge {
                    from: "source".to_owned(),
                    to: "site".to_owned(),
                    label: Some("renders".to_owned()),
                }],
                lanes: Vec::new(),
            }),
            Component::Callouts(Callouts {
                items: vec![Callout {
                    kind: CalloutKind::Hurt,
                    title: "Parser input is strict".to_owned(),
                    body: vec![InlineNode::Cite {
                        text: "The parser calls expect on numeric conversion.".to_owned(),
                        path: "src/parser/mod.rs".to_owned(),
                        lines: "1-3".to_owned(),
                    }],
                }],
            }),
            Component::FileTable(FileTable {
                rows: vec![
                    FileRow {
                        name: "src".to_owned(),
                        kind: FileRowKind::Dir,
                        role: "Rust source room.".to_owned(),
                        signatures: Vec::new(),
                        gotcha: None,
                        cite: None,
                    },
                    FileRow {
                        name: "src/lib.rs".to_owned(),
                        kind: FileRowKind::File,
                        role: "Exports the public answer function.".to_owned(),
                        signatures: vec!["pub fn answer() -> u32".to_owned()],
                        gotcha: None,
                        cite: Some(CitationRef {
                            path: "src/lib.rs".to_owned(),
                            lines: "1-5".to_owned(),
                        }),
                    },
                ],
            }),
            Component::Disclosure(glance_gen::Disclosure {
                heading: "Full context".to_owned(),
                children: vec![Component::Narrative(Narrative {
                    heading: "Source".to_owned(),
                    paragraphs: vec![vec![InlineNode::Text {
                        text: "Source details stay below the fold.".to_owned(),
                    }]],
                })],
            }),
        ],
    }
}
