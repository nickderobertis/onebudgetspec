"""Validate staged Unix release products without executing non-native binaries."""

import argparse
import email
import hashlib
import json
import tarfile
import tomllib
import zipfile
from pathlib import Path
from typing import TypedDict


class ProductLedger(TypedDict):
    """The identity and payload hashes of one staged target's products."""

    target: str
    version: str
    wheel_filename: str
    wheel: str
    npm: str
    staged: str
    cargo: str


def wheel_member(archive: zipfile.ZipFile, suffix: str) -> bytes:
    """Read one required wheel member, refusing ambiguous or missing layouts."""
    members = [name for name in archive.namelist() if name.endswith(suffix)]
    if len(members) != 1:
        raise ValueError(f"wheel needs exactly one {suffix}; rebuild its binary wheel")
    return archive.read(members[0])


def carrier_member(archive: tarfile.TarFile, name: str) -> bytes:
    """Read a required regular carrier member."""
    members = [member for member in archive.getmembers() if member.name == name]
    if len(members) != 1:
        raise ValueError(f"carrier needs exactly one {name}; rebuild its npm carrier")
    info = members[0]
    if not info.isfile():
        raise ValueError(f"carrier {name} is not a regular file; rebuild its npm carrier")
    member = archive.extractfile(info)
    if member is None:
        raise ValueError(f"carrier lacks {name}; rebuild its npm carrier")
    return member.read()


def validate(
    wheel: Path, carrier: Path, staged: Path, cargo: Path, target: str, version: str
) -> ProductLedger:
    """Check target identity and prove each packaging form reused the same CLI bytes."""
    declared = tomllib.loads(
        (Path(__file__).resolve().parents[3] / "rust-toolchain.toml").read_text()
    )["toolchain"]["targets"]
    if target not in declared or not target.endswith(("apple-darwin", "unknown-linux-gnu")):
        raise ValueError(f"unsupported native installer target {target}; use build-dist.sh targets")
    system = "darwin" if target.endswith("apple-darwin") else "linux"
    cpu = "x64" if target.startswith("x86_64") else "arm64"
    architecture = "x86_64" if cpu == "x64" else "arm64" if system == "darwin" else "aarch64"
    with zipfile.ZipFile(wheel) as archive:
        wheel_bytes = wheel_member(archive, "/scripts/onebudgetspec")
        metadata = email.message_from_bytes(wheel_member(archive, ".dist-info/METADATA"))
        tags = wheel_member(archive, ".dist-info/WHEEL").decode()
        if metadata["Name"] != "onebudgetspec-cli" or metadata["Version"] != version:
            raise ValueError(
                f"wheel identity {metadata['Name']}/{metadata['Version']} != CLI {version}; "
                "reconcile workspace versions and rebuild the wheel"
            )
        platforms = ("macosx_",) if system == "darwin" else ("manylinux_", "linux_")
        if not any(
            line.startswith("Tag: ")
            and line.endswith("_" + architecture)
            and any(platform in line for platform in platforms)
            for line in tags.splitlines()
        ):
            raise ValueError(f"wheel tags {tags!r} != {target}; rebuild for that target")
    with tarfile.open(carrier) as archive:
        carrier_bytes = carrier_member(archive, "package/bin/onebudgetspec")
        manifest = json.loads(carrier_member(archive, "package/package.json"))
        if not isinstance(manifest, dict) or (
            manifest.get("name") != f"@onebudgetspec/cli-{system}-{cpu}"
            or manifest.get("version") != version
            or manifest.get("cpu") != [cpu]
            or manifest.get("os") != [system]
        ):
            raise ValueError(f"carrier identity {manifest} != {target}/{version}; rebuild carrier")
    payloads = {
        "wheel": wheel_bytes,
        "npm": carrier_bytes,
        "staged": staged.read_bytes(),
        "cargo": cargo.read_bytes(),
    }
    differences = [name for name, payload in payloads.items() if payload != wheel_bytes]
    if differences:
        raise ValueError(f"payload mismatch for {differences}; rebuild from one target binary")
    hashes = {name: hashlib.sha256(payload).hexdigest() for name, payload in payloads.items()}
    return ProductLedger(
        target=target,
        version=version,
        wheel_filename=wheel.name,
        wheel=hashes["wheel"],
        npm=hashes["npm"],
        staged=hashes["staged"],
        cargo=hashes["cargo"],
    )


def main() -> None:
    """Write the validated payload ledger, or name a refusal and a recovery step."""
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("wheel", "carrier", "staged", "hashes", "cargo"):
        parser.add_argument(name, type=Path)
    parser.add_argument("target")
    parser.add_argument("version")
    args = parser.parse_args()
    try:
        result = validate(
            args.wheel, args.carrier, args.staged, args.cargo, args.target, args.version
        )
        args.hashes.write_text(json.dumps(result))
    except (ValueError, KeyError, OSError, zipfile.BadZipFile, tarfile.TarError) as error:
        parser.exit(
            1, f"validate-products: {error}; rebuild products and rerun native-products.sh\n"
        )


if __name__ == "__main__":
    main()
