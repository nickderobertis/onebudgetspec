"""Read the repository's existing release-version grammar for both classifier runtimes."""

import re
import sys
from pathlib import Path


def authoritative_version() -> re.Pattern[str]:
    """Load the release automation's validator without requiring an installed tools wheel."""
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools/src"))
    from repo_checks.versions import VERSION

    return VERSION


VERSION = authoritative_version()

if __name__ == "__main__":
    print(VERSION.pattern)
