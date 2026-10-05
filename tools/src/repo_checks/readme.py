"""The example files the README shows, laid out as it describes them.

An example is a fenced block whose info string names its path,
````` ```yaml title="services/api/budgets.yaml" `````.
"""

import re
from pathlib import Path

from repo_checks.paths import ROOT

_BLOCK = re.compile(r'^```[a-z]+ title="([^"]+)"\n(.*?)^```$', re.MULTILINE | re.DOTALL)


def examples(root: Path = ROOT) -> dict[str, str]:
    """Each example's path, relative to the example tree, to its contents."""
    return {
        match.group(1): match.group(2)
        for match in _BLOCK.finditer((root / "README.md").read_text())
    }


def lay_out(files: dict[str, str], into: Path) -> None:
    """Write ``files`` under ``into``."""
    for relative, contents in files.items():
        path = into / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)
