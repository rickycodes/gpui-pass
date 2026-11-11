# Roadmap

This roadmap describes the next product work. It has no delivery dates; order reflects dependencies and risk.

## Current baseline

- Browse and search an existing `pass` store, reveal multiline entries, and edit or delete individual entries.
- Generate random passwords or memorable phrases and save them encrypted to the store's configured recipients.
- Use the existing GnuPG agent and OpenPGP card, with PIN and touch flows handled by the user's configured tools.

## Planned

1. **Complete safe write policy handling.** Verify signed `.gpg-id` recipient policies before writing. Detect concurrent file changes before replacing an edited entry, and report conflicts without losing either version.
2. **Add Git support.** Stage and commit app-created, edited, and deleted entries. Add explicit sync actions with clear conflict handling; never silently overwrite local changes.
3. **Add backup and export flows.** Keep encrypted backups distinct from plaintext exports. Make destinations and overwrite behavior clear, and protect plaintext export files with restrictive permissions.
4. **Improve key and store diagnostics.** Show which recipients protect a store or entry and help users diagnose missing keys without managing or transferring private key material.

## Later

- Add native macOS support after validating GnuPG discovery, pinentry, fonts, and keyboard behavior.
- Evaluate Windows support with reviewed file-access and subprocess handling.

## Verification work

- Run the real `pass` integration fixture in an environment that can start a disposable `gpg-agent`.
- Validate behavior with graphical pinentry and tampered OpenPGP ciphertext.
- Keep the Linux GUI and hardware-card workflows covered during changes to reveal, cancellation, and card detection.
