"""Serialize native builds across processes, including maturin's post-Cargo staging."""

import os
import subprocess
import sys
import time
from pathlib import Path


def main() -> int:
    """Run a build while holding the workspace lock; nested builds inherit it."""
    root = Path(__file__).resolve().parent.parent
    lock_path = root / "target" / "native-build.lock"
    lock_path.parent.mkdir(exist_ok=True)
    with lock_path.open("a+b") as lock:
        if os.name == "nt":
            import msvcrt

            lock.write(b"\0")
            lock.flush()
            while True:
                lock.seek(0)
                try:
                    msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
                    break
                except OSError:
                    time.sleep(0.1)
        else:
            import fcntl

            fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        return subprocess.call(
            sys.argv[1:], env={**os.environ, "ONEBUDGETSPEC_NATIVE_BUILD_LOCK": "1"}
        )


if __name__ == "__main__":
    sys.exit(main())
