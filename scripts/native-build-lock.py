"""Serialize native builds across processes, including maturin's post-Cargo staging."""

import errno
import os
import subprocess
import sys
import time
from pathlib import Path


def main() -> int:
    """Run a build while holding the workspace lock; nested builds inherit it."""
    if len(sys.argv) < 2:
        print(
            "native-build-lock: usage: native-build-lock.py <command> [arguments...]",
            file=sys.stderr,
        )
        return 64
    root = Path(__file__).resolve().parent.parent
    lock_path = root / "target" / "native-build.lock"
    try:
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
                    except OSError as error:
                        if error.errno not in (errno.EACCES, errno.EAGAIN, errno.EDEADLK):
                            raise
                        time.sleep(0.1)
            else:
                import fcntl

                fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
            try:
                return subprocess.call(
                    sys.argv[1:], env={**os.environ, "ONEBUDGETSPEC_NATIVE_BUILD_LOCK": "1"}
                )
            except OSError as error:
                print(
                    f"native-build-lock: cannot launch {sys.argv[1]!r}: {error}; "
                    "install the command or correct its path, then rerun the build",
                    file=sys.stderr,
                )
                return 1
    except OSError as error:
        print(
            f"native-build-lock: cannot lock {lock_path}: {error}; "
            "ensure target is writable and native-build.lock is a regular file, "
            "then rerun the build",
            file=sys.stderr,
        )
        return 1


if __name__ == "__main__":
    sys.exit(main())
