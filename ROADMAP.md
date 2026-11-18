# Roadmap to v1

The goal is a dependable Linux desktop client for an existing `pass` store. A v1 release should make the supported workflows clear, protect against ordinary data loss, and be straightforward to install and verify. There are no target dates.

## v1 scope

- Linux desktop, using the user's existing GnuPG keyring, `gpg-agent`, and optional OpenPGP smartcard/YubiKey.
- Existing `pass` store format and commands remain usable alongside the app.
- Local store operations: browse, search, reveal, copy, edit, delete, generate, and create entries.
- Current built-in visual design and theme configuration. English (`en-US`) is the supported v1 language; the Fluent setup can make later translation possible without making translation a release gate.
- Optimized Linux x86_64 release artifact, checksum, installation instructions, and CI checks.

Git integration, remote sync, cross-platform support, key management, and additional credential-management features are outside the v1 target.

## Current baseline

Implemented:

- Browse nested entries, search names without decrypting, reveal multiline entries, and hide secrets on timeout, selection changes, Escape, and window deactivation.
- Copy the complete entry without displaying it. `PASSWORD_STORE_CLIP_TIME` controls the timeout (45 seconds by default); the app restores the previous clipboard contents when it still owns the clipboard and shows a countdown.
- Create entries in one composer with explicit New (manual), Pass default
  (pass-compatible random password generated on save), and Memorable phrase
  (editable generated content); manual and memorable text is encrypted exactly
  as entered. Successful replacement clears the composer before encryption
  completes; the composer stays open for the next entry.
- Edit and delete individual entries. The editor is bounded and preserves supported text exactly. Revision checks, explicit conflict choices, and a per-entry lock protect app writes; other store writers do not participate in that lock and may still race the final check and rename.
- Use the existing GnuPG agent and card flow, show operation feedback in a shared notification bar, and report card status. YubiKey model and verification support are available where dependencies and hardware permit.
- Load optional TOML theme settings, with square corners in the default theme. User-facing text is being moved into an embedded `en-US` Fluent catalog.
- Share titlebar and window-control construction across the main and create
  views; input hit testing and IME cursor placement use measured GPUI text
  layout rather than a fixed character advance.
- Build and publish a Linux x86_64 release artifact and checksum through GitHub Actions.

The maintainer reports having exercised the normal workflows against a real
password store throughout development and confirms they work as expected so
far. This includes store selection, nested search, reveal/hide, hidden copy and
clipboard restoration, create and replacement, edit conflicts, delete,
generation, operation feedback, cleanup during navigation and shutdown, and
handling of GnuPG errors and failed writes. This is hands-on validation on the
maintainer's setup, not a claim of coverage across all Linux environments or
hardware.

## Remaining work before v1

### 1. Close the v1 reliability gaps

- Keep the maintainer's real-store workflow validation current as features change; record any environment-specific gaps rather than treating one setup as universal coverage.
- Keep external-writer conflict behavior explicit: preserve edits and warn on changes observed before replacement. Document the remaining narrow race with non-cooperating tools such as `pass` unless a portable coordination mechanism is added.
- Update or archive the 2026-11-14 source review and 2026-11-15 follow-up so their findings and validation counts are not mistaken for the current state.

### 2. Define and verify the supported release

- Specify the tested Linux baseline and runtime requirements, including GnuPG, graphical pinentry, graphics stack, and optional YubiKey packages.
- Verify a clean optimized build, packaged artifact contents, checksum instructions, and a fresh install/run from a release download.
- Run CI checks and integration fixtures from a clean checkout. Keep release notes and README status aligned with what was actually tested.
- Perform a final GUI pass at normal and compact window sizes, with both light and dark themes, using fictional data. Exercise keyboard navigation and accessibility-relevant focus states.
- Perform and record a hardware check for card detection, PIN/touch prompts, reveal, hidden copy, removal/reconnection, and retry. Clearly separate tested behavior from unsupported cards or environments.
- Review the security and privacy documentation against actual behavior, especially clipboard-manager history, crash cleanup limits, logs, filesystem permissions, GnuPG configuration, and the limits of local-user authentication.

## After v1

These are worthwhile follow-ups but are not release blockers:

### GnuPG smart-card coexistence

The app currently keeps a GnuPG smart-card watcher alive and performs
background `--card-status all` probes. These can contend with an external
GnuPG operation, such as Git commit signing, and may leave `gpg-agent` or
`scdaemon` needing a card replug before the operation succeeds. This is an
after-v1 reliability follow-up, not a v1 feature requirement.

- On Linux, prefer the existing udev/USB watcher for card insertion and
  removal instead of maintaining a persistent GnuPG smart-card connection.
- Keep GnuPG status probes one-shot and only run them when card details are
  needed; avoid unnecessary probes during app crypto operations.
- Verify coexistence with Git signing, including repeated insert/remove and
  concurrent app/external GnuPG workflows.

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

- Optional OS reauthentication before replacing an existing entry.
- Move and duplicate entries, with recipient-policy-aware re-encryption.
- Recipient and missing-key diagnostics.
- Complete the Fluent migration and add maintained translations.
- Awareness of external store changes.
- Generator profiles by folder and named store profiles.
- Credential rotation sessions, hand-typing view, quick-copy fields, and local TOTP.
- Export a single entry to another recipient and local links that open a selected entry.
- Investigate macOS support; evaluate Windows only after reviewing safe file and subprocess handling.
- Favorites and recent entries, storing only entry paths in local app state and
  never caching decrypted contents.

## Ideas to revisit

These are exploratory and not current targets:

- Extract read-only USB/udev CCID detection into a standalone FOSS crate after checking existing Rust crates and defining a useful, narrow API. It must not open PC/SC or take over GnuPG card access.
- Search decrypted entry contents, only with clear opt-in behavior and a reviewed secret-lifetime model.

## Explicitly out of scope for v1

- Git history management, remotes, push/pull, or automatic synchronization.
- Automatic cloud backup or synchronization.
- Managing, creating, rotating, or provisioning GPG keys and smartcards.
- Windows or macOS release support.
- Full localization beyond the maintained English catalog.

## Release completion checklist

V1 is ready when all advertised local workflows pass the documented checks; detected edit conflicts preserve the user's work; the narrow race with non-cooperating external writers is documented; the supported Linux release installs and runs from its published artifact; and the README, security notes, and hardware-validation record accurately describe the release and its limits.
