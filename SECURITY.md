# Security boundary

This prototype has not received an independent security review. The GPUI frontend builds on Linux but still needs runtime verification, as does real GnuPG/pinentry integration. The 22 backend tests cover backend behavior and controlled subprocess fixtures.

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
- Every operation-status reporter is a separate latest-value slot containing only static application text. Raw diagnostics, key IDs, serial numbers, filenames, and secret contents are never forwarded. The parser has 4 KiB line and scratch buffers, drops oversized lines completely, and bounds work per poll so output floods cannot starve cancellation. Parser buffers are zeroized; progress does not authorize plaintext display or bypass the GnuPG exit-status check.
- Visible content expires after 30 seconds and clears on deactivation. Pending pinentry may continue while unfocused, but completion is accepted only while the app is active; otherwise the user must reveal again.
- Generated phrases select words, digits, and separators independently and uniformly using OS-provided cryptographic randomness with rejection sampling. The UI estimates entropy from those configured choices. The pass-default mode uses the standard 25-character punctuation/alphanumeric alphabet and honors the generated-length and character-set environment overrides supported by the app. Backend secret buffers are zeroized when they expire, the generator closes, or the window deactivates. Copying requires an explicit button press; after 30 seconds, the app clears the clipboard only when it still contains the exact phrase it copied.
- Linux startup disables core dumps and marks the process non-dumpable. Failure prevents startup.
- No entry writes, shell commands, extensions, Git operations, telemetry, application network code, or plaintext temporary files.

Clipboard contents can also be retained by desktop clipboard managers or synchronization tools. The app cannot erase those external histories, and closing the app before the timer fires leaves its current clipboard value untouched. New passwords are encrypted before writing. Recipient selection follows `PASSWORD_STORE_KEY` or the nearest existing `.gpg-id`, and the resulting ciphertext is written with mode 0600 via a temporary file and atomic no-overwrite link. Newly created folder permissions are restricted to the user. If `PASSWORD_STORE_SIGNING_KEY` is set, entry creation fails closed because signed recipient policy verification is not yet implemented. Automatic Git staging/commit is not implemented.

## Memory and rendering limits

Backend-owned plaintext is zeroized on drop. GPUI receives owned text for shaping/rendering; its text caches, allocator copies, GPU buffers, and compositor surfaces are outside that zeroization boundary. Hiding removes content from application state and rendering, but does not guarantee immediate erasure of every copy. Memory is not locked against swapping. Long entries are truncated in the visual preview; underlying ciphertext is unchanged.

GnuPG may cache key authorization independently. Hiding the UI does not purge or lock the agent. The application never kills the user's normal agent.

Card removal does not currently trigger immediate clearing of already visible content; focus, explicit hide, and timeout still apply. Fresh decryption follows GnuPG's card/PIN/touch policy. Requests are not silently retried, and the app never submits a PIN.

PIN-launch and card-busy events are hints, not a reliable indication of PIN completion or a required touch. Older GnuPG paths may omit card progress entirely. The fallback after two seconds therefore says to touch only **if the YubiKey is blinking**, and to complete any open system prompt first. The app does not probe the card through a competing process, alter agent configuration, or monitor unrelated applications' operations.

## Compatibility boundary

Decryption follows the ciphertext's recipients, so browsing does not need to interpret `.gpg-id`. Multiline text is preserved; first-line password and remaining content are presentation conventions. Binary entries cannot be displayed. Generated phrases use EFF Short Word List #2, version 0 (1,296 words); attribution is in `assets/EFF-WORDLIST-LICENSE.md`.

New-entry writes use nearest-ancestor recipients, multiple recipients, `PASSWORD_STORE_KEY`, restricted directory/file permissions, and atomic no-overwrite creation. Signed recipient policy verification is not implemented and causes writes to fail closed. Replacing entries, concurrent update handling, and Git staging/commit semantics remain unimplemented; generated entries are not automatically added to Git.

## Outstanding verification

Automated tests exercise name/path rejection, Unicode search, symlink omission and replacement, hardlinks, pinned-root replacement, metacharacters, multiline output, failing exit status after plaintext output, binary/oversized output, timeout, cancellation, stale results, focus transitions, and expiry. Status tests cover partial/oversized lines, ignored diagnostics and metadata, live card-wait hints, fallback without status support, stderr floods, cancellation under flooding, and clearing/isolation of per-operation status.

Still required: run GPUI on Linux; validate IME, navigation, focus and rendering cleanup; run the real `pass` fixture; test a protected key with graphical pinentry and genuinely tampered OpenPGP ciphertext. Disposable-agent startup is blocked in the current environment, and GUI/hardware runtime behavior remains unverified.

YubiKey hardware verification must cover insertion/removal, correct-PIN and cancelled-PIN flows, touch-required requests, pinentry focus transitions, stale completion, and retry after reconnect. Controlled subprocess tests do not replace these checks.
