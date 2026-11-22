# Security boundary

This prototype has not received an independent security review. CI runs the core,
desktop, Python helper, and disposable real `pass`/GnuPG checks; current
completion status and test policy are tracked in [ROADMAP.md](ROADMAP.md). These
checks do not replace graphical pinentry, GUI, or physical-card verification.

## Assumptions

The OS, GPUI/dependencies, user-selected GnuPG executable, and GnuPG home/agent configuration are trusted. The application aims to prevent unnecessary plaintext exposure, accidental store mutation, command injection through entry names, path escapes, late asynchronous reveals, unbounded output, and display of failed/integrity-invalid decryptions.

It does not defend against a compromised account, privileged inspection, malicious compositor, keylogger, screenshots, or observation during reveal. Filenames, directory structure, and repository history are plaintext metadata; OpenPGP encryption does not authenticate those names or Git history.

## Implemented controls

- Filename-only browsing. No eager decryption or content indexing.
- Pinned directory handles for traversal and opening. Every component rejects symlinks; entries must be regular files with a single hard link. Replacing the store path cannot redirect an already opened store.
- Encrypted files enter GnuPG on stdin. Entry names are never shell text or command arguments. Stdout remains a separate private plaintext pipe. Stderr carries `--status-fd 2` messages and diagnostics; a bounded allowlist parser maps recognized events to fixed application messages, then discards raw data.
- `--no-options` prevents `gpg.conf` output redirection. Agent pinentry is explicit; automatic key retrieval/import is disabled. No loopback passphrase handling.
- YubiKey/OpenPGP smartcard operations remain with the existing agent and scdaemon. The app inherits the GnuPG home; it does not handle PINs, change card policies, or manage keys.
- Nonblocking reads into a fixed-size zeroizing buffer, 1 MiB output cap, 120-second deadline, and cancellation polling. The subprocess is killed/reaped on every exit path. Failed exit status discards all plaintext, including output emitted before integrity failure.
- Secret buffers have no Clone or Display implementation and Debug is redacted. Secret text must be valid UTF-8.
- Reveal requests carry entry and request identities. Selection/search changes, refresh, hide, and destruction invalidate them. Late results cannot repopulate another entry.
- Copy requests decrypt the complete entry in the background without adding their result to reveal state or rendering it. A result is copied only if its entry remains selected and the window is active. Pending copies retain cancellation tokens and are invalidated on selection, search, refresh, hide, and generator navigation changes; they participate in card-operation coordination.
- Every operation-status reporter is a separate latest-value slot containing only static application text. Raw diagnostics, key IDs, serial numbers, filenames, and secret contents are never forwarded. The parser has 4 KiB line and scratch buffers, drops oversized lines completely, and bounds work per poll so output floods cannot starve cancellation. Parser buffers are zeroized; progress does not authorize plaintext display or bypass the GnuPG exit-status check.
- Visible content expires after 30 seconds and clears on deactivation. Pending reveal/copy pinentry may continue while unfocused, but results are accepted only while the app is active; otherwise the user must retry.
- Generated phrases select words, digits, and separators independently and uniformly using OS-provided cryptographic randomness with rejection sampling. The UI estimates entropy from those configured choices. The pass-default mode uses the standard 25-character punctuation/alphanumeric alphabet and honors the generated-length and character-set environment overrides supported by the app. Backend secret buffers are zeroized when they expire, the generator closes, or the window deactivates. Saving a generated password copies it automatically using the shared clipboard timeout and restoration policy.
- Linux startup disables core dumps and marks the process non-dumpable. Failure prevents startup.
- Generated and user-authored entry creation and editing encrypt content before writing; custom entry contents are not displayed after saving. Existing entries require explicit confirmation before replacement, and replacement checks the encrypted file revision before commit. Deletion removes ciphertext. No shell commands, extensions, Git operations, telemetry, application network code, or plaintext temporary files are used.

For a copied existing entry, the app retains the prior GPUI clipboard item and restores it after `PASSWORD_STORE_CLIP_TIME` seconds (45 by default) only if the clipboard still contains the exact password it copied. If another app replaces the clipboard, that newer value is left alone. Normal app shutdown restores a tracked copy while preserving newer clipboard contents. Crashes or forced termination may leave the copied text on the clipboard. Desktop clipboard managers and synchronization tools may retain their own histories, which the app cannot erase. Generated passwords and editor-selection copies use the same clipboard policy; hiding generated text does not end its clipboard lease. New passwords are encrypted before writing. Recipient selection follows `PASSWORD_STORE_KEY` or the nearest existing `.gpg-id`. When recipients come from `.gpg-id` and `PASSWORD_STORE_SIGNING_KEY` is set, that file must have a detached signature verified by GnuPG, and the reported primary-key fingerprint must match one of the configured full fingerprints. An explicit `PASSWORD_STORE_KEY` override bypasses `.gpg-id` lookup, matching `pass`. The exact recipient and signature bytes are checked again immediately before writing. Resulting ciphertext is written with mode 0600 via a temporary file and atomic no-overwrite link. Newly created folder permissions are restricted to the user. Automatic Git staging/commit is not implemented.

Removing an entry unlinks only the selected regular `.gpg` file through pinned directory handles. It refuses symlinks and hardlinked entry files. This is a normal filesystem deletion, not secure erasure; Git history, snapshots, backups, and filesystem recovery may retain ciphertext.

## Memory and rendering limits

Backend-owned plaintext is zeroized on drop. GPUI receives owned text for shaping/rendering; its text caches, allocator copies, GPU buffers, and compositor surfaces are outside that zeroization boundary. Hiding removes content from application state and rendering, but does not guarantee immediate erasure of every copy. Memory is not locked against swapping. Long entries are truncated in the visual preview; underlying ciphertext is unchanged.

GnuPG may cache key authorization independently. Hiding the UI does not purge or lock the agent. The application never kills the user's normal agent.

Card removal does not currently trigger immediate clearing of already visible content; focus, explicit hide, and timeout still apply. Fresh decryption follows GnuPG's card/PIN/touch policy. Requests are not silently retried, and the app never submits a PIN.

PIN-launch and card-busy events are hints, not a reliable indication of PIN
completion or a required touch. Older GnuPG paths may omit card progress
entirely. The fallback after two seconds therefore says to touch only **if the
YubiKey is blinking**, and to complete any open system prompt first. The app
also performs background `gpg --card-status all` probes for card details; these
can contend with external GnuPG operations and are tracked as an after-v1
coexistence issue. It does not alter agent configuration or monitor unrelated
applications' operations.

## Compatibility boundary

Custom entry creation accepts up to 16 KiB and 512 lines, preserves text exactly as entered, and does not display it after saving. New entries use atomic no-overwrite creation; confirmed replacements use revision-checked atomic replacement.

Decryption follows the ciphertext's recipients, so browsing does not need to interpret `.gpg-id`. Multiline text is preserved; first-line password and remaining content are presentation conventions. Binary entries cannot be displayed. Generated phrases use EFF Short Word List #2, version 0 (1,296 words); attribution is in `assets/EFF-WORDLIST-LICENSE.md`.

New-entry writes use nearest-ancestor recipients, multiple recipients, `PASSWORD_STORE_KEY`, restricted directory/file permissions, and atomic no-overwrite creation. Edited entries are encrypted before an atomic replacement within the pinned store directory. The GUI captures a file revision from the actual decrypted file and checks identity, size, and change timestamps before encryption and again before replacement. A bounded per-entry lock in the user runtime directory coordinates `gpui-pass` instances. On a detected conflict, the editor preserves the user's text and offers explicit overwrite or discard-and-reveal actions; it does not merge the two versions. This remains optimistic detection, not an atomic compare-and-swap: non-cooperating writers such as `pass` can still race between the final check and rename. Git staging/commit semantics remain unimplemented; generated entries are not automatically added to Git.

### External writer race

The revision check protects against changes observed before encryption and
before the app's final replacement step. The per-entry runtime lock coordinates
only `gpui-pass` processes; tools such as `pass`, editors, scripts, or other
filesystem writers do not participate in it.

An external writer can therefore modify the entry after the final check but
before the atomic rename. In that narrow window, the app can replace the
external writer's newer ciphertext. There is no portable filesystem
compare-and-swap or merge operation here. Detected conflicts preserve the
editor buffer and require an explicit overwrite or discard-and-reveal choice;
undetected races cannot be recovered by the app. Avoid editing the same entry
concurrently from the app and another tool, and use Git history or another
backup as recovery when available.

## Outstanding verification

Automated tests exercise name/path rejection, Unicode search, symlink omission and replacement, hardlinks, pinned-root replacement, metacharacters, multiline output, failing exit status after plaintext output, binary/oversized output, timeout, cancellation, stale results, focus transitions, and expiry. Status tests cover partial/oversized lines, ignored diagnostics and metadata, live card-wait hints, fallback without status support, stderr floods, cancellation under flooding, and clearing/isolation of per-operation status.

The disposable real `pass` fixture is part of CI. The maintainer has manually
confirmed hidden copy, reveal/hide, edit, delete, confirmed replacement, and
signed onboarding with a hardware-backed store and YubiKey. Still outstanding
for v1 are the final GUI matrix at normal/compact sizes and light/dark themes,
keyboard/focus coverage, explicit PIN-cancellation and reconnect cases, and
manual verification against genuinely tampered OpenPGP ciphertext.

YubiKey verification on the maintainer's setup covers insertion/removal,
PIN/touch, reveal, hidden copy, retry, and device verification. This does not
establish coverage across other Linux environments, pinentry implementations,
hardware models, or concurrent external GnuPG operations.
