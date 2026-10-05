"""The release scripts, run as the release workflow runs them.

The probe reads a real HTTP server standing in for each registry; publish.sh is driven
through every refusal it makes before contacting a registry.
"""

import json
import os
import subprocess
import sys
import threading
from collections.abc import Iterator
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

import pytest

from repo_checks.paths import ROOT

#: Each path the local registry answers, and how: a status and a JSON body.
ANSWERS: dict[str, tuple[int, object]] = {}


class Registry(BaseHTTPRequestHandler):
    """Answers each request from ANSWERS, and 404 for anything else."""

    def do_GET(self) -> None:
        """Answer one GET from ANSWERS."""
        status, body = ANSWERS.get(self.path, (404, {"errors": "not found"}))
        payload = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(status)
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, format: str, *args: object) -> None:
        """Stay quiet."""


@pytest.fixture
def registry() -> Iterator[str]:
    server = HTTPServer(("127.0.0.1", 0), Registry)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    ANSWERS.clear()
    yield f"http://127.0.0.1:{server.server_port}/"
    server.shutdown()


def probe(identifier: str, base: str) -> subprocess.CompletedProcess[str]:
    env = {
        **os.environ,
        "ONEBUDGETSPEC_PROBE_CRATE_URL": f"{base}crates/",
        "ONEBUDGETSPEC_PROBE_PYPI_URL": f"{base}pypi/",
        "ONEBUDGETSPEC_PROBE_NPM_URL": f"{base}npm/",
    }
    return subprocess.run(
        [sys.executable, str(ROOT / "scripts/release-probe.py"), identifier],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )


def test_the_probe_answers_what_each_registry_serves(registry: str) -> None:
    ANSWERS["/crates/onebudgetspec"] = (200, {"crate": {"max_stable_version": "0.3.1"}})
    ANSWERS["/pypi/onebudgetspec-cli/json"] = (200, {"info": {"version": "0.3.1"}})
    ANSWERS["/npm/@onebudgetspec%2Fcli"] = (200, {"dist-tags": {"latest": "0.3.1"}})
    for identifier in ("crate:onebudgetspec", "pypi:onebudgetspec-cli", "npm:@onebudgetspec/cli"):
        answered = probe(identifier, registry)
        assert (answered.returncode, answered.stdout) == (0, "0.3.1\n"), answered.stderr


def test_the_probe_answers_nothing_only_when_the_registry_says_so(registry: str) -> None:
    ANSWERS["/crates/onebudgetspec"] = (200, {"crate": {"max_stable_version": None}})
    for identifier in ("crate:onebudgetspec", "pypi:onebudgetspec-sdk"):
        answered = probe(identifier, registry)
        assert (answered.returncode, answered.stdout) == (0, ""), answered.stderr


@pytest.mark.parametrize(
    ("answer", "reason"),
    [
        ((500, {"error": "down"}), "answered HTTP 500"),
        ((200, b"not json"), "could not be read"),
        ((200, {"info": {}}), "no version where one is expected"),
        ((200, {"info": {"version": "1.0\nevil"}}), "which is not a version"),
        ((200, {"info": {"version": 3}}), "which is not a version"),
    ],
)
def test_a_registry_that_did_not_answer_is_not_answered(
    registry: str, answer: tuple[int, object], reason: str
) -> None:
    ANSWERS["/pypi/onebudgetspec-cli/json"] = answer
    answered = probe("pypi:onebudgetspec-cli", registry)
    assert answered.returncode == 1
    assert answered.stdout == ""
    assert reason in answered.stderr
    assert "next: re-ask later" in answered.stderr


def test_the_probe_refuses_what_is_not_a_target() -> None:
    for argv in ([], ["crate:onebudgetspec-core"], ["a", "b"]):
        answered = subprocess.run(
            [sys.executable, str(ROOT / "scripts/release-probe.py"), *argv],
            capture_output=True,
            text=True,
            check=False,
        )
        assert answered.returncode == 2
        assert "next:" in answered.stderr


def test_an_unreachable_registry_is_not_answered() -> None:
    answered = probe("crate:onebudgetspec", "http://127.0.0.1:9/")
    assert answered.returncode == 1
    assert "could not be read" in answered.stderr


def publish(*args: str, **env: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", str(ROOT / "scripts/publish.sh"), *args],
        env={"PATH": os.environ["PATH"], "HOME": os.environ.get("HOME", "/tmp"), **env},
        capture_output=True,
        text=True,
        check=False,
    )


def pack(tmp_path: Path, name: str, version: str) -> Path:
    """A real npm tarball of an empty package called ``name`` at ``version``."""
    source = tmp_path / "source"
    source.mkdir()
    (source / "package.json").write_text(json.dumps({"name": name, "version": version}))
    out = tmp_path / "packed"
    out.mkdir()
    subprocess.run(
        ["npm", "pack", str(source), "--silent", "--pack-destination", str(out)],
        check=True,
        capture_output=True,
    )
    return out


@pytest.mark.parametrize(
    ("args", "reason"),
    [
        ((), "unknown target"),
        (("crate", "extra"), "crate takes no directory"),
        (("pypi",), "takes the directory holding its wheels"),
        (("npm",), "takes the directories holding its packed packages"),
    ],
)
def test_publish_refuses_a_malformed_call(args: tuple[str, ...], reason: str) -> None:
    refused = publish(*args)
    assert refused.returncode == 1
    assert reason in refused.stderr
    assert "next: run 'scripts/publish.sh" in refused.stderr


@pytest.mark.parametrize(
    ("target", "token"), [("crate", "CARGO_REGISTRY_TOKEN"), ("sdk-npm", "NPM_TOKEN")]
)
def test_publish_refuses_without_its_token(tmp_path: Path, target: str, token: str) -> None:
    args = (target,) if target == "crate" else (target, str(tmp_path))
    refused = publish(*args)
    assert refused.returncode == 1
    assert f"{token} is not set" in refused.stderr
    assert f"provision the {token} repository secret" in refused.stderr


def test_publish_refuses_a_package_its_target_does_not_publish(tmp_path: Path) -> None:
    packed = pack(tmp_path, "@onebudgetspec/cli", "0.1.0")
    refused = publish("sdk-npm", str(packed), NPM_TOKEN="token")
    assert refused.returncode == 1
    assert "@onebudgetspec/cli, which target sdk-npm does not publish" in refused.stderr


def test_publish_refuses_a_package_at_another_version(tmp_path: Path) -> None:
    packed = pack(tmp_path, "@onebudgetspec/sdk", "9.9.9")
    refused = publish("sdk-npm", str(packed), NPM_TOKEN="token")
    assert refused.returncode == 1
    assert "@onebudgetspec/sdk@9.9.9, not the workspace's 0.1.0" in refused.stderr


def test_publish_refuses_a_wheel_of_another_package(tmp_path: Path) -> None:
    (tmp_path / "onebudgetspec_sdk-0.1.0-py3-none-any.whl").write_text("")
    refused = publish("pypi", str(tmp_path), PYPI_TOKEN="token")
    assert refused.returncode == 1
    assert "is not a onebudgetspec_cli 0.1.0 wheel" in refused.stderr
