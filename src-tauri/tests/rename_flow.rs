use std::path::PathBuf;

use soundbox::{db::Db, rename, scan};

struct Fixture {
    dir: PathBuf,
    db: Db,
    id: i64,
}

/// A real folder with one real wav, scanned into a real database. Rename is the
/// only destructive operation in the app, so it is worth exercising end to end
/// rather than mocking the filesystem.
fn setup(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("sb_flow_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    for (file, freq) in [("original.wav", 440), ("occupied.wav", 220)] {
        let status = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i"])
            .arg(format!("sine=frequency={freq}:duration=1"))
            .arg(dir.join(file))
            .status()
            .expect("ffmpeg must be available for this test");
        assert!(status.success());
    }

    let db = Db::open(&dir.join("library.db")).unwrap();
    let root_id = db.add_root(&dir, "t").unwrap();
    scan::scan_root(&db, &dir, root_id, &dir, |_| {}).unwrap();

    let id = db
        .all_items()
        .unwrap()
        .into_iter()
        .find(|i| i.filename == "original.wav")
        .expect("scan should have indexed original.wav")
        .id;

    Fixture { dir, db, id }
}

fn filename(f: &Fixture) -> String {
    f.db.all_items().unwrap().into_iter().find(|i| i.id == f.id).unwrap().filename
}

#[test]
fn rename_moves_the_file_and_updates_the_index() {
    let f = setup("basic");

    let out = rename::perform(&f.db, f.id, "a nice fart", true).unwrap();
    assert_eq!(out.filename, "a nice fart.wav", "extension must be preserved");
    assert!(!out.suffixed);

    assert!(f.dir.join("a nice fart.wav").exists());
    assert!(!f.dir.join("original.wav").exists());
    assert_eq!(filename(&f), "a nice fart.wav", "index should follow the file");

    let _ = std::fs::remove_dir_all(&f.dir);
}

#[test]
fn undo_restores_the_previous_name() {
    let f = setup("undo");

    rename::perform(&f.db, f.id, "renamed", true).unwrap();
    assert_eq!(filename(&f), "renamed.wav");

    let undone = rename::undo_last(&f.db).unwrap();
    assert_eq!(undone.as_deref(), Some("original"));
    assert!(f.dir.join("original.wav").exists());
    assert!(!f.dir.join("renamed.wav").exists());
    assert_eq!(filename(&f), "original.wav");

    // A second undo must not redo the rename.
    assert_eq!(rename::undo_last(&f.db).unwrap(), None);
    assert!(f.dir.join("original.wav").exists());

    let _ = std::fs::remove_dir_all(&f.dir);
}

#[test]
fn collision_suffixes_and_leaves_the_other_file_alone() {
    let f = setup("collide");
    let occupied_before = std::fs::read(f.dir.join("occupied.wav")).unwrap();

    let out = rename::perform(&f.db, f.id, "occupied", true).unwrap();
    assert_eq!(out.filename, "occupied (2).wav");
    assert!(out.suffixed);
    assert_eq!(
        std::fs::read(f.dir.join("occupied.wav")).unwrap(),
        occupied_before,
        "the existing sound must not be overwritten"
    );

    let _ = std::fs::remove_dir_all(&f.dir);
}

#[test]
fn invalid_name_is_refused_and_changes_nothing() {
    let f = setup("invalid");

    assert!(rename::perform(&f.db, f.id, "bad/name", true).is_err());
    assert!(rename::perform(&f.db, f.id, "NUL", true).is_err());
    assert!(rename::perform(&f.db, f.id, "  ", true).is_err());

    assert!(f.dir.join("original.wav").exists());
    assert_eq!(filename(&f), "original.wav");
    assert_eq!(rename::undo_last(&f.db).unwrap(), None, "failures must not enter the undo log");

    let _ = std::fs::remove_dir_all(&f.dir);
}

#[test]
fn tags_survive_a_rename() {
    let f = setup("tags");
    f.db.tag_file(f.id, "wet").unwrap();
    f.db.toggle_favorite(f.id).unwrap();

    rename::perform(&f.db, f.id, "totally different", true).unwrap();

    let item = f.db.all_items().unwrap().into_iter().find(|i| i.id == f.id).unwrap();
    assert_eq!(item.filename, "totally different.wav");
    assert!(item.tags.contains(&"wet".to_string()), "tags key on content, not path");
    assert!(item.favorite);

    let _ = std::fs::remove_dir_all(&f.dir);
}
