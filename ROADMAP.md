# Roadmap

Version 1.0.0 has been released. This document records the delivered v1 scope and the post-v1 backlog; remaining candidates are not release blockers. There are no target dates.

## v1.0.0 scope (released)

- Linux desktop, using the user's existing GnuPG keyring, `gpg-agent`, and optional OpenPGP smartcard/YubiKey.
- Existing `pass` store format and commands remain usable alongside the app.
- Local store operations: initialize, browse, search, reveal, copy, edit, delete, generate, and create entries.
- Current built-in visual design and theme configuration. English (`en-US`) is the supported v1 language; the Fluent setup can make later translation possible without making translation a release gate.
- Optimized Linux x86_64 release artifact, checksum, installation instructions, and CI checks.

Git integration, remote sync, cross-platform support, key management, and additional credential-management features are outside the v1 target.

## Delivered capabilities

Implemented:

- Group entries by their existing store folders in an expandable sidebar. Search
  remains global and expands matching folder paths; keyboard navigation follows
  visible folders and entries.
- Copy a hidden entry without displaying it; if already revealed, reuse the
  in-memory contents. `PASSWORD_STORE_CLIP_TIME` controls the timeout (45
  seconds by default); restore the previous clipboard only while the app still
  owns it, and show the countdown.
- Create entries in one composer with explicit New (manual), Pass default
  (pass-compatible random password generated on save), and Memorable phrase
  (editable generated content); manual and memorable text is encrypted exactly
  as entered. Successful replacement clears the composer before encryption
  completes; the composer stays open for the next entry.
- Edit and delete individual entries. The editor is bounded and preserves supported text exactly. Revision checks, explicit conflict choices, and a per-entry lock protect app writes; other store writers do not participate in that lock and may still race the final check and rename.
- Use the existing GnuPG agent and card flow, show operation feedback in a shared notification bar, and report card status. YubiKey model and verification support are available where dependencies and hardware permit.
- Load the English source catalog from Fluent. Completing string migration and adding other languages remain post-v1 work.
- Share titlebar and window-control construction across the main and create
  views; input hit testing and IME cursor placement use measured GPUI text
  layout rather than a fixed character advance.
- Build and publish a Linux x86_64 release artifact and checksum through GitHub Actions.
- Document the Ubuntu 24.04 x86_64 release baseline, runtime libraries,
  graphical pinentry, and optional YubiKey dependencies in the README.
- Initialize a new pass-compatible store from existing GPG recipient IDs through
  one onboarding surface. Missing stores open onboarding; valid stores bypass it.
  The same flow is available as a non-destructive `--onboarding-demo` preview.

## v1.0.0 verification record

The v1 acceptance and release checks are complete:

- [x] The disposable compatibility fixture verified that initialized stores
  work with `pass`, preserve existing entries, and support detached recipient
  signatures.
- [x] Onboarding, non-destructive preview mode, empty-store creation,
  existing-store loading, non-empty-target protection, and signed initialization
  were exercised.
- [x] Real-store workflows and YubiKey hardware behavior were exercised by the
  maintainer, including reveal, hidden copy, PIN/touch, retry, and verification.
- [x] GUI checks covered normal and compact sizes, themes, search, keyboard
  navigation, empty/no-results states, and styled GnuPG errors.
- [x] CI checks, release artifact installation, checksum verification,
  headless GUI launch, and published provenance verification passed.
- [x] README and security documentation describe the tested Ubuntu 24.04
  x86_64 baseline and known limits.

Manual validation reflects the maintainer's hardware and Linux setup; it is not
a claim of coverage across all distributions or devices.

## Post-v1 backlog

These are future improvements, not commitments or release blockers.

### Code quality and maintainability

The earlier splits reduced several large modules, but these Rust files still
exceed 300 lines. Split only where a cohesive responsibility boundary improves
readability; 300 lines is a guideline, not a target that justifies fragmentation.

- `src/decrypt.rs`
- `desktop/src/app/entry_operations.rs`
- `desktop/src/input/text_input_views.rs`
- `desktop/src/views/card_status_view.rs`
- `desktop/src/lib.rs`
- `desktop/src/views/entry_detail_view.rs`
- `desktop/src/ui/sidebar_scroll.rs`

### GnuPG smart-card coexistence

The app currently keeps a GnuPG smart-card watcher alive and performs
background `--card-status all` probes. These can contend with an external
GnuPG operation, such as Git commit signing, and may leave `gpg-agent` or
`scdaemon` needing a card replug before the operation succeeds. This is a
post-v1 reliability follow-up, not a v1 feature requirement.

- On Linux, prefer the existing udev/USB watcher for card insertion and
  removal instead of maintaining a persistent GnuPG smart-card connection.
- Keep GnuPG status probes one-shot and only run them when card details are
  needed; avoid unnecessary probes during app crypto operations.
- Verify coexistence with Git signing, including repeated insert/remove and
  concurrent app/external GnuPG workflows.

### YubiKey setup and store migration

This is a post-v1 feature. V1 consumes an existing GnuPG/YubiKey setup and does
not create, import, transfer, or provision private keys or PIN policy.
Any future workflow must make the distinction between hardware verification,
GnuPG readiness, and store migration explicit.

- Start with a read-only readiness wizard covering card detection, reader
  access, GnuPG support, key slots, PIN retry state, and agent configuration.
- Add guided migration to an already-existing card-backed key: require a
  recoverable backup, add the recipient to `.gpg-id`, re-encrypt entries,
  verify decrypt/reveal, and retain an explicit rollback path before removing
  the old recipient.
- Consider optional key generation directly on the card only after the
  readiness and migration workflows are safe; delegate PIN and touch policy
  to GnuPG/pinentry.
- Never make key generation, private-key transfer, recipient-policy changes,
  or loss of the previous recipient an implicit one-click action.

### Manual store backup and restore

This is an after-v1, non-blocking feature. It will provide an explicit
export/import workflow for encrypted store entries, independent of Git or sync.
The initial format is intentionally not a complete clone of a `pass` store.

- Use a versioned `.tar.gz` containing `manifest.toml` and a
  `password-store/` tree. Backwards compatibility with existing personal
  scripts is not a requirement.
- Include only non-hidden regular files and directories under the store.
  Exclude all dotfiles and dot-directories, including `.gpg-id`, `.git`, and
  `.extensions`, for the initial version. Preserve encrypted file bytes and
  relative paths. Restore must warn that recipient policy metadata was omitted
  and that writes require the user to re-establish `.gpg-id` or
  `PASSWORD_STORE_KEY`.
- Make public-key export, private-key export, and ownertrust export separate
  explicit choices. Ownertrust affects the user's GnuPG trust database and may
  be useful even when keys are already present; importing it must have its own
  warning and result.
- Leave archive encryption out of the initial version. Exported archives still
  receive restrictive local permissions and a clear warning that key-bearing
  archives are sensitive.
- Validate archive paths, file types, size, manifest version, and contents
  before any store or GnuPG changes. Reject traversal, links, device files,
  unexpected dotfiles, and malformed or incomplete archives.
- Preview the restore target and conflicts. Restore into a new or empty
  location by default; require a deliberate, recoverable action before
  replacing anything.
- Stage and install the store before importing optional keys or ownertrust,
  then report each import result independently. Do not claim a transactional
  rollback for GnuPG changes.
- Add tests for malformed archives, traversal and link entries, dotfile
  exclusion, partial failures, existing destinations, optional key/ownertrust
  components, and round-trip preservation.

### QR-code display for secret values

This is an after-v1, non-blocking feature. Upstream `pass` supports
`show --qrcode`/`-q` and `generate --qrcode`/`-q`; both encode the selected
password with `qrencode` instead of printing it. The client should provide the
same useful workflows without requiring the `pass` executable or an external
display helper.

#### Proposed scope

- Add an explicit **Show QR code** action for the selected existing entry.
- Add the same action to the generated-password result. A generated password
  is one line, so the generated value is the complete payload.
- For an existing entry, encode only its first line by default. This matches
  `pass`'s normal password convention and prevents usernames, URLs, notes, and
  other multiline metadata from being disclosed accidentally.
- Render the code in an in-app modal or overlay with the entry name and a
  concise warning that anyone who can see the code can read the secret.
- Generate and render locally with a maintained Rust/library path. Do not
  invoke `qrencode`, write an image or plaintext temporary file, use the
  clipboard, make a network request, or add data to logs.

#### Security and failure behavior

- Decrypt only after the user explicitly requests the action; keep the
  plaintext/encoded image within the existing secret lifecycle.
- Hide and discard the QR view on Escape, selection or search changes, refresh,
  window deactivation, timeout, and cancellation, matching reveal behavior.
- Keep the existing reveal and hidden-copy actions available; QR display is not
  an alternate authentication or storage path.
- Encode the exact first-line UTF-8 value. If the encoder cannot represent the
  payload within its supported QR capacity, show a bounded error and do not
  fall back to displaying or copying the secret.
- Never place the payload in accessibility labels, status messages, error
  strings, diagnostics, or telemetry.

#### Acceptance criteria

- A selected multiline entry produces a scannable code for its first line only;
  generated passwords produce a scannable code for the generated value.
- The modal has no plaintext fallback, and all documented hide/cancel paths
  remove the QR view and associated secret state.
- Oversized or otherwise unencodable values fail visibly without changing the
  store or clipboard.
- Tests cover exact payload selection, multiline exclusion, generated values,
  lifecycle cancellation, and the oversized-payload failure. Manual
  verification scans a fictional QR code and checks that no external process,
  file, or network access is required.

The initial version deliberately excludes arbitrary line selection, whole-entry
QR payloads, QR import/scanning, OTP-specific parsing, and QR export to files.

### Additional after-v1 follow-ups

- Move and duplicate entries, with recipient-policy-aware re-encryption.
- Recipient and missing-key diagnostics.
- Complete the Fluent migration and add maintained translations.
- Awareness of external store changes.
- Folder navigation refinements such as expand/collapse all and
  breadcrumbs for deeply nested paths.
- Search refinements such as fuzzy matching, full-path matching, and
  inline match highlighting while preserving global search across collapsed
  folders.
- Store health diagnostics for malformed entry paths, unreadable ciphertext,
  missing recipient/key policy, and orphaned policy files without exposing
  plaintext.
- Generator profiles by folder and named store profiles.
- Credential rotation sessions, hand-typing view, quick-copy fields, and local TOTP.
- Export a single entry to another recipient and local links that open a selected entry.
- Application lock and re-authentication that hide rendered secrets and require
  a fresh GnuPG operation, while documenting that the app cannot purge
  authorization cached by the user's GnuPG agent.
- Bulk selection and explicit multi-entry operations such as delete, move,
  export, or recipient-policy-aware re-encryption.
- Entry history and diff viewing when local history is available, without
  making Git management part of the application.
- Complete keyboard tab order for sidebar rows, titlebar actions, buttons, and
  dialogs; add accessibility semantics and screen-reader state announcements.
- Desktop integration beyond local links, such as launcher actions and optional
  tray or quick-copy workflows.
- Investigate macOS support; evaluate Windows only after reviewing safe file and subprocess handling.
- Favorites and recent entries, storing only entry paths in local app state and
  never caching decrypted contents.

### Broader Linux distribution support

Support beyond the Ubuntu 24.04 x86_64 release baseline is a post-v1 candidate:

- Define supported distro families, minimum glibc, CPU architectures, desktop
  sessions, and host graphics requirements.
- Build against the oldest supported glibc baseline so newer symbol versions do
  not make the binary unusable on older distributions.
- Prototype an AppImage alongside the tarball and assess which user-space
  libraries it can bundle versus host requirements such as GnuPG, pinentry,
  GPU drivers, and desktop portals. Compare maintenance and portability with
  distro-native packages and Flatpak before choosing a primary format; keep the
  tarball until the alternative is verified.
- Test installation and normal workflows across representative Debian/Ubuntu,
  Fedora, and Arch environments under both X11 and Wayland where supported.
- Keep GnuPG, graphical pinentry, YubiKey permissions, Vulkan drivers, and
  desktop portals explicit as host requirements rather than hiding them behind
  unsupported bundling.

## Ideas to revisit

These are exploratory and not current targets:

- Extract read-only USB/udev CCID detection into a standalone FOSS crate after checking existing Rust crates and defining a useful, narrow API. It must not open PC/SC or take over GnuPG card access.
- Search decrypted entry contents, only with clear opt-in behavior and a reviewed secret-lifetime model.

## v1 scope boundaries

These items were deliberately excluded from v1; relevant follow-ups are listed
above:

- Git history management, remotes, push/pull, or automatic synchronization.
- Automatic cloud backup or synchronization.
- Managing, creating, rotating, or provisioning GPG keys and smartcards.
- Windows or macOS release support.
- Full localization beyond the maintained English catalog.

## v1 release status

Version 1.0.0 is published. The release artifact, checksum, installation,
headless GUI launch, and provenance verification passed. There are no remaining
v1 release gates; future work is tracked in the post-v1 backlog.
