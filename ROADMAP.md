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
6. **Add backup and export flows.** Keep encrypted backups distinct from plaintext exports. Make destinations and overwrite behavior clear, and protect plaintext export files with restrictive permissions.
7. **Improve key and store diagnostics.** Show which recipients protect a store or entry and help users diagnose missing keys without managing or transferring private key material.

## Later

- Add native macOS support after validating GnuPG discovery, pinentry, fonts, and keyboard behavior.
- Evaluate Windows support with reviewed file-access and subprocess handling.

## Ideas to revisit

These are recorded for future consideration and are not current product targets:

- Quick-copy conventional fields such as usernames or URLs from an entry.
- Local TOTP setup and code display.
- Opt-in search across decrypted entry contents.

## Verification work

- Run the real `pass` integration fixture in an environment that can start a disposable `gpg-agent`.
- Validate behavior with graphical pinentry and tampered OpenPGP ciphertext.
- Keep the Linux GUI and hardware-card workflows covered during changes to reveal, cancellation, and card detection.
