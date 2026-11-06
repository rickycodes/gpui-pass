# xattr 0.2.3 compatibility patch

Upstream: <https://crates.io/crates/xattr/0.2.3>, MIT OR Apache-2.0. Both license files are retained.

Original crate SHA-256: `6d1526bbe5aaeb5eb06885f4d987bcdfa5e23187055de9b83fe00156a821fabc`.

GPUI 0.2.2 depends on `gpui_http_client`, which depends on `zed-async-tar 0.5.0-zed`, which requires `xattr 0.2.3`. This old xattr release references `libc::ENOATTR` on Linux/Android; current libc no longer supplies that alias. The missing-attribute errno on these platforms is `ENODATA`.

Only `src/sys/mod.rs` differs from the published source: select `ENODATA` for Linux/Android, preserve `ENOATTR` for macOS/BSD, and add a Linux regression test for missing/present/removed attributes on a temporary file. Other source, tests, metadata, and licenses are unchanged. Registry cache files are never modified, and libc is not downgraded.

Remove the patch when the GPUI dependency chain upgrades xattr. See the [libc upstream discussion](https://github.com/rust-lang/libc/issues/1356).

Run the regression:

```sh
cargo test --manifest-path vendor/xattr/Cargo.toml --lib compatibility_tests
```
