"""Corrupt real staged artifacts and drive the production release validation command."""

import io
import json
import os
import subprocess
import sys
import tarfile
import tomllib
import zipfile
from pathlib import Path
from typing import NamedTuple

import pytest
from repo_checks.programs import bash

ROOT = Path(__file__).resolve().parents[4]
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
# The Darwin installer uses Unix venv paths; Windows keeps its wheel/carrier journeys.
pytestmark = pytest.mark.skipif(os.name == "nt", reason="native-products.sh is the Unix installer")


class NativeProducts(NamedTuple):
    """The directory and target of the real staged release products."""

    directory: Path
    target: str


@pytest.fixture(scope="module")
def products(tmp_path_factory: pytest.TempPathFactory) -> NativeProducts:
    """Build and install the host-native products through the release's real command."""
    directory = tmp_path_factory.mktemp("native-products")
    target = subprocess.check_output(["rustc", "-vV"], text=True).split("host: ")[1].splitlines()[0]
    launcher = subprocess.check_output(
        [bash(), str(ROOT / "scripts/build-dist.sh"), "npm-launcher", str(directory / "launcher")],
        cwd=ROOT,
        text=True,
    ).strip()
    # A release runner compiles from a clean tree, so the CLI's source is newer than any
    # earlier build here: Cargo install must reuse the wheel's fresh build, not an older one.
    os.utime(ROOT / "crates/onebudgetspec/src/main.rs")
    subprocess.run(
        [
            bash(),
            str(ROOT / "scripts/release/native/native-products.sh"),
            target,
            str(directory / "products"),
            launcher,
        ],
        cwd=ROOT,
        check=True,
    )
    return NativeProducts(directory, target)


def validate(
    products: NativeProducts, wheel: Path, carrier: Path
) -> subprocess.CompletedProcess[str]:
    """Call the validator exactly as native-products.sh does."""
    directory, target = products.directory, products.target
    out = directory / "products"
    return subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts/release/native/validate-products.py"),
            str(wheel),
            str(carrier),
            str(out / "staged/onebudgetspec"),
            str(out / "revalidated.json"),
            str(out / "cargo/bin/onebudgetspec"),
            target,
            VERSION,
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )


@pytest.mark.parametrize(
    "field",
    [
        "name",
        "version",
        "architecture",
        "binary",
        "binary-duplicate",
        "metadata",
        "metadata-duplicate",
        "tag",
    ],
)
def test_wheel_metadata_and_layout_refusals(
    products: NativeProducts, tmp_path: Path, field: str
) -> None:
    """Invalid metadata and missing payloads cannot enter the release ledger."""
    original = next((products.directory / "products/wheels").glob("*.whl"))
    carrier = next((products.directory / "products/carriers").glob("*.tgz"))
    broken = tmp_path / original.name
    with zipfile.ZipFile(original) as source, zipfile.ZipFile(broken, "w") as destination:
        for member in source.infolist():
            data = source.read(member.filename)
            match field, member.filename:
                case "binary", name if name.endswith("/scripts/onebudgetspec"):
                    continue
                case "binary-duplicate", name if name.endswith("/scripts/onebudgetspec"):
                    destination.writestr("extra.data/scripts/onebudgetspec", data)
                case "metadata", name if name.endswith(".dist-info/METADATA"):
                    continue
                case "metadata-duplicate", name if name.endswith(".dist-info/METADATA"):
                    destination.writestr("extra.dist-info/METADATA", data)
                case "tag", name if name.endswith(".dist-info/WHEEL"):
                    continue
                case "name", name if name.endswith(".dist-info/METADATA"):
                    data = data.replace(b"Name: onebudgetspec-cli", b"Name: unrelated")
                case "version", name if name.endswith(".dist-info/METADATA"):
                    data = data.replace(f"Version: {VERSION}".encode(), b"Version: 999.0.0")
                case "architecture", name if name.endswith(".dist-info/WHEEL"):
                    data = b"Wheel-Version: 1.0\nTag: py3-none-macosx_11_0_unsupported\n"
            destination.writestr(member, data)
    refused = validate(products, broken, carrier)
    assert refused.returncode == 1
    assert "wheel" in refused.stderr and "rebuild" in refused.stderr


@pytest.mark.parametrize(
    "field",
    [
        "name",
        "version",
        "cpu",
        "os",
        "binary",
        "metadata",
        "payload",
        "link",
        "binary-duplicate",
        "metadata-duplicate",
    ],
)
def test_carrier_identity_layout_and_payload_refusals(
    products: NativeProducts, tmp_path: Path, field: str
) -> None:
    """The actual carrier format refuses missing, foreign or differing contents."""
    wheel = next((products.directory / "products/wheels").glob("*.whl"))
    original = next((products.directory / "products/carriers").glob("*.tgz"))
    broken = tmp_path / original.name
    with tarfile.open(original) as source, tarfile.open(broken, "w:gz") as destination:
        for member in source.getmembers():
            if not member.isfile():
                destination.addfile(member)
                continue
            extracted = source.extractfile(member)
            assert extracted is not None
            data = extracted.read()
            if member.name == "package/bin/onebudgetspec":
                match field:
                    case "binary":
                        continue
                    case "payload":
                        data += b"different bytes"
                    case "binary-duplicate":
                        destination.addfile(member, io.BytesIO(data))
                    case "link":
                        member.type = tarfile.SYMTYPE
                        member.linkname = "package/package.json"
                        destination.addfile(member)
                        continue
            if member.name == "package/package.json":
                manifest = json.loads(data)
                match field:
                    case "metadata":
                        continue
                    case "metadata-duplicate":
                        destination.addfile(member, io.BytesIO(data))
                    case "name" | "version":
                        manifest[field] = "unrelated"
                    case "cpu" | "os":
                        manifest[field] = ["unsupported"]
                data = json.dumps(manifest).encode()
            member.size = len(data)
            destination.addfile(member, io.BytesIO(data))
    refused = validate(products, wheel, broken)
    assert refused.returncode == 1
    assert "rebuild" in refused.stderr
    assert (
        "payload mismatch"
        if field == "payload"
        else "carrier"
        if field not in ("binary", "metadata")
        else "package/"
    ) in refused.stderr


def test_tag_mismatch_is_refused_before_installation(
    products: NativeProducts, tmp_path: Path
) -> None:
    """A release tag must agree with the compiled CLI's version."""
    directory, target = products.directory, products.target
    launcher = next((directory / "launcher").glob("*.tgz"))
    refused = subprocess.run(
        [
            bash(),
            str(ROOT / "scripts/release/native/native-products.sh"),
            target,
            str(tmp_path),
            str(launcher),
        ],
        cwd=ROOT,
        env={**os.environ, "TAG": "v999.0.0"},
        text=True,
        capture_output=True,
        check=False,
    )
    assert refused.returncode == 1
    assert "tag v999.0.0 disagrees" in refused.stderr and "reconcile" in refused.stderr
    assert not (tmp_path / "venv").exists()


@pytest.mark.parametrize("product", ["staged", "cargo"])
def test_staged_and_cargo_payload_corruption_is_refused(
    products: NativeProducts, tmp_path: Path, product: str
) -> None:
    """Every payload form must match the wheel's compiled CLI, not only the carrier."""
    out = products.directory / "products"
    wheel = next((out / "wheels").glob("*.whl"))
    carrier = next((out / "carriers").glob("*.tgz"))
    broken = tmp_path / "broken-cli"
    broken.write_bytes((out / "staged/onebudgetspec").read_bytes() + b"different bytes")
    staged = broken if product == "staged" else out / "staged/onebudgetspec"
    cargo = broken if product == "cargo" else out / "cargo/bin/onebudgetspec"
    refused = subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts/release/native/validate-products.py"),
            str(wheel),
            str(carrier),
            str(staged),
            str(tmp_path / "ledger.json"),
            str(cargo),
            products.target,
            VERSION,
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert refused.returncode == 1
    assert f"payload mismatch for ['{product}']" in refused.stderr


def test_real_recompilation_is_refused(products: NativeProducts, tmp_path: Path) -> None:
    """Force Cargo to rebuild the CLI and feed its actual messages to the release guard."""
    source = ROOT / "crates/onebudgetspec/src/main.rs"
    stamp = source.stat()
    try:
        # Touch only this checkout's file timestamp, restoring it below; contents never change.
        source.touch()
        completed = subprocess.run(
            [
                "cargo",
                "install",
                "--quiet",
                "--message-format",
                "json",
                "--path",
                "crates/onebudgetspec",
                "--target",
                products.target,
                "--root",
                str(tmp_path / "cargo"),
                "--locked",
                "--offline",
            ],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=True,
            env={**os.environ, "CARGO_TARGET_DIR": str(ROOT / "target")},
        )
    finally:
        os.utime(source, ns=(stamp.st_atime_ns, stamp.st_mtime_ns))
    records = [json.loads(line) for line in completed.stdout.splitlines() if line.startswith("{")]
    assert any(
        record.get("reason") == "compiler-artifact" and record.get("fresh") is False
        for record in records
    )
    messages = tmp_path / "cargo-messages.jsonl"
    messages.write_text(completed.stdout)
    refused = subprocess.run(
        [sys.executable, str(ROOT / "scripts/release/native/verify-fresh.py"), str(messages)],
        text=True,
        capture_output=True,
        check=False,
    )
    assert refused.returncode == 1
    assert (
        "Cargo install recompiled" in refused.stderr and "shared target directory" in refused.stderr
    )


@pytest.mark.parametrize("failure", ["version", "status", "report"])
def test_installed_product_verification_refuses_real_cli_disagreements(
    products: NativeProducts, tmp_path: Path, failure: str
) -> None:
    """Drive real installed CLIs over changed expectations and SDK-reporting workloads."""
    out = products.directory / "products"
    case = tmp_path / "case"
    case.mkdir()
    document = json.loads((out / "case/budgets.yaml").read_text())
    expected = f"onebudgetspec {VERSION}"
    match failure:
        case "version":
            expected += "-wrong"
        case "status":
            document["budgets"][0]["threshold"] = 3
        case "report":
            command = document["budgets"][0]["command"]
            command[-1] = command[-1].replace("report(2)", "report(3)")
    (case / "budgets.yaml").write_text(json.dumps(document))
    refused = subprocess.run(
        [
            bash(),
            str(ROOT / "scripts/release/native/verify-installed.sh"),
            expected,
            str(case),
            str(tmp_path / "report.json"),
            str(out / "venv/bin/onebudgetspec"),
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert refused.returncode != 0
    reason = {"version": "reports", "status": "check exited 0", "report": "staged check disagrees"}
    assert reason[failure] in refused.stderr
    assert "rebuild" in refused.stderr


def test_node_architecture_mismatch_is_refused(products: NativeProducts, tmp_path: Path) -> None:
    """The installer requires the real Node architecture used by the launcher."""
    target = (
        products.target.replace("x86_64", "aarch64")
        if products.target.startswith("x86_64")
        else products.target.replace("aarch64", "x86_64")
    )
    launcher = next((products.directory / "launcher").glob("*.tgz"))
    refused = subprocess.run(
        [
            bash(),
            str(ROOT / "scripts/release/native/native-products.sh"),
            target,
            str(tmp_path),
            str(launcher),
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert refused.returncode == 1
    assert (
        "Node architecture" in refused.stderr
        and "select the Node build for the target's architecture" in refused.stderr
    )
    assert not (tmp_path / "wheels").exists()


@pytest.mark.parametrize(
    "messages", ["", "not JSON\n", '{"reason":', '{"reason":"compiler-artifact"}\n']
)
def test_missing_or_malformed_cargo_evidence_is_refused(tmp_path: Path, messages: str) -> None:
    """The freshness guard refuses incomplete or corrupt evidence at its file boundary."""
    path = tmp_path / "messages.jsonl"
    path.write_text(messages)
    refused = subprocess.run(
        [sys.executable, str(ROOT / "scripts/release/native/verify-fresh.py"), str(path)],
        text=True,
        capture_output=True,
        check=False,
    )
    assert refused.returncode == 1
    assert "shared target directory" in refused.stderr
    if messages in ("", "not JSON\n"):
        assert "no compiler-artifact evidence" in refused.stderr


def test_real_python_architecture_mismatch_is_refused() -> None:
    """Run the interpreter guard in this real Python process with the opposite target."""
    import platform

    expected = "arm64" if platform.machine().lower() in ("x86_64", "amd64") else "x64"
    refused = subprocess.run(
        [sys.executable, str(ROOT / "scripts/release/native/verify-interpreter.py"), expected],
        text=True,
        capture_output=True,
        check=False,
    )
    assert refused.returncode == 1
    assert (
        "Python architecture" in refused.stderr
        and "select the managed Python for the target's architecture" in refused.stderr
    )


@pytest.mark.parametrize("failure", ["wrong-cli", "unfinished", "unsuccessful"])
def test_cargo_reuse_requires_complete_evidence_for_the_installed_cli(
    products: NativeProducts, tmp_path: Path, failure: str
) -> None:
    """Corrupt real Cargo evidence while leaving unrelated fresh artifacts intact."""
    records = [
        json.loads(line)
        for line in (products.directory / "products/cargo-build.jsonl").read_text().splitlines()
        if line.startswith("{")
    ]
    match failure:
        case "wrong-cli":
            for record in records:
                if record.get("target", {}).get("name") == "onebudgetspec":
                    record["target"]["name"] = "unrelated"
        case "unfinished":
            records = [record for record in records if record.get("reason") != "build-finished"]
        case "unsuccessful":
            for record in records:
                if record.get("reason") == "build-finished":
                    record["success"] = False
    path = tmp_path / "cargo.jsonl"
    path.write_text("\n".join(json.dumps(record) for record in records))
    refused = subprocess.run(
        [sys.executable, str(ROOT / "scripts/release/native/verify-fresh.py"), str(path)],
        text=True,
        capture_output=True,
        check=False,
    )
    assert refused.returncode == 1
    assert "incomplete successful onebudgetspec build evidence" in refused.stderr


@pytest.mark.parametrize("target", ["unsupported-target", "x86_64-pc-windows-msvc"])
def test_product_validator_rejects_undeclared_or_non_unix_targets(
    products: NativeProducts, tmp_path: Path, target: str
) -> None:
    """Drive the validator boundary itself rather than the installer's earlier target guard."""
    out = products.directory / "products"
    refused = subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts/release/native/validate-products.py"),
            str(next((out / "wheels").glob("*.whl"))),
            str(next((out / "carriers").glob("*.tgz"))),
            str(out / "staged/onebudgetspec"),
            str(tmp_path / "hashes.json"),
            str(out / "cargo/bin/onebudgetspec"),
            target,
            VERSION,
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    assert refused.returncode == 1
    assert "unsupported native installer target" in refused.stderr and "rebuild" in refused.stderr


@pytest.mark.parametrize(
    "shape",
    [
        None,
        [],
        {},
        {"results": None},
        {"results": []},
        {"results": [None]},
        {"results": [{"actual": "2", "verdict": "over"}]},
    ],
)
def test_report_validation_refuses_malformed_cli_data(tmp_path: Path, shape: object) -> None:
    """Drive the production report-verification command at its serialized-file boundary."""
    path = tmp_path / "report.json"
    path.write_text(json.dumps(shape))
    refused = subprocess.run(
        [bash(), str(ROOT / "scripts/release/native/verify-installed.sh"), "--report", str(path)],
        text=True,
        capture_output=True,
        check=False,
    )
    assert refused.returncode == 1
    assert "invalid staged report" in refused.stderr and "rebuild" in refused.stderr
    assert "TypeError" not in refused.stderr
