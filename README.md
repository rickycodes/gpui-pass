# gpui-pass

A Rust/GPUI desktop client for existing [pass](https://www.passwordstore.org/) stores. Linux first, with a separate backend so other desktop platforms can be added without changing the store format.

**Status: implementation in progress.** The latest review passed 28 core tests, 23 desktop tests, 9 Python helper tests, and the disposable real `pass` integration fixture. The GPUI desktop builds and runs on Linux, and GUI and YubiKey hardware behavior have been tested on hardware in earlier work. See the [2026-11-14 review](docs/application-review-2026-11-14.md) for outstanding findings and verification limits.

The flat design uses square edges, paper tones, dark ink, restrained green, fine dividers, and generous spacing. [Design preview](docs/design-preview.svg) — an illustration with fictional data, not an application screenshot.

## Initial scope

- Browse nested `.gpg` entries in an existing store.
- Search entry and folder names without decrypting content.
- Explicit reveal: first-line password plus arbitrary remaining entry content.
- Copy the complete entry directly to the clipboard without displaying it.
- Restore the prior clipboard item after `PASSWORD_STORE_CLIP_TIME` seconds (45 by default), unless another app has changed the clipboard first.
- Hide after 30 seconds, on selection/search changes, refresh, Escape, and window deactivation.
- Cancel pending reveals and reject obsolete or background reveal/copy results.
- Generate memorable phrases from randomly selected whole words or use pass's random-character generator defaults. Name entries with folders such as `Email/example.com`.
- Remove an individual encrypted entry after a confirmation prompt; folder deletion is not implemented.
- Folder picker, explicit `--store`, and an isolated `--demo` mode.

Generated entries are encrypted by GnuPG to the recipients resolved from the nearest `.gpg-id` (or `PASSWORD_STORE_KEY`) and created without overwriting an existing entry. When recipients come from `.gpg-id` and `PASSWORD_STORE_SIGNING_KEY` is set, writes require a valid detached signature from one of its configured full fingerprints. The explicit `PASSWORD_STORE_KEY` override bypasses `.gpg-id` lookup, matching `pass`. A detected policy change cancels the write. The editor supports up to 16 KiB and 512 lines and rejects oversized input without truncating it. Saves detect changes to the encrypted entry since reveal and leave conflicting edits unsaved. A process-wide mutex serializes concurrent edits from this app process; another app process or tool such as `pass` can still race the final revision check and rename. Individual entries can be removed after confirmation; this does not erase Git history or backups. The random mode follows `pass` defaults: 25 characters from punctuation and alphanumeric characters, with `PASSWORD_STORE_GENERATED_LENGTH` and `PASSWORD_STORE_CHARACTER_SET` overrides. The character-set parser supports common POSIX classes and safe literal ASCII characters. Automatic Git commits/sync, export, and key management are deferred. Saving a generated password copies it to the clipboard automatically; the displayed generated password hides after 30 seconds, while its clipboard copy follows `PASSWORD_STORE_CLIP_TIME` and restores the previous clipboard contents. The bundled EFF Short Word List #2 is attributed in [assets/EFF-WORDLIST-LICENSE.md](assets/EFF-WORDLIST-LICENSE.md).

Copying an existing entry decrypts it in the background and copies its complete contents, including all lines; the entry never enters the revealed-entry view. The app restores the prior clipboard item after `PASSWORD_STORE_CLIP_TIME` seconds (45 by default), matching `pass`'s default timeout. If the clipboard changes before timeout, the app leaves the newer contents alone. Generated-password and editor-selection copies use the same policy. Normal app exit restores a tracked copy if it still owns the clipboard; crashes or forced termination cannot guarantee cleanup. Set the variable in the desktop launch environment to change the delay. Invalid or clock-overflowing values fall back to 45 seconds.

Manual hardware check: the user reports that hidden copy, reveal/hide, edit, and delete work with a hardware-backed store and YubiKey. See the [cleanup follow-up](docs/application-cleanup-2026-11-15.md) for the remaining GUI and hardware checks.

## Linux build and run

### Theme configuration

The app loads an optional TOML theme at `$XDG_CONFIG_HOME/gpui-pass/theme.toml`, or `~/.config/gpui-pass/theme.toml` when `XDG_CONFIG_HOME` is unset. Missing values inherit the built-in theme. Invalid or unknown values cause the file to be ignored and the built-in theme to be used. Copy [config/theme.example.toml](config/theme.example.toml) as a starting point. Colors use `#RRGGBB`; the translucent `selection` color uses `#RRGGBBAA`. Spacing and radius values are in GPUI logical pixels and are range checked. Changes apply after restarting the app.

Use a current stable Rust toolchain, GnuPG 2.2+, Wayland or X11, and Vulkan graphics support. Configure graphical pinentry through your normal `gpg-agent` setup. Running the client does not require the `pass` CLI.

The desktop is a separate Cargo package so security tests do not depend on GPUI:

```sh
# Fictional entries; no real store or GnuPG access
cargo run --manifest-path desktop/Cargo.toml -- --demo

# PASSWORD_STORE_DIR, falling back to ~/.password-store
cargo run --manifest-path desktop/Cargo.toml

# Explicit store and optional GnuPG executable
cargo run --manifest-path desktop/Cargo.toml -- \
  --store /path/to/password-store --gpg /usr/bin/gpg
```

GPUI is pinned to published version 0.2.2. Linux builds require native compiler, X11/Wayland, font, audio, and SSL development packages. A starting set on Ubuntu is:

```sh
sudo apt install build-essential clang cmake pkg-config libclang-dev \
  libasound2-dev libfontconfig1-dev libfreetype6-dev libssl-dev \
  libwayland-dev libx11-xcb-dev libxkbcommon-x11-dev libvulkan-dev
```

See [GPUI](https://gpui.rs/) and [Zed's Linux build documentation](https://zed.dev/docs/development/linux) for platform requirements. Both desktop and backend dependency lockfiles are included.

GPUI's archive dependency still uses `xattr 0.2.3`, which references the removed Linux `libc::ENOATTR` alias. The desktop uses a small [vendored compatibility patch](vendor/xattr/PATCH.md) selecting Linux/Android `ENODATA` while preserving macOS/BSD behavior. Current libc remains in use. The patch is selected automatically; do not edit Cargo's registry cache or apply the compiler's unrelated `ENOSTR` suggestion.

If linking reports `unable to find library -lxkbcommon-x11`, install `libxkbcommon-x11-dev` (Ubuntu/Debian). Having the runtime `.so.0` installed alone is insufficient for a normal build.

GnuPG defaults to `/usr/bin/gpg`. `--gpg` requires an absolute executable path. The folder picker requires a working desktop portal; use `--store` if unavailable.

| Key | Action |
| --- | --- |
| Ctrl F | Focus search and select query |
| Up / Down | Previous / next entry |
| Enter | Reveal / hide selected entry |
| Escape | Hide and cancel reveal |
| Ctrl R | Refresh store |
| Tab | Switch search/navigation focus |
| Ctrl Q | Quit |
| F11 | Toggle fullscreen |

Drag the dark title bar to move the window. Double-click it to enter or leave fullscreen. The Refresh control remains independently clickable.

## Verification

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all --check
cargo fmt --manifest-path desktop/Cargo.toml --all --check

# Requires gpg, gpg-agent, pass, Python 3; uses disposable keys/store
bash scripts/test-pass-compat.sh

# Still required with package registry access
cargo build --manifest-path desktop/Cargo.toml
cargo clippy --manifest-path desktop/Cargo.toml --all-targets -- -D warnings
```

The integration fixture sets its own `GNUPGHOME` and `PASSWORD_STORE_DIR`, creates root and nested multi-recipient entries through `pass`, checks exact multiline plaintext in Rust, and checks unchanged ciphertext. It stops its own agent and removes its own temporary directory.

For a local smoke check, build and run `--demo`, then check keyboard/mouse/IME search, navigation, resizing, Escape, focus loss, cancellation, and timeout. The real `pass` integration fixture passed outside the sandbox during the latest review. Visual verification must use fictional data.

## YubiKey / OpenPGP smartcards

YubiKey support uses the same chain as an existing GnuPG-backed `pass` setup: `gpg → gpg-agent → scdaemon → OpenPGP card`. The client inherits `GNUPGHOME` and uses the existing keyring and card key stubs. It does not move, import, export, or recreate your private key, or start a separate managed agent.

When available, the card view also shows YubiKey model, firmware, and form factor
using the `ykman` Python library through the FIDO HID interface, plus the matching
USB bus/port location from Linux sysfs. This optional query requires Python 3,
the library, and access to the key's FIDO interface. It never falls back to
PC/SC, which can compete with GnuPG's built-in CCID driver after reconnection.
Serial numbers are not displayed. Other smartcards continue to use the details
reported by GnuPG.

**Verify Device** runs locally on the card status page. Touch the YubiKey when
it blinks; the app verifies a fresh FIDO U2F challenge signature and validates
the attestation certificate chain against bundled Yubico roots. It requires Linux,
Python's `fido2` and `cryptography` packages, OpenSSL 3, and an accessible FIDO HID
interface with U2F support. FIDO2-only devices report an unsupported result.
Verification uses no PC/SC connection or network request, asks for no PIN, and
discards the temporary non-discoverable credential and attestation response.
Unplugging or cancelling discards the result. The offline trust snapshot and
its provenance are documented in [assets/YUBICO-ATTESTATION.md](assets/YUBICO-ATTESTATION.md).

PIN entry belongs to your system pinentry. The app explicitly uses `--pinentry-mode ask`; it never requests a PIN, sends one over stdin, or selects loopback entry. Your agent/scdaemon configuration and hardware touch policy stay in effect. A compact status block with a subtle background gives PIN and security-key guidance during reveal and copy. Copy status appears in the shared bottom notification bar, so you can follow the system prompt and touch a blinking YubiKey without revealing the password. The 30-second display timer begins after successful reveal decryption, not while unlocking the card.

The helper follows allowlisted `--status-fd` events for pinentry launch, card insertion, card-busy progress, agent contention, and decryption. Only fixed application messages reach the UI; raw diagnostics and key/card metadata are discarded. GnuPG does not supply a universal touch-required event, and some versions/backends do not forward card-busy progress during private-key decryption. If no actionable status arrives after two seconds, the helper gives conditional guidance: **if your YubiKey is blinking, touch its gold contact**, and complete any open system prompt first. Card-busy guidance is conditional too; a busy card does not prove a touch is required. No competing card probes or global touch-detector service are used. See [GnuPG status documentation](https://raw.githubusercontent.com/gpg/gnupg/master/doc/DETAILS).

Your current `pass` setup is the intended prerequisite. A terminal-only pinentry may need a graphical equivalent for the desktop launcher. Ignoring `gpg.conf` does **not** ignore `gpg-agent.conf` or `scdaemon.conf`. [GnuPG smartcard documentation](https://www.gnupg.org/documentation/manuals/gnupg/gpg_002dcard.html)

YubiKey hardware behavior has been tested on Linux, including card insertion/removal, reveal, PIN and touch handling, and retry after reconnect.

The app follows your existing agent/card policy; it does not force a fresh PIN or touch for every reveal. Removing a card does not currently hide an entry already revealed in memory; normal hide/focus/timeout rules apply.

Card diagnostics are written automatically to `$XDG_STATE_HOME/gpui-pass/diagnostics.log`,
or `~/.local/state/gpui-pass/diagnostics.log` when `XDG_STATE_HOME` is unset.
The startup terminal output prints the actual path. Set `GPUI_PASS_LOG` to override it.
The log records timestamps, process/probe IDs, insertion generations, subprocess
timing, output byte counts, numeric GnuPG errors, retry decisions, and pauses for
reveal/save operations. It excludes entry contents, PINs, card serials, key
fingerprints, and raw subprocess output. Each log rotates at 1 MiB, keeping one
previous file (`diagnostics.log.1`); newly created files are readable only by the
current user. Events are also printed to stderr. Logs can be inspected directly
after reproducing a problem without copying terminal output.

## Compatibility and future platforms

`src/` contains store traversal, GnuPG subprocess handling, zeroizing secret storage, and reveal lifecycle. `desktop/` contains GPUI, search input, actions, and styling.

The shared status types live in `src/operation.rs` (`OperationStatus`, `StatusKind`, `OperationProgress`); the desktop renders feedback in its bottom notification bar. Reveal, hidden copy, and save operations each receive a fresh, bounded latest-value reporter; workers publish application-owned static messages and the UI observes changes. Active prompts and app messages take priority over passive clipboard status. The generator shares the same footer. See the [cleanup follow-up](docs/application-cleanup-2026-11-15.md) for fixes and remaining review findings.

Existing stores require no migration, database, recipient changes, or new entry schema. The client uses `GNUPGHOME`, your normal keyring, and agent configuration. It deliberately ignores `gpg.conf` so configuration cannot redirect plaintext into files, and does not run extensions or accept `PASSWORD_STORE_GPG_OPTS`.

The prototype rejects symlinks, multiply linked files, traversal, backslashes, and non-UTF-8/control/bidirectional-formatting filenames. Unsupported filenames are omitted. Limits: 1 MiB decrypted text, 50,000 entries, 100,000 directory nodes, and 32 directory levels. Displayed content is capped separately.

Secure file access and process I/O currently use Unix APIs through `rustix`, with Linux-only process hardening gated separately. Non-Unix access fails closed. macOS still needs native build/pinentry, font, executable discovery, and shortcut validation. Native Windows needs reviewed reparse-point/file-handle and subprocess adapters plus GnuPG packaging. WSL is a separate option to investigate, not currently a supported target.

See [ROADMAP.md](ROADMAP.md) for planned work and [SECURITY.md](SECURITY.md) for the security boundary and limitations.
