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
    for _ in 0..600 {
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

/// Clicks a Scripture reference and waits for the click to land. A click on a cold target
/// cache is held in `pending_clicks` until the worker translates the targets, so a fixed
/// frame count races the worker on slow runners.
fn click_reference(h: &mut Harness, spot: Pos2) {
    h.click(spot);
    for _ in 0..600 {
        if h.shell.st.references.pending_clicks.is_empty() {
            h.frames(2);
            return;
        }
        h.paced_frame();
    }
    panic!("reference click never resolved its targets");
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
    let none_in_library = |h: &Harness| {
        h.texts()
            .iter()
            .any(|t| t.starts_with("None in your library"))
    };
    for _ in 0..600 {
        h.paced_frame();
        if none_in_library(&h) {
            break;
        }
    }
    assert!(none_in_library(&h));
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
    let (mut h, files, bible, tool) = setup("references-popup", false);
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
    let reference_start = unit.spans.len();
    for (start, end, target) in [(0, 3, 17), (8, 8, 16), (8, 8, 17), (0, 0, 16)] {
        unit.spans.push(obs_model::Span {
            start,
            end,
            kind: if start == 8 {
                obs_model::SpanKind::Xref {
                    targets: vec![john(target)].into(),
                }
            } else {
                obs_model::SpanKind::ScriptureRef {
                    targets: vec![john(target)].into(),
                }
            },
        });
    }
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
    for (offset, label) in ["John 3:17", "John 3:16", "John 3:17", "John 3:16"]
        .into_iter()
        .enumerate()
    {
        let view = h.shell.st.tabs[&bible].view.as_ref().unwrap();
        let [ox, oy] = view.text_origin;
        let mut spot = None;
        for (_, top, laid) in view.laid_in_view(800.0) {
            for line in &laid.lines {
                for item in &line.items {
                    for (bytes, x0, x1) in item.clusters() {
                        let wanted = match laid.to_unit(bytes.start) {
                            Some(obs_layout::UnitPos::Text(0)) => offset == 0,
                            Some(obs_layout::UnitPos::Text(8)) => {
                                assert_eq!(
                                    view.reference_at(
                                        (x0 + x1) / 2.0,
                                        top + (line.top + line.bottom) / 2.0
                                    ),
                                    None,
                                    "following text is not a zero-width reference"
                                );
                                false
                            }
                            Some(obs_layout::UnitPos::Object {
                                object: obs_layout::InlineObject::ScriptureReference { span },
                                ..
                            }) => span == reference_start + offset,
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
        let spot = spot.expect("reference glyph");
        assert_eq!(
            view.reference_at(spot.x - ox, spot.y - oy).unwrap().1,
            reference_start + offset
        );
        h.events.push(Event::PointerMoved(spot));
        let expected = if label == "John 3:17" {
            "For God sent not"
        } else {
            "For God so loved"
        };
        let preview_shown = |h: &Harness| {
            h.tree.as_ref().is_some_and(|tree| {
                tree.nodes.iter().any(|(_, node)| {
                    node.role() == egui::Role::Label
                        && node.value().is_some_and(|text| text.starts_with(expected))
                })
            })
        };
        // The preview comes from the worker: wait for it rather than a fixed frame count.
        for _ in 0..600 {
            h.paced_frame();
            if preview_shown(&h) {
                break;
            }
        }
        assert!(h.find(label).is_some(), "{:?}", h.texts());
        assert!(
            h.tree.as_ref().unwrap().nodes.iter().any(|(_, node)| {
                node.role() == egui::accesskit::Role::Link
                    && node.description() == Some(format!("Go to {label}").as_str())
                    && (offset == 0 || node.label() == Some(label))
                    && node.bounds().is_some_and(|bounds| {
                        bounds.x0 <= f64::from(spot.x)
                            && f64::from(spot.x) <= bounds.x1
                            && bounds.y0 <= f64::from(spot.y)
                            && f64::from(spot.y) <= bounds.y1
                    })
            }),
            "reference has its own accessible Link around its glyph"
        );
        assert!(h.find("★ KJV").is_some(), "{:?}", h.texts());
        assert!(h.meshes.contains(&h.shell.st.palette.ink));
        assert!(preview_shown(&h), "popup exposes laid-out preview text");
        click_reference(&mut h, spot);
        assert_eq!(
            h.shell
                .session()
                .workspace
                .tab(tool)
                .unwrap()
                .history
                .current(),
            Some(john(if label == "John 3:17" { 17 } else { 16 }).start()),
            "click navigates the same target as the preview"
        );
        h.shell
            .run("tab.go", &json!({"tab": bible.0, "text": "Jn 3:16"}));
        h.frames(3);
    }
}

#[test]
fn reference_clicks_preserve_standard_target_with_non_reversible_maps() {
    use obs_scripture::{Standard, VerseMap, VerseMapEntry};
    for case in ["absent", "merged", "no-preceding"] {
        for new_tab in [false, true] {
            let name = format!("references-{case}-{new_tab}");
            let (mut h, _files, bible, tool) = setup(&name, true);
            let native = john(if case == "no-preceding" { 18 } else { 16 }).start();
            let map = VerseMap::new(
                vec![VerseMapEntry {
                    verse: native,
                    last: native.verse,
                    standard: Some(if case == "merged" {
                        Passage::new(native, john(17).start()).unwrap()
                    } else {
                        Passage::verse(native)
                    }),
                }],
                if case == "merged" {
                    vec![]
                } else {
                    vec![john(17).start()]
                },
                &Standard,
            );
            let destination = if new_tab {
                // The existing Bible remains identity-numbered. The newly loaded preferred
                // Bible has an Absent/merged target, so broadcasting its native Verse is wrong.
                let mut resource = obs_store::sample_resource();
                resource.verse_map = Some(map);
                h.shell.st.docs.insert(
                    "kjv-sample".into(),
                    Arc::new(ReadingDoc::from_resource(resource)),
                );
                h.modifiers = Modifiers::COMMAND;
                h.click_text("John 3:17");
                h.modifiers = Modifiers::NONE;
                h.shell.session().workspace.focused().unwrap()
            } else {
                h.shell
                    .st
                    .session
                    .workspace
                    .tab_mut(bible)
                    .unwrap()
                    .verse_map = Some(Arc::new(map));
                h.shell.st.reference_navigate(bible, john(17), false);
                h.shell.apply_effects();
                bible
            };
            h.frames(3);
            let ws = &h.shell.session().workspace;
            let destination = ws.tab(destination).unwrap();
            assert_eq!(destination.link, Some('A'));
            // No preceding Verse leaves a new view at its first Verse; an existing view
            // stays at its location. Neither fallback becomes the Link set's target.
            let expected = if case == "no-preceding" && new_tab {
                Verse::new(Book::Psalms, 3, 0)
            } else {
                john(16).start()
            };
            assert_eq!(destination.history.current(), Some(expected), "{name}");
            assert_eq!(
                ws.tab(tool).unwrap().history.current(),
                Some(john(17).start()),
                "{name}"
            );
            if new_tab {
                assert_eq!(
                    ws.tab(bible).unwrap().history.current(),
                    Some(john(17).start()),
                    "{name}"
                );
            }
        }
    }
}

pub(super) fn reference_spot(h: &Harness, tab: obs_engine::TabId, span: usize) -> Pos2 {
    let view = h.shell.st.tabs[&tab].view.as_ref().unwrap();
    let [ox, oy] = view.text_origin;
    for (_, top, laid) in view.laid_in_view(800.0) {
        for line in &laid.lines {
            for item in &line.items {
                for (_, x0, x1) in item.clusters() {
                    let x = (x0 + x1) / 2.0;
                    let y = top + (line.top + line.bottom) / 2.0;
                    if view.reference_at(x, y).is_some_and(|(_, s)| s == span) {
                        return pos2(ox + x, oy + y);
                    }
                }
            }
        }
    }
    panic!("reference glyph not laid out");
}

#[test]
fn linked_lexicon_reference_click_moves_bible_without_moving_lexicon() {
    let (mut h, files, bible, tool) = setup("references-lexicon", false);
    let (resource, _) = obs_formats::import_abbott_smith(
        &obs_formats::LexiconOptions {
            resource_id: "reference-lexicon".into(),
            ..Default::default()
        },
        r#"<TEI><body><entry n="test|G1"><ref osisRef="John.3.17">John 3:17</ref></entry></body></TEI>"#,
    )
    .unwrap();
    let span = resource.units[0]
        .spans
        .iter()
        .position(|s| matches!(s.kind, obs_model::SpanKind::ScriptureRef { .. }))
        .unwrap();
    add_file(&mut h, &files, &resource);
    h.shell.session_mut().workspace.focus(bible);
    h.shell
        .run("resource.open", &json!({"id": "reference-lexicon"}));
    let lexicon = h.shell.session().workspace.focused().unwrap();
    h.frames(3);
    let tab = h.shell.session().workspace.tab(lexicon).unwrap();
    assert_eq!(tab.link, Some('A'));
    assert_eq!(
        tab.key_schemes,
        [
            obs_model::KeyScheme::StrongsNumber,
            obs_model::KeyScheme::Lemma
        ]
    );
    let location = tab.history.current();
    let count = h.shell.session().workspace.tabs().count();
    let spot = reference_spot(&h, lexicon, span);
    click_reference(&mut h, spot);
    let ws = &h.shell.session().workspace;
    assert_eq!(
        ws.tab(bible).unwrap().history.current(),
        Some(john(17).start())
    );
    assert_eq!(
        ws.tab(tool).unwrap().history.current(),
        Some(john(17).start())
    );
    assert_eq!(ws.tab(lexicon).unwrap().history.current(), location);
    assert_eq!(ws.tabs().count(), count);
}

#[test]
fn descending_scripture_reference_click_keeps_first_written_target() {
    let (mut h, _files, bible, tool) = setup("references-descending", false);
    let mut resource = obs_store::sample_resource();
    resource.verse_map = Some(obs_scripture::VerseMap::new(
        [16, 17]
            .map(|n| obs_scripture::VerseMapEntry {
                verse: john(n).start(),
                last: n,
                standard: Some(john(n)),
            })
            .into(),
        vec![],
        &obs_scripture::Standard,
    ));
    let unit = resource
        .units
        .iter_mut()
        .find(|u| u.id == obs_model::UnitId::book(Book::John))
        .unwrap();
    unit.spans.push(obs_model::Span {
        start: 0,
        end: 3,
        kind: obs_model::SpanKind::ScriptureRef {
            targets: vec![john(17), john(16)].into(),
        },
    });
    let span = unit.spans.len() - 1;
    h.shell.st.docs.insert(
        "kjv-sample".into(),
        Arc::new(ReadingDoc::from_resource(resource)),
    );
    h.shell.st.opened(bible);
    h.shell.st.session.workspace.focus(bible);
    h.shell.apply_effects();
    h.frames(3);
    let spot = reference_spot(&h, bible, span);
    click_reference(&mut h, spot);
    for tab in [bible, tool] {
        assert_eq!(
            h.shell
                .session()
                .workspace
                .tab(tab)
                .unwrap()
                .history
                .current(),
            Some(john(17).start())
        );
    }
}

#[test]
fn range_and_split_map_reference_popups_keep_one_preview_per_written_target() {
    let (mut h, files, bible, _tool) = setup("references-range-popup", false);
    let range = Passage::new(john(16).start(), john(17).start()).unwrap();
    // An identity-mapped range stays one target; a Standard Verse split across two native
    // Verses previews once. Neither becomes per-Verse targets that the popup limit truncates.
    for (revision, standard_17, heading) in
        [(2, john(17), "John 3:16-17"), (3, john(16), "John 3:16")]
    {
        let mut resource = obs_store::sample_resource();
        let map = resource.verse_map.take().unwrap();
        let entries = map
            .entries()
            .iter()
            .map(|e| obs_scripture::VerseMapEntry {
                standard: (e.verse != john(17).start())
                    .then_some(e.standard)
                    .flatten()
                    .or(Some(standard_17)),
                ..*e
            })
            .collect();
        resource.verse_map = Some(obs_scripture::VerseMap::new(
            entries,
            map.absent().to_vec(),
            &obs_scripture::Standard,
        ));
        let unit = resource
            .units
            .iter_mut()
            .find(|u| u.id == obs_model::UnitId::book(Book::John))
            .unwrap();
        unit.spans.push(obs_model::Span {
            start: 0,
            end: 3,
            kind: obs_model::SpanKind::ScriptureRef {
                targets: vec![range].into(),
            },
        });
        let span = unit.spans.len() - 1;
        // Previews read the library file through its own Verse map, so replace it as well.
        resource.manifest.revision = revision;
        resource.manifest.content_hash = resource.content_hash();
        add_file(&mut h, &files, &resource);
        h.shell.st.docs.insert(
            "kjv-sample".into(),
            Arc::new(ReadingDoc::from_resource(resource)),
        );
        h.shell.st.opened(bible);
        h.shell.st.session.workspace.focus(bible);
        h.shell.apply_effects();
        h.ctx.enable_accesskit();
        h.events.push(Event::PointerMoved(pos2(1.0, 1.0)));
        h.frames(3);
        // The replaced file reloads the Cross-references Tool: settle before counting
        // headings so its reappearing "John 3:16" group never races the popup's.
        for _ in 0..600 {
            if h.find("Loading Cross-references…").is_none() && h.find("John 11:25-26").is_some()
            {
                break;
            }
            h.paced_frame();
        }
        let headings = |h: &Harness| h.texts().iter().filter(|t| **t == heading).count();
        let preview_texts = |h: &Harness| -> Vec<String> {
            h.tree
                .as_ref()
                .into_iter()
                .flat_map(|tree| tree.nodes.iter())
                .filter(|(_, node)| node.role() == egui::Role::Label)
                .filter_map(|(_, node)| node.value().map(str::to_string))
                .filter(|text| text.starts_with("For God"))
                .collect()
        };
        let before = headings(&h);
        let spot = reference_spot(&h, bible, span);
        h.events.push(Event::PointerMoved(spot));
        // Hold the hover until the worker's target translation and preview both land.
        for _ in 0..600 {
            h.paced_frame();
            if h.find("Loading Scripture reference…").is_none() && !preview_texts(&h).is_empty() {
                break;
            }
        }
        assert_eq!(headings(&h), before + 1, "{heading}: {:?}", h.texts());
        assert!(h.find("Loading Scripture reference…").is_none());
        assert!(
            h.find("Preview shortened — click the reference to read")
                .is_none()
        );
        let previews = preview_texts(&h);
        assert_eq!(previews.len(), 1, "{heading}: {previews:?}");
        assert!(previews[0].starts_with("For God so loved"));
        assert!(previews[0].contains("For God sent not"), "{}", previews[0]);
    }
}
