//! Headless egui snapshots and real pointer interactions, using the shell's existing harness.
use super::*;
use obs_model::{CrossReference, Resource};
use obs_scripture::{Book, Passage, Verse};

struct ReferenceFiles(std::path::PathBuf);
impl Drop for ReferenceFiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn setup(
    name: &str,
    extra: bool,
) -> (
    Harness,
    ReferenceFiles,
    obs_engine::TabId,
    obs_engine::TabId,
) {
    let mut h = Harness::new();
    let dir = temp_db(name).parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&dir).unwrap();
    let files = ReferenceFiles(dir);
    h.shell.st.session.library = obs_engine::Library::new();
    let options = obs_formats::DatasetOptions {
        resource_id: "openbible".into(),
        title: "OpenBible.info".into(),
        ..Default::default()
    };
    let (mut references, _) = obs_formats::import_openbible_cross_references(
        include_str!("../../formats/tests/assets/openbible/cross-references-excerpt.txt"),
        &options,
    )
    .unwrap();
    if extra {
        for n in 17..21 {
            references.cross_references.push(CrossReference {
                from: john(16),
                to: john(n),
                votes: 21 - i32::from(n),
            });
        }
        references.cross_references.sort();
        references.manifest.content_hash = references.content_hash();
    }
    for resource in [obs_store::sample_resource(), references] {
        add_file(&mut h, &files, &resource);
    }
    h.shell.reset_layout();
    let bible = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("kjv-sample".into()))
        .unwrap()
        .id;
    h.shell
        .run("tab.go", &json!({"tab": bible.0, "text": "Jn 3:16"}));
    h.shell
        .run("tool.cross_references", &json!({"split": true}));
    let tool = h.shell.st.session.workspace.focused().unwrap();
    for _ in 0..30 {
        h.paced_frame();
        if h.find("John 11:25-26").is_some() {
            break;
        }
    }
    (h, files, bible, tool)
}

fn add_file(h: &mut Harness, files: &ReferenceFiles, resource: &Resource) {
    let path = files
        .0
        .join(format!("{}.obr", resource.manifest.resource_id));
    obs_store::write_obr(resource, &path).unwrap();
    h.shell.st.session.library.add_file(&path).unwrap();
}

fn john(n: u16) -> Passage {
    Passage::verse(Verse::new(Book::John, 3, n))
}

#[test]
fn cross_references_follow_john_3_16_ranked_snapshot_and_show_all() {
    let (mut h, _files, bible, tool) = setup("references-rows", true);
    assert_eq!(
        h.shell.session().workspace.tab(tool).unwrap().link,
        Some('A')
    );
    assert_eq!(
        h.shell
            .session()
            .workspace
            .tab(tool)
            .unwrap()
            .history
            .current(),
        Some(john(16).start())
    );
    let labels = [
        "John 11:25-26",
        "John 6:40",
        "1 Tim 1:15-16",
        "John 1:29",
        "Eph 2:4",
        "John 1:18",
        "John 3:17",
        "John 3:18",
    ];
    let snapshot = h
        .texts()
        .into_iter()
        .filter(|t| labels.contains(t))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(snapshot, labels.join("\n"));
    assert!(h.find("John 3:19").is_none());
    h.click_text("Show all");
    assert!(h.find("John 3:19").is_some());
    assert!(h.find("John 3:20").is_some());
    assert!(h.find("Show less").is_some());
    // A clicked Standard target drives the Tool's Link set and the Bible follows it.
    h.click_text("John 3:17");
    h.paced_frame();
    assert_eq!(
        h.shell
            .session()
            .workspace
            .tab(bible)
            .unwrap()
            .history
            .current(),
        Some(john(17).start())
    );
    assert_eq!(
        h.shell
            .session()
            .workspace
            .tab(tool)
            .unwrap()
            .history
            .current(),
        Some(john(17).start())
    );
    for _ in 0..5 {
        h.paced_frame();
    }
    assert!(
        h.texts()
            .iter()
            .any(|t| t.starts_with("None in your library"))
    );
    assert!(h.shell.st.tabs[&tool].expanded_references.is_empty());
}

#[test]
fn reference_new_tab_inherits_link_set_and_jumps_after_loading_verse_map() {
    let (mut h, _files, _bible, _tool) = setup("references-new-tab", true);
    let count = h.shell.session().workspace.tabs().count();
    h.modifiers = Modifiers::COMMAND;
    h.click_text("John 3:17");
    h.modifiers = Modifiers::NONE;
    h.frames(3);
    assert_eq!(h.shell.session().workspace.tabs().count(), count + 1);
    let new = h.shell.session().workspace.focused().unwrap();
    let tab = h.shell.session().workspace.tab(new).unwrap();
    assert_eq!(tab.kind, TabKind::Resource("kjv-sample".into()));
    assert_eq!(tab.link, Some('A'));
    assert_eq!(tab.history.current(), Some(john(17).start()));
}

#[test]
fn scripture_ref_and_zero_width_xref_hover_draw_worker_previews() {
    let (mut h, files, bible, _tool) = setup("references-popup", false);
    let mut resource = obs_store::sample_resource();
    let unit = resource
        .units
        .iter_mut()
        .find(|u| u.id == obs_model::UnitId::book(Book::John))
        .unwrap();
    unit.spans.retain(|s| {
        !matches!(
            s.kind,
            obs_model::SpanKind::Xref { .. } | obs_model::SpanKind::ScriptureRef { .. }
        )
    });
    unit.spans.push(obs_model::Span {
        start: 0,
        end: 3,
        kind: obs_model::SpanKind::ScriptureRef {
            targets: vec![john(17)].into(),
        },
    });
    unit.spans.push(obs_model::Span {
        start: 8,
        end: 8,
        kind: obs_model::SpanKind::Xref {
            targets: vec![john(16)].into(),
        },
    });
    resource.manifest.content_hash = resource.content_hash();
    // The read-only fixture is replaced before any preview opens it.
    add_file(&mut h, &files, &resource);
    h.shell.st.docs.insert(
        "kjv-sample".into(),
        Arc::new(ReadingDoc::from_resource(resource)),
    );
    h.shell.st.opened(bible);
    h.shell.st.session.workspace.focus(bible);
    h.shell.apply_effects();
    h.ctx.enable_accesskit();
    h.frames(3);
    for label in ["John 3:17", "John 3:16"] {
        let view = h.shell.st.tabs[&bible].view.as_ref().unwrap();
        let [ox, oy] = view.text_origin;
        let mut spot = None;
        for (_, top, laid) in view.laid_in_view(800.0) {
            for line in &laid.lines {
                for item in &line.items {
                    for (bytes, x0, x1) in item.clusters() {
                        let wanted = match laid.to_unit(bytes.start) {
                            Some(obs_layout::UnitPos::Text(0)) => label == "John 3:17",
                            Some(obs_layout::UnitPos::Object {
                                object: obs_layout::InlineObject::ScriptureReference,
                                ..
                            }) => label == "John 3:16",
                            _ => false,
                        };
                        if wanted {
                            spot = Some(pos2(
                                ox + (x0 + x1) / 2.0,
                                oy + top + (line.top + line.bottom) / 2.0,
                            ));
                        }
                    }
                }
            }
        }
        h.events
            .push(Event::PointerMoved(spot.expect("reference glyph")));
        for _ in 0..10 {
            h.paced_frame();
        }
        assert!(h.find(label).is_some(), "{:?}", h.texts());
        assert!(h.find("★ KJV").is_some(), "{:?}", h.texts());
        assert!(h.meshes.contains(&h.shell.st.palette.ink));
        let expected = if label == "John 3:17" {
            "For God sent not"
        } else {
            "For God so loved"
        };
        assert!(
            h.tree
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .any(|(_, node)| node.role() == egui::Role::Label
                    && node.value().is_some_and(|text| text.starts_with(expected))),
            "popup exposes laid-out preview text"
        );
    }
}
