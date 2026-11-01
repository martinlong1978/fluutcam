// SPDX-License-Identifier: MIT OR Apache-2.0

//! The edit journal (D06 §7.3, §8): a crash at any byte recovers a valid
//! prefix of the edits.

use fluut_model::document::Document;
use fluut_model::edit::Edit;
use fluut_model::journal::{Journal, Record, decode, encode, file_hash, journal_path, recover};
use fluut_model::persist::save;
use fluut_model::project::{Project, Setup, read};
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn edits() -> Vec<Record> {
    let mut s = Setup::new("s1", "Top");
    s.material = Some("MDF".into());
    vec![
        Record::Edit(Edit::SetName("Crash test".into())),
        Record::Edit(Edit::add_setup(None, &s)),
        Record::Edit(Edit::set_setup_field(
            "s1",
            "stock",
            Some("{ kind = \"block\", margin = [1, 2, 3] }"),
        )),
        Record::Undo,
        Record::Redo,
        Record::Edit(Edit::Group {
            label: "Two".into(),
            edits: vec![
                Edit::set_setup_field("s1", "name", Some("\"Ünïcode ✓\"")),
                Edit::SetUnits("inch".into()),
            ],
        }),
        Record::Undo,
    ]
}

/// Writes the session: project saved, journal started, records appended.
fn session(name: &str) -> (PathBuf, Vec<u8>, Project) {
    let d = dir(name);
    let path = d.join("p.fluut");
    let base = Project::new("Base");
    let saved = save(&path, &base).unwrap();
    let hash = file_hash(&saved.bytes);
    let mut doc = Document::new(base.clone())
        .with_journal(Journal::create(&journal_path(&path), &hash).unwrap());
    doc.replay(&edits());
    doc.journal_mut().unwrap().sync().unwrap();
    let journal = std::fs::read(journal_path(&path)).unwrap();
    (path, journal, doc.project().clone())
}

/// The document after replaying `records` onto the base project.
fn replayed(records: &[Record]) -> Project {
    let mut d = Document::new(Project::new("Base"));
    assert_eq!(d.replay(records), records.len());
    d.project().clone()
}

#[test]
fn records_round_trip() {
    for r in edits() {
        let b = encode(&r);
        let mut j = b"FLUJ\x01\x00".to_vec();
        j.extend_from_slice(&[0; 32]);
        j.extend_from_slice(&b);
        assert_eq!(decode(&j, &[0; 32]).unwrap(), [r]);
    }
}

#[test]
fn recovery_replays_the_journal() {
    let (path, journal, last) = session("journal-full");
    let hash = file_hash(&std::fs::read(&path).unwrap());
    let records = recover(&journal_path(&path), &hash).unwrap().unwrap();
    assert_eq!(records, edits());
    assert_eq!(replayed(&records), last);
    assert_eq!(last.setups[0].stock.as_ref().unwrap().kind(), "block");
    assert_eq!(decode(&journal, &hash).unwrap().len(), edits().len());
}

#[test]
fn a_cut_at_every_byte_recovers_a_prefix() {
    let (path, journal, _) = session("journal-cut");
    let hash = file_hash(&std::fs::read(&path).unwrap());
    let all = edits();
    let mut lens = Vec::new();
    for cut in 0..=journal.len() {
        match decode(&journal[..cut], &hash) {
            // Inside the header: nothing to offer.
            None => assert!(cut < 38, "{cut}"),
            Some(r) => {
                assert_eq!(r[..], all[..r.len()], "cut at {cut}");
                // Every prefix replays to a valid document.
                let p = replayed(&r);
                let text = fluut_model::project::write(&p);
                assert_eq!(read(&text).unwrap().project, p);
                lens.push(r.len());
            }
        }
    }
    // Recovery grows one record at a time up to all of them.
    assert_eq!(lens.first(), Some(&0));
    assert_eq!(lens.last(), Some(&all.len()));
    assert!(lens.windows(2).all(|w| w[1] == w[0] || w[1] == w[0] + 1));
}

#[test]
fn a_damaged_record_ends_recovery() {
    let (path, mut journal, _) = session("journal-flip");
    let hash = file_hash(&std::fs::read(&path).unwrap());
    // Flip one byte inside the third record's payload.
    let mut at = 38;
    for _ in 0..2 {
        let n = u32::from_le_bytes(journal[at..at + 4].try_into().unwrap()) as usize;
        at += 8 + n;
    }
    journal[at + 5] ^= 0x40;
    let r = decode(&journal, &hash).unwrap();
    assert_eq!(r, edits()[..2]);
    // A huge length is damage too.
    let mut j2 = journal[..38].to_vec();
    j2.extend_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(decode(&j2, &hash).unwrap(), []);
}

#[test]
fn a_journal_for_another_file_is_not_offered() {
    let (path, _, _) = session("journal-other");
    let other = file_hash(b"another file");
    assert_eq!(recover(&journal_path(&path), &other).unwrap(), None);
    assert_eq!(recover(&path.with_extension("none"), &other).unwrap(), None);
    // After a save, the journal is reset for the new file: nothing to
    // recover.
    let mut j = Journal::create(&journal_path(&path), &other).unwrap();
    j.append(&Record::Undo).unwrap();
    j.reset(&other).unwrap();
    assert_eq!(recover(&journal_path(&path), &other).unwrap(), None);
}

#[test]
fn syncs_are_batched() {
    let d = dir("journal-sync");
    let mut j = Journal::create(&d.join("j"), &[0; 32]).unwrap();
    assert!(!j.sync_if_due(0).unwrap());
    j.append(&Record::Undo).unwrap();
    assert!(j.sync_if_due(0).unwrap());
    j.append(&Record::Undo).unwrap();
    assert!(!j.sync_if_due(999).unwrap());
    assert!(j.sync_if_due(1000).unwrap());
}
