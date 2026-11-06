# Security boundary

This prototype has not received an independent security review. The GPUI frontend passes a Linux compiler check but still needs runtime verification, as does real GnuPG/pinentry integration. The 15 backend tests cover backend behavior and controlled subprocess fixtures.

## Assumptions

The OS, GPUI/dependencies, user-selected GnuPG executable, and GnuPG home/agent configuration are trusted. The application aims to prevent unnecessary plaintext exposure, accidental store mutation, command injection through entry names, path escapes, late asynchronous reveals, unbounded output, and display of failed/integrity-invalid decryptions.

It does not defend against a compromised account, privileged inspection, malicious compositor, keylogger, screenshots, or observation during reveal. Filenames, directory structure, and repository history are plaintext metadata; OpenPGP encryption does not authenticate those names or Git history.

## Implemented controls

- Filename-only browsing. No eager decryption or content indexing.
- Pinned directory handles for traversal and opening. Every component rejects symlinks; entries must be regular files with a single hard link. Replacing the store path cannot redirect an already opened store.
- Encrypted files enter GnuPG on stdin. Entry names are never shell text or command arguments. Stdout is private; stderr is discarded.
- `--no-options` prevents `gpg.conf` output redirection. Agent pinentry is explicit; automatic key retrieval/import is disabled. No loopback passphrase handling.
- YubiKey/OpenPGP smartcard operations remain with the existing agent and scdaemon. The app inherits the GnuPG home; it does not handle PINs, change card policies, or manage keys.
- Nonblocking reads into a fixed-size zeroizing buffer, 1 MiB output cap, 120-second deadline, and cancellation polling. The subprocess is killed/reaped on every exit path. Failed exit status discards all plaintext, including output emitted before integrity failure.
- Secret buffers have no Clone or Display implementation and Debug is redacted. Secret text must be valid UTF-8.
- Reveal requests carry entry and request identities. Selection/search changes, refresh, hide, and destruction invalidate them. Late results cannot repopulate another entry.
- Visible content expires after 30 seconds and clears on deactivation. Pending pinentry may continue while unfocused, but completion is accepted only while the app is active; otherwise the user must reveal again.
- Linux startup disables core dumps and marks the process non-dumpable. Failure prevents startup.
- No entry writes, shell commands, extensions, Git operations, clipboard access, telemetry, application network code, or plaintext temporary files.

## Memory and rendering limits

Backend-owned plaintext is zeroized on drop. GPUI receives owned text for shaping/rendering; its text caches, allocator copies, GPU buffers, and compositor surfaces are outside that zeroization boundary. Hiding removes content from application state and rendering, but does not guarantee immediate erasure of every copy. Memory is not locked against swapping. Long entries are truncated in the visual preview; underlying ciphertext is unchanged.

GnuPG may cache key authorization independently. Hiding the UI does not purge or lock the agent. The application never kills the user's normal agent.

Card removal does not currently trigger immediate clearing of already visible content; focus, explicit hide, and timeout still apply. Fresh decryption follows GnuPG's card/PIN/touch policy. Requests are not silently retried, and the app never submits a PIN.

## Compatibility boundary

Decryption follows the ciphertext's recipients, so this read-only client does not need to interpret `.gpg-id`. Multiline text is preserved; first-line password and remaining content are presentation conventions. Binary entries cannot be displayed.

Future writes must implement nearest-ancestor recipients, multiple recipients, `PASSWORD_STORE_KEY`, signed-recipient policies, permissions, atomic replacement, concurrent modification handling, and Git semantics. These policies must not be inferred from the read-only implementation.

## Outstanding verification

Automated tests exercise name/path rejection, Unicode search, symlink omission and replacement, hardlinks, pinned-root replacement, metacharacters, multiline output, failing exit status after plaintext output, binary/oversized output, timeout, cancellation, stale results, focus transitions, and expiry.

Still required: run GPUI on Linux; validate IME, navigation, focus and rendering cleanup; run the real `pass` fixture; test a protected key with graphical pinentry and genuinely tampered OpenPGP ciphertext. Disposable-agent startup is blocked in the current environment, and GUI/hardware runtime behavior remains unverified.

YubiKey hardware verification must cover insertion/removal, correct-PIN and cancelled-PIN flows, touch-required requests, pinentry focus transitions, stale completion, and retry after reconnect. Controlled subprocess tests do not replace these checks.
