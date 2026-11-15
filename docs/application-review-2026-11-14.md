# Application review — 2026-11-14

Reviewed the store backend, recipient policies, GnuPG subprocesses, generation,
secret lifetimes, desktop editing, clipboard behavior, notifications, navigation,
card helpers, diagnostics, and theme loading. This is a source review with
automated checks, not an independent security certification or hardware/GUI test.
Findings below describe the current implementation; they have not been fixed
by this review.

## Highest priority

1. **Editing can overwrite another client's changes.**
   `src/store.rs:318` validates that the current entry is a regular file, then
   replaces it. It does not compare it with the version originally decrypted
   for editing. A terminal edit made while the GUI editor is open can be lost.
   This is already on the roadmap and remains the most important write-safety
   gap. Carry an encrypted-entry revision into the edit and detect conflicts
   before saving. A check immediately before rename still needs a concurrency
   strategy; atomic rename alone does not implement compare-and-swap.

2. **Copy workers cannot actually be cancelled and are missing from device
   coordination.** `desktop/src/main.rs:2117` gives the worker a cancellation
   token that the UI never retains. Selection changes and Escape drop the
   receiver, but the GnuPG process can keep running until its deadline. Repeating
   this can accumulate workers and prompts. Copy is also absent from the card
   polling pause (`main.rs:950`) and verification busy checks. Retain a token
   in a named pending-operation type, cancel on invalidation/drop, and include
   copy in the shared device-operation guard. Search and refresh should also
   invalidate copy requests, rather than relying only on final entry equality.

3. **Notifications are hidden in the generator and can suppress each other.**
   The full-window generator overlay (`main.rs:1892`) covers the footer inserted
   beneath it (`main.rs:3095`). Generator validation/save errors and progress
   therefore disappear from view after removing their inline displays. A
   clipboard countdown also wins over every operation/error notification
   (`main.rs:2810`): copy A, then copy B with a touch requirement, and A's
   countdown hides B's guidance. Make the footer a shared sibling beneath page
   content and overlays; prioritize required interaction and failures above
   passive clipboard information. Render the operation title as well as detail
   so a busy message identifies which operation is waiting.

4. **Clipboard handling is split across incompatible lifecycles.**
   Entry copies restore previous contents after the configured timeout;
   generated copies use a separate 30-second clear-on-hide path and omit the
   persistent indicator. Copying from the editor bypasses both timers. Normal
   app exit does not restore/clear a tracked copy, so the password can remain
   on the clipboard indefinitely after closing the app. Consolidate these into
   one clipboard controller with explicit policies for each copy source and
   cleanup on normal shutdown, retaining the newer-clipboard-content check.
   Clipboard-manager histories remain outside the app's control.

## Other correctness and hardening findings

- **The copied-entry label changes with selection.** `main.rs:2812` uses the
  selected entry rather than recording the copy's source. Copy A and select B:
  the footer says B was copied. Store source metadata with the clipboard item.
  Also stop displaying the countdown once the clipboard is known to have been
  replaced by another application.
- **Editor input is silently filtered and truncated.** `desktop/src/search.rs:41`
  caps inserted text at 16,384 characters and removes controls including tabs.
  Pasting structured multiline content can silently alter or shorten it before
  saving. Reject oversized input with visible feedback and preserve supported
  text exactly; keep filename validation separate from secret editing.
- **Clipboard timeout arithmetic can panic.** `main.rs:2041` adds an arbitrary
  parsed `u64` duration to `Instant`. An extreme `PASSWORD_STORE_CLIP_TIME`
  can overflow the platform's clock range. Validate with `checked_add` and a
  documented fallback. This requires local launch configuration; it is not a
  remote attack vector.
- **Probe output can cause false timeouts.** Card status and metadata helpers
  (`main.rs:3193`, `3350`) wait for process exit before draining stdout/stderr.
  A full pipe prevents exit. Drain bounded output during execution, as the
  encryption/decryption helper already does, and retain the overall deadline.
- **Log files follow symlinks and retain existing permissions.**
  `desktop/src/diagnostics.rs:51` uses ordinary append/create with mode 0600;
  that mode only applies to creation. Existing files can be broadly readable,
  and a symlink can redirect appends. Use no-follow opening, validate a regular
  file and link count, and establish restrictive permissions. Logs currently
  exclude secret contents; this is filesystem hardening rather than evidence
  of password disclosure.
- **Editor zeroization has limits beyond the renderer.** The transient inserted
  string in `search.rs:46` is ordinary `String`; growing the wrapped editor
  string can also leave its previous allocation unwiped. Use zeroizing
  transient buffers and an allocation strategy that wipes replaced storage.
  GPUI shaping, clipboard, and compositor copies still have separate limits.

## Performance opportunities

- **Bound or virtualize editor rendering first.** `begin_edit` loads the full
  entry (up to 1 MiB), while `entry_editor.rs:110` shapes every line on each
  prepaint, including lines outside the viewport. The reveal preview's 16,384
  character cap does not apply here. A newline-heavy entry can generate hundreds
  of thousands of shaped lines. Start with an explicit edit-size/line limit and
  informative refusal; longer term, cache layout and shape visible lines.
- **Normalize search once.** `Entry::matches` lowercases each entry and each
  query term on every call. At the supported 50,000-entry limit this adds work
  and allocations to every keystroke on the UI thread. Cache normalized names
  in a store index and tokenize/lowercase each query once. Benchmark with
  fictional large stores before choosing debounce or background search.
- **Remove periodic filesystem work from the UI thread.** Linux wakes the
  application at 100 ms intervals and scans USB sysfs at roughly 250 ms
  intervals, alongside event watchers. Move scans to a coalesced background
  fallback and measure latency before changing hardware scheduling: the prior
  review records a regression from changing this area without live hardware.
- **Avoid rebuilding hidden pages.** `render` constructs the underlying entry
  or card page even while the generator covers it. Render the active content
  through a shared app shell instead.
- **Reuse cached sorting.** The initial backend listing caches sort keys, but
  the desktop insertion sort comparator (`main.rs:622`) lowercases strings
  repeatedly. Use cached keys or insert into the existing sorted list.

These are source-level cost observations, not measured latency improvements.

## Simplification

Extract a clipboard controller, notification model, and pending-operation type
before splitting the remaining views. Those boundaries address the concrete
bugs above and provide testable state transitions. The current footer is an
ordered conditional, not yet a notification controller. Separate the generator
name buffer from the entry editor buffer; their validation and lifetime rules
are different. Use named timing constants by purpose (notification expiry,
secret visibility, clipboard policy, probe deadline), rather than one timeout
for unrelated operations. Share titlebar/window-control builders and replace
the fixed `ADVANCE = 7.8` input hit-testing constant with measured text layout.

## Validation

- Core: 26 tests passed; the disposable real `pass` integration fixture also
  passed outside the sandbox, where its temporary GnuPG agent could start.
- Desktop: 14 Rust tests passed. Python helper tests: 9 passed.
- Core Clippy passed with warnings denied. Desktop Clippy completed with five
  style/structure warnings across normal and test builds.
- Both formatting checks fail on existing formatting differences. No broad
  formatting rewrite was made during this review.
- No GUI interaction, physical-card operations, large-store benchmarks, or
  dependency advisory scan was performed. Existing tests do not cover the
  notification/clipboard UI interactions identified above.

The reviewed core still rejects link/path escapes, bounds plaintext output,
discards output from failed decryptions, cancels obsolete reveal requests,
uses OS randomness, and rejects signed recipient policies pending verification
support. No new direct path to bypassing those protections was identified.

Recommended order: write-conflict protection; cancellable/coordinated copies;
shared visible notifications and clipboard lifecycle; bounded exact editor
input/rendering; then measured search and polling optimizations.
