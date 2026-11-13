# Roadmap

This roadmap describes the next product work. It has no delivery dates; order reflects dependencies and risk.

## Current baseline

- Browse and search an existing `pass` store, reveal multiline entries, and edit or delete individual entries.
- Copy an entry's complete contents without revealing it, with `PASSWORD_STORE_CLIP_TIME` timeout and prior clipboard restoration.
- Generate random passwords or memorable phrases and save them encrypted to the store's configured recipients.
- Use the existing GnuPG agent and OpenPGP card, with PIN and touch flows handled by the user's configured tools and copy/reveal progress shown in the app.

## Planned

1. **Complete safe write policy handling.** Verify signed `.gpg-id` recipient policies before writing. Detect concurrent file changes before replacing an edited entry, and report conflicts without losing either version.
2. **Create custom entries.** Add a secure in-app flow for creating user-written single-line or multiline entries, with explicit overwrite confirmation.
3. **Move and duplicate entries.** Support individual entries and directories. Re-encrypt entries when the destination's recipient policy differs, and handle name conflicts without silently overwriting data.
4. **Add Git support.** Stage and commit app-created, edited, moved, copied, and deleted entries. Add explicit sync actions with clear conflict handling; never silently overwrite local changes.
5. **Add a copy shortcut.** Provide a keyboard shortcut for copying the selected entry without revealing it.
6. **Add configurable UI themes.** Move visual values into semantic design tokens and load an optional user theme config for colors, spacing, and corner radii. Keep the current appearance as the default; validate values and fall back safely when configuration is missing or invalid.
7. **Add full-store backup and restore.** Provide a manual archive workflow, separate from Git and remote sync. Export the encrypted store and, for a complete migration, the selected GnuPG secret keys and ownertrust. Treat archives containing private key material as highly sensitive, use restrictive permissions, and validate archive paths and contents before restoring. Stage and review a restore before changing the destination; preserve an existing store instead of silently replacing it. Support inspecting or restoring the store and key material as deliberate steps.
8. **Improve key and store diagnostics.** Show which recipients protect a store or entry and help users diagnose missing keys without managing or transferring private key material.

## Later

- Add native macOS support after validating GnuPG discovery, pinentry, fonts, and keyboard behavior.
- Evaluate Windows support with reviewed file-access and subprocess handling.

## Ideas to revisit

These are recorded for future consideration and are not current product targets:

- Consider extracting read-only USB/udev CCID presence detection into a standalone FOSS crate. Keep it focused on detecting device presence without opening PC/SC or taking over GnuPG's card access; assess existing Rust device-discovery crates before committing to the extraction.
- Quick-copy conventional fields such as usernames or URLs from an entry.
- Local TOTP setup and code display.
- Opt-in search across decrypted entry contents.

## Verification work

- Run the real `pass` integration fixture in an environment that can start a disposable `gpg-agent`.
- Validate behavior with graphical pinentry and tampered OpenPGP ciphertext.
- Keep the Linux GUI and hardware-card workflows covered during changes to reveal, cancellation, and card detection.
