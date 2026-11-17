Vendored blade-graphics 0.7.1 from crates.io, upstream commit
88b6c64cd7a32dd933acc4e5266808a07143ce84.

Backports GPU topology and display-server detection from
[Blade PR #315](https://github.com/kvark/blade/pull/315), merged on 2026-03-21
(commit `bed7477f3f6f35be017b7ff38fbf5966e3cc30d3`). The detection functions and
presentation policy are copied from that commit. Integration adapts the change
to 0.7.1, which lacks the later OpenXR initialization path.

The backend no longer reads `/etc/prime-discrete`. It scans PCI display devices
in sysfs once at initialization and detects Wayland/X11 from the environment.
Single-GPU systems do not trigger the PRIME workaround. When both Intel and
NVIDIA are present, the upstream policy rejects Intel on X11 and NVIDIA on
Wayland. An unknown display server or unavailable topology does not exclude GPUs.

The upstream fix is included in the graphics-0.8 and later release line.
GPUI 0.2.2 requires blade-graphics 0.7, so that newer release is outside its
compatible dependency range. Remove the vendored dependency once GPUI uses a
fixed Blade release, or if upstream publishes a compatible 0.7 patch release.

The initial, narrower fix was validated on Intel Iris Xe with
`/etc/prime-discrete` set to `on` and no NVIDIA adapter enumerated by Vulkan.
Identical 15-second demo runs with unoptimized
dependencies changed from llvmpipe to Intel, reducing average CPU from 54% to 8%
of one core and peak resident memory from 204064 KiB to 130624 KiB. These figures
include startup; the idle render counter settled to zero in both runs.

The upstream-aligned backport was then validated in the same 15-second demo:
Intel was selected, average CPU was 6% of one core (including startup), peak
resident memory was 124172 KiB, and idle app renders settled to zero. Mixed-GPU
hardware behavior was covered by policy tests rather than physical hardware.

Six regression tests cover single GPU systems, Intel with a software fallback,
mixed Intel/NVIDIA under X11 and Wayland, missing topology, unknown display
servers, and unaffected vendors. Run them through the desktop dependency graph:

```sh
cargo test --manifest-path desktop/Cargo.toml -p blade-graphics --lib prime_presentation_tests --locked
```

The upstream documentation images are omitted; source, text documentation, and
the upstream MIT license are included.
