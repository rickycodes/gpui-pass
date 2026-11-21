//! Run through scripts/test-pass-compat.sh to create isolated disposable keys and stores.
use gpui_pass_core::{
    Cancellation, Entry, GeneratorOptions, Gpg, OperationProgress, OperationStatus, Secret, Store,
};
use std::{env, fs};
use zeroize::Zeroizing;

#[test]
#[ignore = "requires the disposable fixture created by scripts/test-pass-compat.sh"]
fn roundtrips_a_real_pass_store_without_changing_existing_entries() {
    let path = env::var_os("GPUI_PASS_TEST_STORE").expect("run scripts/test-pass-compat.sh");
    let store = Store::open(&path).unwrap();
    let before_root = fs::read(store.root().join("personal/email.gpg")).unwrap();
    let before_nested = fs::read(store.root().join("work/service.gpg")).unwrap();
    let entries = store.entries().unwrap();
    assert_eq!(
        entries.iter().map(Entry::name).collect::<Vec<_>>(),
        ["personal/email", "work/service"]
    );
    let gpg = Gpg::new("/usr/bin/gpg").unwrap();
    for (name, expected) in [
        (
            "personal/email",
            "fixture-root-secret\nusername: person@example.test\n\nPreserve multiline notes.\n",
        ),
        (
            "work/service",
            "fixture-nested-secret\nusername: work@example.test\n",
        ),
    ] {
        let entry = entries.iter().find(|entry| entry.name() == name).unwrap();
        let secret = gpg
            .decrypt(&store, entry, &Cancellation::default())
            .unwrap();
        assert_eq!(secret.expose(), expected);
    }
    assert_eq!(
        fs::read(store.root().join("personal/email.gpg")).unwrap(),
        before_root
    );
    assert_eq!(
        fs::read(store.root().join("work/service.gpg")).unwrap(),
        before_nested
    );

    let generated = Entry::from_relative_file("generated/roundtrip.gpg").unwrap();
    let cancel = Cancellation::default();
    let progress = OperationProgress::new(OperationStatus::working("Fixture", "Disposable store"));
    let password = gpg
        .generate_entry(
            &store,
            &generated,
            GeneratorOptions {
                words: 4,
                add_digits: true,
                add_separators: true,
            },
            &cancel,
            &progress,
        )
        .unwrap();
    assert_eq!(
        gpg.decrypt(&store, &generated, &cancel).unwrap().expose(),
        format!("{}\n", password.expose())
    );
    for text in [
        "password\nnotes\n".to_owned(),
        "password\nnotes".to_owned(),
        String::new(),
        "x".repeat(1024 * 1024),
    ] {
        let secret = Secret::from_bytes(Zeroizing::new(text.into_bytes())).unwrap();
        gpg.update_entry(&store, &generated, &secret, &cancel, &progress)
            .unwrap();
        assert_eq!(
            gpg.decrypt(&store, &generated, &cancel).unwrap().expose(),
            secret.expose()
        );
    }
    store.remove_entry(&generated).unwrap();

    let initialized_root = std::path::Path::new(&path)
        .parent()
        .unwrap()
        .join("app-initialized");
    let initialized_store =
        Store::initialize(&initialized_root, &["root@example.test".to_owned()], None).unwrap();
    let initialized_entry = Entry::from_relative_file("app/initialized.gpg").unwrap();
    let initialized_secret =
        Secret::from_bytes(Zeroizing::new(b"created-by-gpui-pass\n".to_vec())).unwrap();
    gpg.create_entry(
        &initialized_store,
        &initialized_entry,
        &initialized_secret,
        &Cancellation::default(),
        &progress,
    )
    .unwrap();
}
