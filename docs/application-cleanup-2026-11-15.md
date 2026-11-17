# Review fixes — 2026-11-15

Follow-up to the [2026-11-14 application review](application-review-2026-11-14.md).

## Implemented

- Pending copies own a cancellation token and cancel their worker when discarded.
  Search, refresh, selection, Escape, and opening the generator invalidate them.
  Copy is included in card polling/probing and verification busy guards; reveal
  and copy cannot start concurrently.
- A shared clipboard controller records the copy source independently of the
  selected entry, preserves the original clipboard through repeated copies,
  forgets its lease when another application replaces it, and restores only its
  own contents. Entry copies, generated passwords, and editor selections use
  `PASSWORD_STORE_CLIP_TIME` (45 seconds by default). Invalid/overflowing values
  fall back safely. Normal application shutdown and window release restore a
  tracked copy. Crashes, forced termination, and clipboard-manager histories
  remain outside that cleanup guarantee.
- The generator renders as the active page inside the app shell, sharing the
  footer instead of covering it. App messages and active operations take
  priority over clipboard status; operation messages include title and detail.
  Hidden underlying pages are no longer built while generating.
- Versioned reveals capture the identity, size, and modification/change
  timestamps of the actual encrypted file opened for decryption, then validate
  them after decryption. Edited saves check that revision before encrypting and
  before replacing the pinned destination. Detected conflicts preserve the
  current on-disk file and leave the editor buffer available.
- Editing is explicitly bounded to 16 KiB and 512 lines, including the initial
  load. Oversized pastes are rejected rather than silently truncated, and tabs,
  line endings, and Unicode are preserved. Replacement buffers and transient
  insertion text use zeroizing storage; old editor allocations are wiped when
  replaced instead of being grown in place.
- Metadata helpers drain both output pipes while running, cap each stream at
  64 KiB, retain their deadline, and kill/reap children on errors. This removes
  the full-pipe exit-wait deadlock without changing hardware retry scheduling.
- Diagnostic files reject final-component symlinks, non-regular files, and hard
  links, and enforce 0600 on existing files as well as new files.
- Store search caches normalized entry names and normalizes query terms once
  per filter. Post-generation sorting caches sort keys. Product image lookup
  initializes only the selected image. Generated secret visibility reuses the
  shared reveal duration constant.
- Shared titlebar/window-control construction now serves the main, generator,
  and custom-entry views. Input hit testing and IME cursor bounds use measured
  GPUI text layout instead of a fixed character advance.

## Remaining work and limits

- Signed recipient policies are verified against the configured full
  fingerprints in `PASSWORD_STORE_SIGNING_KEY`, and the exact policy snapshot
  is checked again before writing. A bounded per-entry runtime lock coordinates
  app instances, but non-cooperating writers can still race between the final
  comparison and rename. The editor preserves conflicted text and offers
  explicit overwrite or discard-and-reveal choices; it does not merge versions.
  The legacy core `update_entry` API snapshots at call time; clients needing
  protection since read must use `decrypt_with_revision` and
  `update_entry_if_unchanged`, as the GUI does.
- Keep periodic sysfs scans and hardware retry scheduling unchanged until they
  can be measured with real hardware. Large-store search latency has not been
  benchmarked. The editor now has a firm bound; viewport virtualization and
  layout caching can be considered if larger editing is needed.
- GPUI, clipboard, allocator, and compositor copies remain outside complete
  zeroization guarantees. Log parent directories still depend on the user's
  trusted filesystem configuration. Clipboard ownership is determined by text
  equality, so an indistinguishable later copy of the same text cannot be
  distinguished from the app's copy.
- After this review, the user manually exercised hidden copy, reveal/hide,
  editing, and deletion with a hardware-backed store and YubiKey and reported
  that they worked. This confirms those flows on that setup. The generator
  footer, cancellation when changing selection or pressing Escape,
  normal-shutdown restoration, external clipboard replacement, and the full
  PIN/touch/reconnect matrix still need separate validation.

## Validation

- 28 core tests and 23 desktop tests pass.
- Nine Python helper tests and the disposable real `pass`/GnuPG fixture pass.
- Regression coverage includes copy cancellation on drop, clipboard replacement
  and restoration, invalid timeout configuration, exact editor input and limits,
  external edits before/during encryption, changes during decryption, bounded
  subprocess output/deadlines, and log link/permission handling.
- Core Clippy passes with warnings denied. Desktop Clippy completes with two
  existing structural warnings: resize helper argument count and test-module
  placement. New standalone modules are formatted; the existing repository-wide
  formatting backlog was not rewritten.
