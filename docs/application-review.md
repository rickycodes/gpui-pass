# Application review — 2026-11-11

Reviewed the store backend, GnuPG subprocess handling, generation, session lifecycle,
status decoding, desktop operations, text editing, clipboard handling, card detection,
and compatibility scripts.

## Patched

- **Edit content changed on every save.** Encryption always appended a newline.
  Updates now preserve exact bytes; generated entries retain the pass newline convention.
- **Encryption could hang before timeout/cancellation.** A blocking stdin write ran
  before output draining. Stdin is now nonblocking and fed alongside stdout/stderr reads.
  Completion also checks that all input was submitted.
- **Large valid entries could fail encryption.** The ciphertext limit equaled the
  plaintext limit, leaving no room for OpenPGP overhead. Ciphertext now has a separate
  2 MiB bound for the 1 MiB plaintext limit.
- **Unnecessary subprocess delays.** Encryption/decryption slept after every read,
  even with more data available. They now sleep only when no input/output progresses.
- **Signed recipient policy could be bypassed unintentionally.** A detached
  `.gpg-id.sig` was ignored unless the signing environment variable was set. A signature
  beside the selected recipient file now blocks writes pending verification support.
- **Editor state could outlive its entry.** Filtering/refresh now clears the editor;
  selection/store changes and duplicate saves are blocked during a write. Cancelling
  an edit requests cancellation of an outstanding edit save. As with other writes,
  cancellation cannot undo a replacement that has already completed.
- **Sensitive state cleanup.** The editor buffer now uses zeroizing storage and is
  wiped on cancellation. Completed generation in an inactive window no longer copies
  or retains the generated password for display. Cancelling a nonexistent edit no
  longer erases the generator's entry name on focus loss.
- **Card detail recovery could stop permanently.** Slow enumeration now gets quiet
  retries after the initial short burst. An in-flight `ykman` worker remains tracked
  across removal, its stale result is discarded, and new GnuPG probes wait for it.
- **Sorting allocated repeatedly.** Store listing computes each sort key once.
- **Stale state and messages.** Failed edits clear the saving flag, generation clears
  stale IME composition, and shared error messages apply to both creation and updates.

## Validation

- 26 core tests and 2 desktop tests pass.
- Added regression coverage for exact edited bytes, generated trailing newlines,
  encryption backpressure, deadlines while stdin is blocked, and signed policies.
- The disposable real `pass`/GnuPG fixture passes generation and edit/decrypt round
  trips for multiline text with and without a trailing newline, empty text, and 1 MiB.
- Release compilation and diff whitespace checks pass. Clippy completes; existing
  desktop style warnings remain about type complexity, argument count, a nested
  conditional, and test-module placement.
- No GUI interaction or physical card reconnection was exercised during this pass.

## Larger follow-ups

- Detect external edits before replacement to prevent overwriting another client's
  changes; concurrent update handling is already a documented limitation.
- Split desktop operation state and card monitoring out of the large main module.
  Explicit operation state would reduce dependence on coordinated boolean flags.
- Move sysfs scans off the UI thread and measure large-store search and long-entry
  rendering before changing polling or introducing caches.
- Add GPUI interaction coverage for focus, IME, mouse selection, and save/cancel
  transitions. Current desktop tests cover USB event parsing rather than interaction.

GPUI text caches, allocator copies, and rendered buffers remain outside the editor
buffer's zeroization guarantee, as described in SECURITY.md.
