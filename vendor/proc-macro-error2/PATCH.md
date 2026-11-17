Vendored proc-macro-error2 2.0.1 from crates.io under its upstream
MIT OR Apache-2.0 license. Both licenses and upstream tests are preserved.

Change `extern crate proc_macro` to `pub extern crate proc_macro` in `src/lib.rs`.
The existing public re-export in `__export` must refer to a public import.
This fixes the `pub_use_of_private_extern_crate` future-incompatibility warning
(Rust issue https://github.com/rust-lang/rust/issues/127909) without suppressing
the lint or changing macro behavior.

GPUI 0.2.2 pulls this crate through stacksafe-macro. Version 2.0.1 remains the
latest published release at the time of this patch. Remove this override when
a compatible upstream release fixes the import or GPUI removes the dependency.
