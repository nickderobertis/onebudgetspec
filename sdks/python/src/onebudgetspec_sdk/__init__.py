"""The onebudgetspec Python SDK.

A scaffold today: it carries the release version so the package builds, installs and
releases under the same gate as the command line, and the typed API over the
``onebudgetspec`` binary lands on top of it.
"""

__all__ = ["__version__"]

#: The version of this package, which releases in lock step with the ``onebudgetspec`` binary.
__version__ = "0.1.0"
