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
    """Write ``files`` under ``into``.

    Raises:
        ValueError: an example's path is absolute or climbs out of ``into``.
    """
    for relative, contents in files.items():
        if Path(relative).is_absolute() or ".." in Path(relative).parts:
            raise ValueError(f"the README example path {relative!r} leaves the example tree")
        path = into / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)
