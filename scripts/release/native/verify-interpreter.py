"""Require the selected Python process to match the wheel target's architecture."""

import argparse
import platform


def main() -> None:
    """Report an architecture mismatch before any wheel installation."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("expected", choices=("x64", "arm64"))
    args = parser.parse_args()
    machine = platform.machine().lower()
    actual = {"x86_64": "x64", "amd64": "x64", "aarch64": "arm64", "arm64": "arm64"}.get(machine)
    if actual != args.expected:
        parser.exit(
            1,
            f"Python architecture {machine} != {args.expected}; "
            "select the managed Python for the target's architecture\n",
        )


if __name__ == "__main__":
    main()
