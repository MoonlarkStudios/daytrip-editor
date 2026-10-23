//! Application model differential and offline delivery tests.
use super::*;
use crate::document::collaboration::{AgentSnapshot, MAX_STATE_BYTES, ProposedEdit};
use crate::document::projection::Projection;
use yrs::{Assoc, ReadTxn, StateVector, Text, Transact};

const RICH: &str = "# 日本語 e\u{301} 🧑‍💻\r\n\r\n| A | B |\n|---|---|\n| café | שלום |\n\n![alt](image.png)\r\n\r\nLast paragraph.\n";
fn oracle(document: &Document) {
    assert_eq!(document.source(), document.collaboration.materialize());
    let fresh = Document::new(document.source().to_owned());
    assert_eq!(document.words, fresh.words);
    assert_eq!(document.characters, fresh.characters);
    assert_eq!(
        format!(
            "{:?}",
            document
                .blocks
                .iter()
                .map(|block| &block.rendered)
                .collect::<Vec<_>>()
        ),
        format!(
            "{:?}",
            fresh
                .blocks
                .iter()
                .map(|block| &block.rendered)
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(
        document
            .blocks
            .iter()
            .map(|block| (&block.range, &block.heading))
            .collect::<Vec<_>>(),
        fresh
            .blocks
            .iter()
            .map(|block| (&block.range, &block.heading))
            .collect::<Vec<_>>()
    );
}
fn edit(document: &mut Document, range: Range<usize>, text: &str, origin: EditOrigin) {
    document
        .transact(
            &[SourceEdit {
                range,
                text: text.into(),
            }],
            origin,
            None,
            true,
        )
        .unwrap();
    oracle(document);
}
fn proposal(snapshot: AgentSnapshot, range: Range<usize>, text: &str) -> AgentProposal {
    AgentProposal {
        author: "Test agent".into(),
        edits: vec![ProposedEdit {
            start: range.start,
            end: range.end,
            expected: snapshot.source[range].into(),
            replacement: text.into(),
        }],
        base: snapshot,
    }
}

#[test]
fn three_replicas_converge_after_concurrent_replacements_and_reordered_duplicate_delivery() {
    let mut a = Document::new(RICH.into());
    let initial = a.collaboration.state();
    let mut b = Document::from_state(&initial).unwrap();
    let mut c = Document::from_state(&initial).unwrap();
    let ids = [
        a.collaboration.replica_id(),
        b.collaboration.replica_id(),
        c.collaboration.replica_id(),
    ];
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[1], ids[2]);
    assert_ne!(ids[0], ids[2]);
    let clock = a.collaboration.doc_for_test().transact().state_vector();
    edit(&mut a, 0..0, "A ", EditOrigin::Human);
    edit(&mut b, 0..1, "##", EditOrigin::Human);
    let start = c.source().find("Last").unwrap();
    edit(&mut c, start..start + 4, "Final", EditOrigin::Agent);
    // Two incremental updates by one author, deliver the dependent one first.
    let b1 = b.collaboration.diff_for_test(&clock);
    let bclock = b.collaboration.doc_for_test().transact().state_vector();
    edit(&mut b, 2..2, "B", EditOrigin::Human);
    let b2 = b.collaboration.diff_for_test(&bclock);
    let au = a.collaboration.state();
    let cu = c.collaboration.state();
    a.receive(&b2).unwrap();
    let pending = a.collaboration.state();
    let mut reopened_pending = Document::from_state(&pending).unwrap();
    assert_eq!(reopened_pending.source(), a.source());
    reopened_pending.receive(&b1).unwrap();
    a.receive(&cu).unwrap();
    a.receive(&b1).unwrap();
    a.receive(&b2).unwrap();
    b.receive(&cu).unwrap();
    b.receive(&au).unwrap();
    b.receive(&au).unwrap();
    c.receive(&b2).unwrap();
    c.receive(&au).unwrap();
    c.receive(&b1).unwrap();
    for document in [&a, &b, &c] {
        oracle(document);
        assert_eq!(document.source(), a.source());
    }
    let reopened = Document::from_state(&a.collaboration.state()).unwrap();
    assert_ne!(
        reopened.collaboration.replica_id(),
        a.collaboration.replica_id()
    );
    assert_eq!(reopened.collaboration.id(), a.collaboration.id());
    assert_eq!(reopened.source(), a.source());
}

#[test]
fn original_state_batch_offsets_are_validated_before_any_mutation() {
    let mut document = Document::new(RICH.into());
    let end = document.source().find("Last").unwrap();
    document
        .transact(
            &[
                SourceEdit {
                    range: 0..1,
                    text: "##".into(),
                },
                SourceEdit {
                    range: end..end + 4,
                    text: "Final".into(),
                },
            ],
            EditOrigin::Agent,
            None,
            true,
        )
        .unwrap();
    oracle(&document);
    assert!(document.source().starts_with("## 日本"));
    assert!(document.source().contains("Final paragraph"));
    let before = document.source().to_owned();
    for edits in [
        vec![
            SourceEdit {
                range: 1..3,
                text: "x".into(),
            },
            SourceEdit {
                range: 2..4,
                text: "y".into(),
            },
        ],
        vec![SourceEdit {
            range: 4..5,
            text: "x".into(),
        }],
        vec![
            SourceEdit {
                range: 0..0,
                text: "x".into(),
            },
            SourceEdit {
                range: 0..0,
                text: "y".into(),
            },
        ],
    ] {
        assert!(
            document
                .transact(&edits, EditOrigin::Human, None, true)
                .is_err()
        );
        assert_eq!(document.source(), before);
        oracle(&document);
    }
}

#[test]
fn stable_selections_keep_direction_affinity_unicode_and_deleted_target_gaps() {
    let mut document = Document::new(RICH.into());
    let start = RICH.find("日本").unwrap();
    let end = RICH.find("Last").unwrap();
    let forward = document.collaboration.selection(Selection {
        anchor: start,
        head: end,
    });
    let reverse = document.collaboration.selection(Selection {
        anchor: end,
        head: start,
    });
    let left = document.collaboration.position(start, Assoc::Before);
    let right = document.collaboration.position(start, Assoc::After);
    edit(&mut document, start..start, "λ", EditOrigin::Agent);
    assert_eq!(document.collaboration.resolve(&left), Some(start));
    assert_eq!(document.collaboration.resolve(&right), Some(start + 2));
    assert_eq!(
        document.collaboration.resolve_selection(&forward),
        Some(Selection {
            anchor: start + 2,
            head: end + 2
        })
    );
    assert_eq!(
        document.collaboration.resolve_selection(&reverse),
        Some(Selection {
            anchor: end + 2,
            head: start + 2
        })
    );
    let target = document.collaboration.position(start + 2, Assoc::After);
    edit(&mut document, start..end + 2, "", EditOrigin::Human);
    assert_eq!(document.collaboration.resolve(&target), Some(start));
    for (offset, _) in document
        .source()
        .char_indices()
        .chain(std::iter::once((document.source().len(), '\0')))
    {
        for assoc in [Assoc::Before, Assoc::After] {
            assert_eq!(
                document
                    .collaboration
                    .resolve(&document.collaboration.position(offset, assoc)),
                Some(offset)
            );
        }
    }
}

#[test]
fn origin_scoped_undo_redo_survives_intervening_remote_and_agent_edits() {
    let mut document = Document::new("alpha beta gamma".into());
    let mut remote = Document::from_state(&document.collaboration.state()).unwrap();
    document
        .replace_edit(
            6..10,
            "BETA",
            Selection {
                anchor: 10,
                head: 6,
            },
            Selection::caret(10),
        )
        .unwrap();
    edit(&mut remote, 0..0, "remote ", EditOrigin::Human);
    document.receive(&remote.collaboration.state()).unwrap();
    edit(&mut document, 17..17, " agent", EditOrigin::Agent);
    assert_eq!(
        document.undo(),
        Some(Selection {
            anchor: 17,
            head: 13
        })
    );
    assert_eq!(document.source(), "remote alpha beta agent gamma");
    // Agent undo is independent and cannot undo the remote author.
    document.history(EditOrigin::Agent, false).unwrap();
    assert_eq!(document.source(), "remote alpha beta gamma");
    document.redo().unwrap();
    assert_eq!(document.source(), "remote alpha BETA gamma");
    document.history(EditOrigin::Agent, true).unwrap();
    assert_eq!(document.source(), "remote alpha BETA agent gamma");
    oracle(&document);
}

#[test]
fn versioned_proposals_allow_unrelated_edits_and_reject_overlap_and_stale_gaps() {
    let mut document =
        Document::new("First paragraph.\n\nSecond paragraph.\n\nThird paragraph.".into());
    let snapshot = document.collaboration.snapshot(document.source());
    let start = snapshot.source.find("Second").unwrap();
    let accepted = proposal(snapshot.clone(), start..start + 6, "Better");
    edit(&mut document, 0..0, "# Title\n\n", EditOrigin::Human);
    document.accept(&accepted).unwrap();
    assert!(document.source().contains("Better paragraph"));
    assert!(document.accept(&accepted).is_err()); // never double apply an accepted proposal
    let gap = proposal(snapshot.clone(), start..start, "New ");
    assert!(document.accept(&gap).is_err());
    let stale = proposal(snapshot, 0..0, "Prefix ");
    assert!(document.accept(&stale).is_err());
    oracle(&document);
    document.history(EditOrigin::Agent, false).unwrap();
    assert!(document.source().contains("Second paragraph"));
}

#[test]
fn disclosure_is_local_over_identical_replicated_source() {
    let a = Document::new(RICH.into());
    let b = Document::from_state(&a.collaboration.state()).unwrap();
    let active = Projection::with_context(a.source(), Some(2));
    let inactive = Projection::new(b.source());
    assert_ne!(active.text, inactive.text);
    assert_eq!(a.source(), b.source());
    assert_eq!(
        a.collaboration.state().update,
        b.collaboration.state().update
    );
}

#[test]
fn randomized_three_replica_delivery_matches_fresh_source_and_parser_oracles() {
    let base = Document::new(RICH.into()).collaboration.state();
    let mut replicas: Vec<_> = (0..3)
        .map(|_| Document::from_state(&base).unwrap())
        .collect();
    let mut seed = 0x517cc1b727220a95u64;
    let mut updates = Vec::new();
    for turn in 0..90 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let index = seed as usize % 3;
        let boundaries: Vec<_> = replicas[index]
            .source()
            .char_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(replicas[index].source().len()))
            .collect();
        let point = seed.rotate_left(17) as usize % boundaries.len();
        let end = (point + usize::from(turn % 3 == 0)).min(boundaries.len() - 1);
        edit(
            &mut replicas[index],
            boundaries[point]..boundaries[end],
            ["x", "é", "🧑‍💻", "\r\n", "שלום"][turn % 5],
            EditOrigin::Human,
        );
        updates.push(replicas[index].collaboration.state());
        if turn % 4 == 0 {
            let target = (index + 1) % 3;
            replicas[target]
                .receive(&updates[seed.rotate_left(31) as usize % updates.len()])
                .unwrap();
            oracle(&replicas[target]);
        }
    }
    // Full states are batched updates; reverse delivery includes old and duplicated states.
    for update in updates.iter().rev().chain(updates.iter().step_by(5)) {
        for replica in &mut replicas {
            replica.receive(update).unwrap();
        }
    }
    for replica in &replicas {
        oracle(replica);
        assert_eq!(replica.source(), replicas[0].source());
    }
}

#[test]
fn invalid_oversized_and_foreign_updates_leave_authority_untouched() {
    let mut document = Document::new("text".into());
    let before = document.collaboration.state();
    for update in [vec![], vec![255], vec![0; MAX_STATE_BYTES + 1]] {
        assert!(
            document
                .receive(&ReplicaState {
                    document: before.document,
                    update
                })
                .is_err()
        );
        assert_eq!(document.source(), "text");
        oracle(&document);
    }
    assert!(
        document
            .receive(&Document::new("same text".into()).collaboration.state())
            .is_err()
    );
    // A valid Yrs update containing another root is rejected before application.
    let doc = yrs::Doc::new();
    doc.get_or_insert_text("other")
        .insert(&mut doc.transact_mut(), 0, "bad");
    let update = doc
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    assert!(
        document
            .receive(&ReplicaState {
                document: before.document,
                update
            })
            .is_err()
    );
    assert_eq!(document.collaboration.state().update, before.update);
}

#[test]
fn equal_length_disjoint_batches_invalidate_changed_renderings() {
    let mut document = Document::new("# Title\n\nFirst paragraph.\n\nLast paragraph.".into());
    let last = document.source().find("Last").unwrap();
    document
        .transact(
            &[
                SourceEdit {
                    range: 2..7,
                    text: "Other".into(),
                },
                SourceEdit {
                    range: last..last + 4,
                    text: "Tail".into(),
                },
            ],
            EditOrigin::Agent,
            None,
            true,
        )
        .unwrap();
    oracle(&document);
    document.history(EditOrigin::Agent, false).unwrap();
    oracle(&document);
}

#[test]
fn human_redo_preserves_remote_edits_received_after_undo() {
    let mut document = Document::new("alpha beta".into());
    let mut remote = Document::from_state(&document.collaboration.state()).unwrap();
    document.replace(6..10, "BETA").unwrap();
    document.undo().unwrap();
    remote.replace(0..0, "remote ").unwrap();
    document.receive(&remote.collaboration.state()).unwrap();
    document.redo().unwrap();
    assert_eq!(document.source(), "remote alpha BETA");
    oracle(&document);
}

#[test]
fn reopening_and_cloning_never_reuses_the_saved_writer_identity() {
    let document = Document::new(RICH.into());
    let state = document.collaboration.state();
    let mut ids = std::collections::HashSet::from([document.collaboration.replica_id()]);
    for _ in 0..64 {
        let reopened = Document::from_state(&state).unwrap();
        assert!(ids.insert(reopened.collaboration.replica_id()));
        assert_eq!(reopened.collaboration.id(), document.collaboration.id());
    }
}

#[test]
fn pending_delete_only_updates_survive_reopen_before_their_insert_arrives() {
    let mut a = Document::new("base".into());
    let mut b = Document::from_state(&a.collaboration.state()).unwrap();
    let before = b.collaboration.doc_for_test().transact().state_vector();
    b.replace(4..4, "X").unwrap();
    let insertion = b.collaboration.diff_for_test(&before);
    let before = b.collaboration.doc_for_test().transact().state_vector();
    b.replace(4..5, "").unwrap();
    let deletion = b.collaboration.diff_for_test(&before);
    a.receive(&deletion).unwrap();
    let mut reopened = Document::from_state(&a.collaboration.state()).unwrap();
    reopened.receive(&insertion).unwrap();
    assert_eq!(reopened.source(), "base");
    oracle(&reopened);
}

#[test]
fn replace_all_keeps_thousands_of_disjoint_matches_in_one_undo_group() {
    let source = "word ".repeat(4200);
    let mut document = Document::new(source.clone());
    assert_eq!(document.replace_all("word", "term").unwrap(), 4200);
    assert_eq!(document.source(), "term ".repeat(4200));
    oracle(&document);
    document.undo().unwrap();
    assert_eq!(document.source(), source);
    oracle(&document);
    document.redo().unwrap();
    assert_eq!(document.source(), "term ".repeat(4200));
    oracle(&document);
}

#[test]
fn fragmented_unicode_items_keep_byte_anchors_history_and_reopen_compatibility() {
    let source = RICH.repeat(32);
    let mut document = Document::new(source.clone());
    // Split the original large item at disjoint scalar boundaries. Both inserted
    // and retained spans contain unequal UTF-8/UTF-16 lengths.
    let edits: Vec<_> = source
        .match_indices("Last")
        .map(|(point, _)| SourceEdit {
            range: point..point + 4,
            text: "新🦀e\u{301}".into(),
        })
        .collect();
    document
        .transact(&edits, EditOrigin::Human, None, true)
        .unwrap();
    oracle(&document);
    let changed = document.source().to_owned();
    let mut peer = Document::from_state(&document.collaboration.state()).unwrap();

    // Force captures and resolutions beyond the small position cache, through
    // many text items, while preserving both insertion affinities.
    let anchors: Vec<_> = changed
        .char_indices()
        .step_by(7)
        .flat_map(|(point, _)| {
            [Assoc::Before, Assoc::After]
                .map(|assoc| (point, assoc, document.collaboration.position(point, assoc)))
        })
        .collect();
    for (point, _, anchor) in &anchors {
        assert_eq!(document.collaboration.resolve(anchor), Some(*point));
    }
    let start = changed.find("日本").unwrap();
    let end = changed.rfind("新").unwrap();
    let reverse = document.collaboration.selection(Selection {
        anchor: end,
        head: start,
    });
    let composition = document
        .collaboration
        .composition_target(Selection::caret(start));
    edit(&mut peer, 0..0, "peer 🧑‍💻\r\n", EditOrigin::Human);
    document.receive(&peer.collaboration.state()).unwrap();
    let prefix = "peer 🧑‍💻\r\n".len();
    for (point, assoc, anchor) in &anchors {
        // At the original initial gap, Before remains before the remote insert.
        let expected = if *point == 0 && *assoc == Assoc::Before {
            0
        } else {
            point + prefix
        };
        assert_eq!(document.collaboration.resolve(anchor), Some(expected));
    }
    assert_eq!(
        document.collaboration.resolve_selection(&reverse),
        Some(Selection {
            anchor: end + prefix,
            head: start + prefix
        })
    );
    assert_eq!(
        document.collaboration.resolve_composition(&composition),
        Some(Selection::caret(start + prefix))
    );
    document.history(EditOrigin::Human, false).unwrap();
    assert_eq!(document.source(), format!("peer 🧑‍💻\r\n{source}"));
    document.history(EditOrigin::Human, true).unwrap();
    assert_eq!(document.source(), format!("peer 🧑‍💻\r\n{changed}"));
    oracle(&document);
    peer.receive(&document.collaboration.state()).unwrap();
    peer.receive(&document.collaboration.state()).unwrap();
    assert_eq!(peer.source(), document.source());
    let reopened = Document::from_state(&document.collaboration.state()).unwrap();
    assert_eq!(reopened.source(), document.source());
    oracle(&reopened);
}

#[test]
fn known_insertions_do_not_hide_delete_only_updates_or_invalid_envelopes() {
    let mut document = Document::new(RICH.into());
    let mut peer = Document::from_state(&document.collaboration.state()).unwrap();
    let clock = document
        .collaboration
        .doc_for_test()
        .transact()
        .state_vector();
    let point = peer.source().find("日本語").unwrap();
    edit(
        &mut peer,
        point..point + "日本語".len(),
        "",
        EditOrigin::Human,
    );
    let deletes = peer.collaboration.diff_for_test(&clock);
    // Same insertion clocks, nonempty delete set. A state-vector-only shortcut
    // must not classify this as already applied.
    assert_eq!(
        clock,
        peer.collaboration.doc_for_test().transact().state_vector()
    );
    document.receive(&deletes).unwrap();
    assert_eq!(document.source(), peer.source());
    let generation = document.collaboration.generation();
    document.receive(&deletes).unwrap();
    assert_eq!(document.collaboration.generation(), generation);
    let mut invalid = document.collaboration.state();
    invalid.update.push(0); // trailing bytes, even though all operations are known
    assert!(document.receive(&invalid).is_err());
    assert_eq!(document.collaboration.generation(), generation);
    oracle(&document);
}

#[test]
fn mixed_batches_retain_one_full_parse_fallback_and_exact_markdown() {
    for source in [
        "# Title\n\nParagraph.\n\nTail.\n",
        "[id]: /old\n\n[Paragraph][id].\n\nTail.\n",
        "```\nParagraph.\n```\n\nTail.\n",
        "- item\n\nParagraph.\n\nTail.\n",
    ] {
        let mut document = Document::new(source.into());
        let point = source.find("Paragraph").unwrap();
        let tail = source.find("Tail").unwrap();
        let edits = [
            SourceEdit {
                range: point..point + 9,
                text: "one\n\ntwo".into(),
            },
            SourceEdit {
                range: tail..tail + 4,
                text: "[link](image.png)".into(),
            },
        ];
        document
            .transact(&edits, EditOrigin::Agent, None, true)
            .unwrap();
        oracle(&document);
        document.history(EditOrigin::Agent, false).unwrap();
        assert_eq!(document.source(), source);
        oracle(&document);
        document.history(EditOrigin::Agent, true).unwrap();
        oracle(&document);
    }
}
