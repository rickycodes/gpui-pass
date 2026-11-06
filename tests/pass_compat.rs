//! Run through scripts/test-pass-compat.sh to create isolated disposable keys and stores.
use gpui_pass_core::{Cancellation, Entry, Gpg, Store};
use std::{env, fs};

#[test]
#[ignore = "requires the disposable fixture created by scripts/test-pass-compat.sh"]
fn opens_a_real_pass_store_without_modification() {
    let path = env::var_os("GPUI_PASS_TEST_STORE").expect("run scripts/test-pass-compat.sh");
    let store = Store::open(path).unwrap();
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
}
