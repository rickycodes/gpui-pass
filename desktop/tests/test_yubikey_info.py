"""The metadata probe must never open or enumerate the smartcard interface."""
import contextlib
import io
from pathlib import Path
import runpy
import sys
import types
import unittest
from unittest.mock import MagicMock, patch


class HidMetadataTests(unittest.TestCase):
    def setUp(self):
        self.helper = runpy.run_path(str(Path(__file__).parents[1] / "src/security/yubikey_info.py"))
        self.connection_type = type("FidoConnection", (), {})
        self.connection = MagicMock()
        device = types.SimpleNamespace(pid=types.SimpleNamespace(get_type=lambda: "fixture"))
        info = types.SimpleNamespace(version="5.7.0", form_factor="USB-C Keychain", serial=987654321)
        self.connect = MagicMock(return_value=(self.connection, device, info))
        device_module = types.ModuleType("ykman.device")
        device_module.connect_to_device = self.connect
        device_module.get_name = lambda info, key_type: "Fixture YubiKey"
        fido_module = types.ModuleType("yubikit.core.fido")
        fido_module.FidoConnection = self.connection_type
        self.modules = {"ykman.device": device_module, "yubikit.core.fido": fido_module}

    def test_only_hid_is_requested_and_only_allowed_fields_are_printed(self):
        output = io.StringIO()
        with patch.dict(sys.modules, self.modules), contextlib.redirect_stdout(output):
            self.helper["main"]()
        self.connect.assert_called_once_with(connection_types=(self.connection_type,))
        self.connection.__exit__.assert_called_once()
        self.assertEqual(output.getvalue(),
                         "Device type: Fixture YubiKey\nFirmware version: 5.7.0\nForm factor: USB-C Keychain\n")

    def test_unavailable_hid_does_not_fall_back_to_other_interfaces(self):
        self.connect.side_effect = ValueError("No HID device")
        output = io.StringIO()
        with patch.dict(sys.modules, self.modules), contextlib.redirect_stdout(output):
            with self.assertRaises(ValueError):
                self.helper["main"]()
        self.connect.assert_called_once_with(connection_types=(self.connection_type,))
        self.assertEqual(output.getvalue(), "")

    def test_structured_firmware_is_displayed_as_a_version_number(self):
        self.connect.return_value[2].version = types.SimpleNamespace(major=5, minor=7, patch=4)
        output = io.StringIO()
        with patch.dict(sys.modules, self.modules), contextlib.redirect_stdout(output):
            self.helper["main"]()
        self.assertIn("Firmware version: 5.7.4\n", output.getvalue())
        self.assertNotIn("major=", output.getvalue())


if __name__ == "__main__":
    unittest.main()
