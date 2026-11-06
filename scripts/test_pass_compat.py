"""Verify Rust against disposable keys and a store created by the real pass CLI."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    with tempfile.TemporaryDirectory(prefix="gpui-pass-compat-") as directory:
        home = Path(directory) / "gnupg"
        home.mkdir(mode=0o700)
        environment = os.environ.copy()
        for name in ("PASSWORD_STORE_KEY", "PASSWORD_STORE_GPG_OPTS",
                     "PASSWORD_STORE_SIGNING_KEY", "PASSWORD_STORE_EXTENSIONS_DIR"):
            environment.pop(name, None)
        environment.update(
            GNUPGHOME=str(home), PASSWORD_STORE_DIR=str(Path(directory) / "store"),
            GPUI_PASS_TEST_STORE=str(Path(directory) / "store"),
            PASSWORD_STORE_ENABLE_EXTENSIONS="false",
        )

        def run(command, data=None):
            result = subprocess.run(command, input=data, env=environment,
                                    capture_output=True, timeout=60)
            if result.returncode:
                # Diagnostics here refer only to the disposable fixture.
                sys.stderr.buffer.write(result.stderr)
                raise RuntimeError(f"Fixture command failed: {command[0]}")

        try:
            for name in ("Root <root@example.test>", "Work <work@example.test>"):
                run(["gpg", "--batch", "--pinentry-mode", "loopback", "--passphrase", "",
                     "--quick-generate-key", f"GPUI Pass {name}", "default", "default", "1d"])
            run(["pass", "init", "root@example.test"])
            run(["pass", "init", "-p", "work", "root@example.test", "work@example.test"])
            run(["pass", "insert", "-m", "personal/email"],
                b"fixture-root-secret\nusername: person@example.test\n\nPreserve multiline notes.\n")
            run(["pass", "insert", "-m", "work/service"],
                b"fixture-nested-secret\nusername: work@example.test\n")
            return subprocess.run(
                ["cargo", "test", "--locked", "--test", "pass_compat", "--", "--ignored"],
                env=environment, timeout=60,
            ).returncode
        finally:
            subprocess.run(["gpgconf", "--homedir", str(home), "--kill", "gpg-agent"],
                           capture_output=True, timeout=10)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (RuntimeError, subprocess.TimeoutExpired) as error:
        sys.exit(str(error))
