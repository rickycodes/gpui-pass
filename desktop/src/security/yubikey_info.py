"""Optional device metadata over FIDO HID only; never enumerate PC/SC readers."""


def main():
    import logging

    # Library diagnostics can contain identifiers. Only the three fields below
    # may cross the helper's output boundary.
    logging.disable(logging.CRITICAL)
    from ykman.device import connect_to_device, get_name
    from yubikit.core.fido import FidoConnection

    connection, device, info = connect_to_device(connection_types=(FidoConnection,))
    with connection:
        version = info.version
        firmware = (f"{version.major}.{version.minor}.{version.patch}"
                    if all(hasattr(version, part) for part in ("major", "minor", "patch"))
                    else str(version))
        print("Device type:", get_name(info, device.pid.get_type()))
        print("Firmware version:", firmware)
        print("Form factor:", info.form_factor)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        # Missing library, unavailable HID access, or a CCID-only key: the app
        # still displays GnuPG/sysfs details. Never fall back to PC/SC.
        raise SystemExit(1)
