"""The release scripts, run as the release workflow runs them.

The probe reads a real HTTP server standing in for each registry; publish.sh is driven
through every refusal it makes before contacting a registry.
"""

import json
import os
import subprocess
import sys
import threading
import zipfile
from collections.abc import Iterator
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from typing import NamedTuple
from urllib.parse import unquote

import pytest

ROOT = Path(__file__).resolve().parents[3]


class Answer(NamedTuple):
    """What the local registry answers one path with."""

    status: int
    body: object


#: Each path the local registry answers (unquoted), and how.
ANSWERS: dict[str, Answer] = {}
#: The paths the local registry was sent a package to, in order.
UPLOADS: list[str] = []


class Registry(BaseHTTPRequestHandler):
    """Answers each GET from ANSWERS (404 otherwise) and accepts every upload."""

    def do_GET(self) -> None:
        """Answer one GET from ANSWERS."""
        answer = ANSWERS.get(unquote(self.path), Answer(404, {"error": "not found"}))
        payload = (
            answer.body if isinstance(answer.body, bytes) else json.dumps(answer.body).encode()
        )
        self.send_response(answer.status)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(payload)

    def do_PUT(self) -> None:
        """Accept one upload and record where it went."""
        self.rfile.read(int(self.headers.get("Content-Length", 0)))
        UPLOADS.append(unquote(self.path))
        self.send_response(201)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(b'{"ok": true}')

    def log_message(self, format: str, *args: object) -> None:
        """Stay quiet."""


@pytest.fixture
def registry() -> Iterator[str]:
    server = HTTPServer(("127.0.0.1", 0), Registry)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    ANSWERS.clear()
    UPLOADS.clear()
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
        [sys.executable, str(ROOT / "scripts/release/release-probe.py"), identifier],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )


def test_the_probe_answers_what_each_registry_serves(registry: str) -> None:
    ANSWERS["/crates/onebudgetspec"] = Answer(200, {"crate": {"max_stable_version": "0.3.1"}})
    ANSWERS["/pypi/onebudgetspec-cli/json"] = Answer(200, {"info": {"version": "0.3.1"}})
    ANSWERS["/npm/@onebudgetspec/cli"] = Answer(200, {"dist-tags": {"latest": "0.3.1"}})
    for identifier in ("crate:onebudgetspec", "pypi:onebudgetspec-cli", "npm:@onebudgetspec/cli"):
        answered = probe(identifier, registry)
        assert (answered.returncode, answered.stdout) == (0, "0.3.1\n"), answered.stderr


def test_the_probe_answers_nothing_only_when_the_registry_says_so(registry: str) -> None:
    ANSWERS["/crates/onebudgetspec"] = Answer(200, {"crate": {"max_stable_version": None}})
    for identifier in ("crate:onebudgetspec", "pypi:onebudgetspec-sdk"):
        answered = probe(identifier, registry)
        assert (answered.returncode, answered.stdout) == (0, ""), answered.stderr


@pytest.mark.parametrize(
    ("answer", "reason"),
    [
        (Answer(500, {"error": "down"}), "answered HTTP 500"),
        (Answer(200, b"not json"), "could not be read"),
        (Answer(200, {"info": {}}), "no version where one is expected"),
        (Answer(200, {"info": {"version": "1.0.0\n"}}), "which is not a version"),
        (Answer(200, {"info": {"version": 3}}), "which is not a version"),
    ],
)
def test_a_registry_that_did_not_answer_is_not_answered(
    registry: str, answer: Answer, reason: str
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
            [sys.executable, str(ROOT / "scripts/release/release-probe.py"), *argv],
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
        ["bash", str(ROOT / "scripts/release/publish.sh"), *args],
        env={"PATH": os.environ["PATH"], "HOME": os.environ.get("HOME", "/tmp"), **env},
        capture_output=True,
        text=True,
        check=False,
    )


def pack(tmp_path: Path, name: str, version: str) -> Path:
    """A real npm tarball of an empty package called ``name`` at ``version``."""
    source = tmp_path / "source"
    source.mkdir(parents=True)
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
    assert "next: run 'scripts/release/publish.sh" in refused.stderr


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


def test_publish_refuses_an_unreadable_wheel(tmp_path: Path) -> None:
    (tmp_path / "onebudgetspec_sdk-0.1.0-py3-none-any.whl").write_text("")
    refused = publish("pypi", str(tmp_path), PYPI_TOKEN="token")
    assert refused.returncode == 1
    assert "is not a readable wheel" in refused.stderr


def npm_registry_env(base: str, tmp_path: Path) -> dict[str, str]:
    """Point npm at the local registry, with no retries and a private cache."""
    return {
        "npm_config_registry": base,
        "npm_config_cache": str(tmp_path / "npm-cache"),
        "npm_config_fetch_retries": "0",
        "NPM_TOKEN": "token",
    }


def packument(version: str, name: str = "@onebudgetspec/sdk") -> Answer:
    """The registry document of ``name`` serving ``version``."""
    manifest = {"name": name, "version": version, "dist": {"tarball": "x", "shasum": "x"}}
    return Answer(
        200, {"name": name, "dist-tags": {"latest": version}, "versions": {version: manifest}}
    )


def test_publish_skips_a_version_npm_already_serves(registry: str, tmp_path: Path) -> None:
    ANSWERS["/@onebudgetspec/sdk"] = packument("0.1.0")
    packed = pack(tmp_path, "@onebudgetspec/sdk", "0.1.0")
    done = publish("sdk-npm", str(packed), **npm_registry_env(registry, tmp_path))
    assert done.returncode == 0, done.stderr
    assert done.stdout == "publish: already published at 0.1.0, skipped: @onebudgetspec/sdk\n"
    assert UPLOADS == []


@pytest.mark.parametrize("served", [None, "0.0.9"])
def test_publish_uploads_a_version_npm_does_not_serve(
    registry: str, tmp_path: Path, served: str | None
) -> None:
    if served:
        ANSWERS["/@onebudgetspec/sdk"] = packument(served)
    packed = pack(tmp_path, "@onebudgetspec/sdk", "0.1.0")
    done = publish("sdk-npm", str(packed), **npm_registry_env(registry, tmp_path))
    assert done.returncode == 0, done.stderr
    assert UPLOADS == ["/@onebudgetspec/sdk"]


def test_publish_refuses_when_npm_cannot_say_what_it_serves(registry: str, tmp_path: Path) -> None:
    ANSWERS["/@onebudgetspec/sdk"] = Answer(500, {"error": "down"})
    packed = pack(tmp_path, "@onebudgetspec/sdk", "0.1.0")
    refused = publish("sdk-npm", str(packed), **npm_registry_env(registry, tmp_path))
    assert refused.returncode == 1
    assert "npm could not say whether @onebudgetspec/sdk@0.1.0 is published" in refused.stderr
    assert UPLOADS == []


def test_publish_refuses_a_tarball_without_a_manifest(tmp_path: Path) -> None:
    (tmp_path / "empty").mkdir()
    (tmp_path / "packed").mkdir()
    subprocess.run(
        ["tar", "-czf", str(tmp_path / "packed/broken.tgz"), "-C", str(tmp_path / "empty"), "."],
        check=True,
    )
    refused = publish("sdk-npm", str(tmp_path / "packed"), NPM_TOKEN="token")
    assert refused.returncode == 1
    assert "holds no readable package/package.json" in refused.stderr


def test_publish_reads_a_wheel_s_identity_from_its_metadata(tmp_path: Path) -> None:
    wheel = tmp_path / "onebudgetspec_cli-0.1.0-py3-none-any.whl"
    with zipfile.ZipFile(wheel, "w") as archive:
        archive.writestr(
            "onebudgetspec_sdk-0.1.0.dist-info/METADATA",
            "Metadata-Version: 2.4\nName: onebudgetspec-sdk\nVersion: 0.1.0\n",
        )
    refused = publish("pypi", str(tmp_path), PYPI_TOKEN="token")
    assert refused.returncode == 1
    assert "is onebudgetspec-sdk 0.1.0, not onebudgetspec-cli 0.1.0" in refused.stderr
    wheel.write_text("not a zip")
    refused = publish("pypi", str(tmp_path), PYPI_TOKEN="token")
    assert "is not a readable wheel" in refused.stderr


def test_publish_skips_crates_crates_io_already_serves(registry: str) -> None:
    for crate in ("onebudgetspec-core", "onebudgetspec"):
        ANSWERS[f"/api/v1/crates/{crate}/0.1.0"] = Answer(200, {"version": {"num": "0.1.0"}})
    done = publish(
        "crate", CARGO_REGISTRY_TOKEN="token", ONEBUDGETSPEC_CRATES_API=f"{registry}api/v1"
    )
    assert done.returncode == 0, done.stderr
    assert (
        done.stdout
        == "publish: already published at 0.1.0, skipped: onebudgetspec-core onebudgetspec\n"
    )


def test_publish_refuses_when_crates_io_cannot_say(registry: str) -> None:
    ANSWERS["/api/v1/crates/onebudgetspec-core/0.1.0"] = Answer(503, {"errors": []})
    refused = publish(
        "crate", CARGO_REGISTRY_TOKEN="token", ONEBUDGETSPEC_CRATES_API=f"{registry}api/v1"
    )
    assert refused.returncode == 1
    assert "crates.io answered HTTP 503 for onebudgetspec-core 0.1.0" in refused.stderr
    unreachable = publish(
        "crate", CARGO_REGISTRY_TOKEN="token", ONEBUDGETSPEC_CRATES_API="http://127.0.0.1:9/api/v1"
    )
    assert unreachable.returncode == 1
    assert "crates.io could not be reached" in unreachable.stderr


class NpmTarget(NamedTuple):
    """The directories release.yml passes publish.sh for the npm target."""

    carriers: Path
    launcher: Path


def npm_target(tmp_path: Path) -> NpmTarget:
    """Two packed carriers and the packed launcher, in the directories release.yml passes."""
    carriers = tmp_path / "carriers"
    for platform in ("linux-x64", "darwin-arm64"):
        packed = pack(tmp_path / platform, f"@onebudgetspec/cli-{platform}", "0.1.0")
        carriers.mkdir(exist_ok=True)
        for tarball in packed.iterdir():
            tarball.rename(carriers / tarball.name)
    return NpmTarget(carriers, pack(tmp_path / "launcher", "@onebudgetspec/cli", "0.1.0"))


def test_publish_uploads_the_carriers_before_the_launcher(registry: str, tmp_path: Path) -> None:
    target = npm_target(tmp_path)
    env = npm_registry_env(registry, tmp_path)
    done = publish("npm", str(target.carriers), str(target.launcher), **env)
    assert done.returncode == 0, done.stderr
    assert UPLOADS == [
        "/@onebudgetspec/cli-darwin-arm64",
        "/@onebudgetspec/cli-linux-x64",
        "/@onebudgetspec/cli",
    ]


def test_publish_resumes_a_partly_published_release(registry: str, tmp_path: Path) -> None:
    target = npm_target(tmp_path)
    for platform in ("linux-x64", "darwin-arm64"):
        name = f"@onebudgetspec/cli-{platform}"
        ANSWERS[f"/{name}"] = packument("0.1.0", name)
    done = publish(
        "npm", str(target.carriers), str(target.launcher), **npm_registry_env(registry, tmp_path)
    )
    assert done.returncode == 0, done.stderr
    assert UPLOADS == ["/@onebudgetspec/cli"]
    assert "skipped: @onebudgetspec/cli-darwin-arm64 @onebudgetspec/cli-linux-x64" in done.stdout


def wheel_of(directory: Path, file_name: str, name: str, version: str) -> Path:
    """A wheel file called ``file_name`` whose metadata declares ``name`` at ``version``."""
    directory.mkdir(parents=True, exist_ok=True)
    wheel = directory / file_name
    with zipfile.ZipFile(wheel, "w") as archive:
        archive.writestr(
            f"{name.replace('-', '_')}-{version}.dist-info/METADATA",
            f"Metadata-Version: 2.4\nName: {name}\nVersion: {version}\n",
        )
    return wheel


@pytest.mark.parametrize(
    ("target", "name", "reason"),
    [
        (
            "sdk-pypi",
            "onebudgetspec-cli",
            "is onebudgetspec-cli 0.1.0, not onebudgetspec-sdk 0.1.0",
        ),
        (
            "sdk-pypi",
            "onebudgetspec-sdk",
            "is onebudgetspec-sdk 0.2.0, not onebudgetspec-sdk 0.1.0",
        ),
        ("pypi", "onebudgetspec-cli", "is onebudgetspec-cli 0.2.0, not onebudgetspec-cli 0.1.0"),
    ],
)
def test_publish_refuses_a_wheel_of_another_target_or_version(
    tmp_path: Path, target: str, name: str, reason: str
) -> None:
    version = "0.1.0" if name == "onebudgetspec-cli" and target == "sdk-pypi" else "0.2.0"
    wheel_of(tmp_path, f"{name.replace('-', '_')}-0.1.0-py3-none-any.whl", name, version)
    refused = publish(target, str(tmp_path), PYPI_TOKEN="token")
    assert refused.returncode == 1
    assert reason in refused.stderr


def test_publish_refuses_an_unexpected_answer_from_npm(registry: str, tmp_path: Path) -> None:
    # The registry lists 0.1.0 but describes it as another version.
    manifest = {"name": "@onebudgetspec/sdk", "version": "0.1.0-other", "dist": {"tarball": "x"}}
    ANSWERS["/@onebudgetspec/sdk"] = Answer(
        200,
        {
            "name": "@onebudgetspec/sdk",
            "dist-tags": {"latest": "0.1.0"},
            "versions": {"0.1.0": manifest},
        },
    )
    packed = pack(tmp_path, "@onebudgetspec/sdk", "0.1.0")
    refused = publish("sdk-npm", str(packed), **npm_registry_env(registry, tmp_path))
    assert refused.returncode == 1
    assert "npm answered '0.1.0-other' when asked for @onebudgetspec/sdk@0.1.0" in refused.stderr
    assert UPLOADS == []


def test_an_unusable_registry_url_is_refused_with_its_fix() -> None:
    answered = subprocess.run(
        [sys.executable, str(ROOT / "scripts/release/release-probe.py"), "pypi:onebudgetspec-cli"],
        env={**os.environ, "ONEBUDGETSPEC_PROBE_PYPI_URL": "not a url/"},
        capture_output=True,
        text=True,
        check=False,
    )
    assert answered.returncode == 2
    assert "correct ONEBUDGETSPEC_PROBE_PYPI_URL" in answered.stderr


def probe_against(
    declaration: Path, identifier: str = "pypi:onebudgetspec-cli"
) -> subprocess.CompletedProcess[str]:
    """The real probe, reading ``declaration`` as its release-targets.toml."""
    return subprocess.run(
        [sys.executable, str(ROOT / "scripts/release/release-probe.py"), identifier],
        env={**os.environ, "ONEBUDGETSPEC_RELEASE_TARGETS": str(declaration)},
        capture_output=True,
        text=True,
        check=False,
    )


@pytest.mark.parametrize(
    "declaration",
    [
        'target = "not a table"\n',
        '[[target]]\nname = "pypi"\n',
        "[[target]]\nid = 7\n",
        '[[target]]\nid = "pypi onebudgetspec-cli"\n',
        "schema_version = 2\n",
    ],
)
def test_a_malformed_release_declaration_is_refused(tmp_path: Path, declaration: str) -> None:
    path = tmp_path / "release-targets.toml"
    path.write_text(declaration)
    answered = probe_against(path)
    assert answered.returncode == 1, answered.stderr
    assert answered.stdout == ""
    assert f"{path} is not a list of targets each with a <registry>:<name> id" in answered.stderr
    assert "next: restore release-targets.toml" in answered.stderr


@pytest.mark.parametrize("contents", [None, "[[target]\nid = "])
def test_an_unreadable_release_declaration_is_refused(tmp_path: Path, contents: str | None) -> None:
    path = tmp_path / "release-targets.toml"
    if contents is not None:
        path.write_text(contents)
    answered = probe_against(path)
    assert answered.returncode == 1
    assert f"{path} cannot be read" in answered.stderr
    assert "next: restore release-targets.toml" in answered.stderr


def test_a_target_on_a_registry_the_probe_does_not_read_is_refused(tmp_path: Path) -> None:
    path = tmp_path / "release-targets.toml"
    path.write_text('[[target]]\nid = "maven:onebudgetspec"\n')
    answered = probe_against(path, "maven:onebudgetspec")
    assert answered.returncode == 1
    assert "names maven:onebudgetspec on no registry this probe reads" in answered.stderr


@pytest.mark.parametrize(
    ("target", "token"),
    [("crate", "CARGO_REGISTRY_TOKEN"), ("pypi", "PYPI_TOKEN"), ("sdk-npm", "NPM_TOKEN")],
)
@pytest.mark.parametrize(
    ("declaration", "reason"),
    [
        ('target = "not a table"\n', "cannot be read as a list of release targets"),
        ("[[target]\nid = ", "cannot be read as a list of release targets"),
        (None, "cannot be read as a list of release targets"),
        ('[[target]]\nname = "other"\nid = "crate:x"\n', "names no packages for target"),
    ],
)
def test_publish_refuses_a_declaration_it_cannot_read(
    tmp_path: Path, target: str, token: str, declaration: str | None, reason: str
) -> None:
    path = tmp_path / "release-targets.toml"
    if declaration is not None:
        path.write_text(declaration)
    args = (target,) if target == "crate" else (target, str(tmp_path))
    refused = publish(*args, **{token: "token", "ONEBUDGETSPEC_RELEASE_TARGETS": str(path)})
    assert refused.returncode == 1, refused.stdout
    assert f"{path} {reason}" in refused.stderr
    assert "next: restore release-targets.toml from git" in refused.stderr
    assert "already published" not in refused.stdout


@pytest.mark.parametrize(
    ("target", "token", "declaration"),
    [
        (
            "crate",
            "CARGO_REGISTRY_TOKEN",
            '[[target]]\nname = "crate"\nid = "pypi:onebudgetspec"\n',
        ),
        ("crate", "CARGO_REGISTRY_TOKEN", '[[target]]\nname = "crate"\nid = "onebudgetspec"\n'),
        (
            "sdk-npm",
            "NPM_TOKEN",
            '[[target]]\nname = "sdk-npm"\nid = "npm:@onebudgetspec/sdk"\ncovers = ["pypi:x"]\n',
        ),
    ],
)
def test_publish_refuses_ids_outside_the_target_s_registry(
    tmp_path: Path, target: str, token: str, declaration: str
) -> None:
    path = tmp_path / "release-targets.toml"
    path.write_text(declaration)
    args = (target,) if target == "crate" else (target, str(tmp_path))
    env = {token: "token", "ONEBUDGETSPEC_RELEASE_TARGETS": str(path)}
    # Never crates.io, whatever happens.
    env["ONEBUDGETSPEC_CRATES_API"] = "http://127.0.0.1:9/api/v1"
    refused = publish(*args, **env)
    assert refused.returncode == 1, refused.stdout
    assert f"{path} cannot be read as a list of release targets" in refused.stderr
    assert "<name> id" in refused.stderr
