"""Local FIDO U2F attestation. Only a fixed result token leaves this process."""
import hashlib
import logging
import os
import subprocess
import sys
import time

logging.disable(logging.CRITICAL)


class VerificationFailure(Exception):
    pass


def verify_chain(certificate, roots, intermediates):
    # Use OpenSSL's full path validation (validity, signatures, CA constraints),
    # with only the bundled Yubico roots trusted. No system CAs or disk files.
    from cryptography import x509
    from cryptography.hazmat.primitives.serialization import Encoding

    leaf = x509.load_der_x509_certificate(certificate).public_bytes(Encoding.PEM)
    fds = []
    try:
        for name, data in [("yubico-roots", roots), ("yubico-intermediates", intermediates)]:
            fd = os.memfd_create(name, os.MFD_CLOEXEC)
            fds.append(fd)
            with os.fdopen(os.dup(fd), "wb") as stream:
                stream.write(data)
            os.lseek(fd, 0, os.SEEK_SET)
        result = subprocess.run([
            "openssl", "verify", "-no-CAfile", "-no-CApath", "-no-CAstore",
            "-trusted", f"/proc/self/fd/{fds[0]}",
            "-untrusted", f"/proc/self/fd/{fds[1]}",
        ], input=leaf, capture_output=True, pass_fds=fds, timeout=5)
        if result.returncode:
            raise VerificationFailure()
    finally:
        for fd in fds:
            os.close(fd)


def verify_registration(registration, application, challenge, roots, intermediates):
    # Certificate trust alone is insufficient: prove possession of the
    # attestation key by verifying its signature over this fresh challenge.
    registration.verify(application, challenge)
    verify_chain(registration.certificate, roots, intermediates)


def run(product_id, roots, intermediates):
    from fido2.hid import CtapHidDevice, list_descriptors, open_connection, CAPABILITY
    from fido2.ctap1 import Ctap1, ApduError, APDU

    candidates = [d for d in list_descriptors() if d.vid == 0x1050 and d.pid == product_id]
    if not candidates:
        return "unavailable"
    if len(candidates) != 1:
        return "multiple"
    descriptor = candidates[0]
    with CtapHidDevice(descriptor, open_connection(descriptor)) as device:
        if device.capabilities & CAPABILITY.NMSG:
            return "unsupported"
        protocol = Ctap1(device)
        application = hashlib.sha256(b"gpui-pass:local-device-verification:v1").digest()
        challenge = os.urandom(32)
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            try:
                registration = protocol.register(challenge, application)
                break
            except ApduError as error:
                if error.code != APDU.USE_NOT_SATISFIED:
                    return "unsupported"
                time.sleep(0.2)
        else:
            return "timeout"
        try:
            verify_registration(registration, application, challenge, roots, intermediates)
        except (FileNotFoundError, AttributeError, ImportError):
            return "unavailable"
        except subprocess.TimeoutExpired:
            return "failed"
        except (VerificationFailure, ValueError):
            return "untrusted"
        except Exception:
            # A malformed signature or unsupported validation format must not
            # become success or cause raw certificate data to be printed.
            return "untrusted"
        return "verified"


def main():
    try:
        payload = sys.stdin.buffer.read(65537)
        if len(payload) > 65536:
            raise ValueError()
        roots, intermediates = payload.split(b"\n---INTERMEDIATES---\n", 1)
        result = run(int(sys.argv[1]), roots, intermediates)
    except (ImportError, FileNotFoundError, PermissionError, AttributeError):
        result = "unavailable"
    except Exception:
        result = "failed"
    print(result)


if __name__ == "__main__":
    main()
