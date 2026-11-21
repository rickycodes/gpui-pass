use gpui_pass_core::{Entry, Error, OperationStatus, Secret, Session, Store};
use std::{
    fs,
    time::{Duration, Instant},
};
use tempfile::tempdir;
use zeroize::Zeroizing;

fn entry(name: &str) -> Entry {
    Entry::from_relative_file(name).unwrap()
}
fn secret() -> Secret {
    Secret::from_bytes(Zeroizing::new(
        b"test-only-secret\nnotes: preserved\n".to_vec(),
    ))
    .unwrap()
}

#[test]
fn rejects_traversal_absolute_controls_and_ambiguous_paths() {
    for path in [
        "",
        "../x.gpg",
        "/x.gpg",
        "a/../x.gpg",
        "a/./x.gpg",
        "./x.gpg",
        "a//x.gpg",
        "a\\x.gpg",
        "x",
        "x.GPG",
        "x\nx.gpg",
        "x\u{202e}.gpg",
        ".gpg",
    ] {
        assert!(
            Entry::from_relative_file(path).is_err(),
            "accepted {path:?}"
        );
    }
}

#[test]
fn preserves_nested_names_and_matches_unicode_and_multiple_terms() {
    let item = entry("work/Émail/account.gpg.gpg");
    assert_eq!(item.name(), "work/Émail/account.gpg");
    assert_eq!(item.title(), "account.gpg");
    assert_eq!(item.folder(), "work/Émail");
    assert!(item.matches("éMAIL work"));
    assert!(!item.matches("work personal"));
}

#[test]
fn lists_only_ciphertext_and_preserves_store_bytes() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("work")).unwrap();
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    fs::create_dir_all(dir.path().join(".extensions")).unwrap();
    for name in [
        "work/Émail.gpg",
        "alpha.gpg",
        ".git/hidden.gpg",
        ".extensions/hidden.gpg",
        ".gpg-id",
        "notes.txt",
    ] {
        fs::write(dir.path().join(name), b"ciphertext-fixture").unwrap();
    }
    let store = Store::open(dir.path()).unwrap();
    let entries = store.entries().unwrap();
    assert_eq!(
        entries.iter().map(Entry::name).collect::<Vec<_>>(),
        ["alpha", "work/Émail"]
    );
    assert_eq!(
        fs::read(dir.path().join("alpha.gpg")).unwrap(),
        b"ciphertext-fixture"
    );
}

#[cfg(unix)]
#[test]
fn missing_entry_revision_is_not_misclassified_as_invalid() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.entry_revision(&entry("new-entry.gpg")),
        Err(Error::EntryNotFound)
    ));
}

#[cfg(unix)]
#[test]
fn initializes_a_pass_store_without_overwriting_existing_data() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("new-store");
    let store = Store::initialize(
        &root,
        &[
            "alice@example.com".to_owned(),
            "backup@example.com".to_owned(),
        ],
        Some(b"signature"),
    )
    .unwrap();
    assert!(store.has_root_recipient_policy().unwrap());
    assert_eq!(
        fs::read(root.join(".gpg-id")).unwrap(),
        b"alice@example.com\nbackup@example.com\n"
    );
    assert_eq!(fs::read(root.join(".gpg-id.sig")).unwrap(), b"signature");
    assert!(matches!(
        Store::initialize(&root, &["other@example.com".to_owned()], None),
        Err(Error::StoreNotEmpty)
    ));

    let existing = dir.path().join("empty-store");
    fs::create_dir(&existing).unwrap();
    Store::initialize(&existing, &["alice@example.com".to_owned()], None).unwrap();
    assert_eq!(
        fs::read(existing.join(".gpg-id")).unwrap(),
        b"alice@example.com\n"
    );
}

#[cfg(unix)]
#[test]
fn listing_does_not_follow_symlinks() {
    use std::os::unix::fs::symlink;
    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("outside.gpg"), b"ciphertext").unwrap();
    symlink(outside.path(), dir.path().join("escape")).unwrap();
    symlink(
        outside.path().join("outside.gpg"),
        dir.path().join("escape.gpg"),
    )
    .unwrap();
    assert!(
        Store::open(dir.path())
            .unwrap()
            .entries()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn selection_and_escape_cancel_pending_requests_and_reject_late_results() {
    let mut session = Session::default();
    session.select(Some(entry("first.gpg")));
    let first = session.begin_reveal().unwrap();
    session.select(Some(entry("second.gpg")));
    assert!(first.cancellation.is_cancelled());
    assert!(!session.finish_reveal(&first, secret(), true, Instant::now()));
    let second = session.begin_reveal().unwrap();
    session.hide();
    assert!(!session.finish_reveal(&second, secret(), true, Instant::now()));
    assert!(session.revealed(Instant::now()).is_none());
}

#[test]
fn reveal_expires_and_does_not_extend_when_read() {
    let mut session = Session::default();
    session.select(Some(entry("first.gpg")));
    let request = session.begin_reveal().unwrap();
    let now = Instant::now();
    assert!(session.finish_reveal(&request, secret(), true, now));
    assert!(session.revealed(now + Duration::from_secs(29)).is_some());
    assert!(session.revealed(now + Duration::from_secs(30)).is_none());
    assert!(session.expire(now + Duration::from_secs(30)));
    assert!(!session.expire(now + Duration::from_secs(31)));
}

#[test]
fn deactivation_hides_visible_but_allows_pinentry_and_rejects_background_completion() {
    let mut session = Session::default();
    session.select(Some(entry("first.gpg")));
    let request = session.begin_reveal().unwrap();
    session.deactivate();
    assert!(!request.cancellation.is_cancelled());
    assert!(!session.finish_reveal(&request, secret(), false, Instant::now()));
    assert!(!session.is_pending());
    let request = session.begin_reveal().unwrap();
    assert!(session.finish_reveal(&request, secret(), true, Instant::now()));
    session.deactivate();
    assert!(session.revealed(Instant::now()).is_none());
}

#[test]
fn secrets_are_redacted_and_invalid_utf8_is_rejected() {
    assert_eq!(format!("{:?}", secret()), "Secret([REDACTED])");
    assert!(matches!(
        Secret::from_bytes(Zeroizing::new(vec![0xff])),
        Err(Error::InvalidText)
    ));
}

#[test]
fn operation_status_is_scoped_to_the_pending_request_and_clears_on_completion() {
    let mut session = Session::default();
    session.select(Some(entry("first.gpg")));
    let old = session.begin_reveal().unwrap();
    let attention = OperationStatus::attention("Check key", "Touch it if blinking.");
    old.progress.update(attention);
    assert_eq!(session.pending_status(), Some(attention));

    session.select(Some(entry("second.gpg")));
    assert!(session.pending_status().is_none());
    let current = session.begin_reveal().unwrap();
    let initial = session.pending_status();
    old.progress.update(OperationStatus::attention(
        "Old worker",
        "Must not replace current status.",
    ));
    assert_eq!(session.pending_status(), initial);

    assert!(session.finish_reveal(&current, secret(), true, Instant::now()));
    assert!(session.pending_status().is_none());
    let failed = session.begin_reveal().unwrap();
    assert!(session.fail_reveal(&failed));
    assert!(session.pending_status().is_none());
    session.begin_reveal().unwrap();
    session.hide();
    assert!(session.pending_status().is_none());
}
