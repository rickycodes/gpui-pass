"""Real signature and X.509 path tests using disposable in-memory identities."""
import datetime
from pathlib import Path
import runpy
import types
import unittest
from unittest.mock import patch

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import NameOID
from fido2.ctap1 import RegistrationData
from fido2.attestation import InvalidSignature


HELPER = runpy.run_path(str(Path(__file__).parents[1] / "src/security/verify_device.py"))


def certificate(name, key, issuer, issuer_key, ca, expired=False):
    now = datetime.datetime.utcnow()
    subject = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, name)])
    return (x509.CertificateBuilder().subject_name(subject).issuer_name(issuer or subject)
            .public_key(key.public_key()).serial_number(x509.random_serial_number())
            .not_valid_before(now - datetime.timedelta(days=2))
            .not_valid_after(now + datetime.timedelta(days=-1 if expired else 1))
            .add_extension(x509.BasicConstraints(ca=ca, path_length=None), critical=True)
            .sign(issuer_key, hashes.SHA256()))


def pem(cert):
    return cert.public_bytes(serialization.Encoding.PEM)


class AttestationTests(unittest.TestCase):
    def setUp(self):
        self.root_key = ec.generate_private_key(ec.SECP256R1())
        self.root = certificate("Fixture root", self.root_key, None, self.root_key, True)
        self.intermediate_key = ec.generate_private_key(ec.SECP256R1())
        self.intermediate = certificate("Fixture intermediate", self.intermediate_key,
                                        self.root.subject, self.root_key, True)
        self.attestation_key = ec.generate_private_key(ec.SECP256R1())
        self.leaf = certificate("Fixture attestation", self.attestation_key,
                                self.intermediate.subject, self.intermediate_key, False)
        self.application = b"a" * 32
        self.challenge = b"b" * 32

    def registration(self, cert=None):
        cert = cert or self.leaf
        credential = ec.generate_private_key(ec.SECP256R1()).public_key().public_bytes(
            serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint)
        handle = b"fixture-key-handle"
        signed = b"\0" + self.application + self.challenge + handle + credential
        signature = self.attestation_key.sign(signed, ec.ECDSA(hashes.SHA256()))
        return RegistrationData(b"\x05" + credential + bytes([len(handle)]) + handle
                                + cert.public_bytes(serialization.Encoding.DER) + signature)

    def verify(self, registration, challenge=None, roots=None):
        HELPER["verify_registration"](registration, self.application, challenge or self.challenge,
                                      roots or pem(self.root), pem(self.intermediate))

    def test_valid_signature_and_trusted_intermediate_chain_pass(self):
        self.verify(self.registration())

    def test_replayed_signature_with_a_different_challenge_fails(self):
        with self.assertRaises(InvalidSignature):
            self.verify(self.registration(), challenge=b"c" * 32)

    def test_self_signed_yubico_name_is_not_a_trusted_manufacturer(self):
        forged = certificate("Yubico U2F Root CA Serial 457200631", self.attestation_key,
                             None, self.attestation_key, True)
        with self.assertRaises(HELPER["VerificationFailure"]):
            self.verify(self.registration(forged))

    def test_expired_attestation_is_rejected(self):
        expired = certificate("Expired", self.attestation_key,
                              self.intermediate.subject, self.intermediate_key, False, expired=True)
        with self.assertRaises(HELPER["VerificationFailure"]):
            self.verify(self.registration(expired))

    def test_non_ca_intermediate_is_rejected(self):
        self.intermediate = certificate("Not a CA", self.intermediate_key,
                                        self.root.subject, self.root_key, False)
        self.leaf = certificate("Fixture leaf", self.attestation_key,
                                self.intermediate.subject, self.intermediate_key, False)
        with self.assertRaises(HELPER["VerificationFailure"]):
            self.verify(self.registration())

    def test_multiple_matching_keys_are_rejected_before_opening_hid(self):
        descriptor = types.SimpleNamespace(vid=0x1050, pid=0x0406)
        with patch("fido2.hid.list_descriptors", return_value=[descriptor, descriptor]), \
                patch("fido2.hid.open_connection") as opened:
            self.assertEqual(HELPER["run"](0x0406, b"roots", b"intermediates"), "multiple")
            opened.assert_not_called()

    def test_an_unrelated_hid_device_is_never_verified(self):
        descriptor = types.SimpleNamespace(vid=0x1234, pid=0x0406)
        with patch("fido2.hid.list_descriptors", return_value=[descriptor]), \
                patch("fido2.hid.open_connection") as opened:
            self.assertEqual(HELPER["run"](0x0406, b"roots", b"intermediates"), "unavailable")
            opened.assert_not_called()


if __name__ == "__main__":
    unittest.main()
