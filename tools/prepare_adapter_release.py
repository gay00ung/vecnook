#!/usr/bin/env python3
"""Stage version-only companion source before its unpublished core dependency exists.

This archive is review/gate input, never a substitute for Cargo's final package.
After core publication, package the actual companion and rerun the same paired
gate against that archive before uploading it. Runtime/build changes are refused.
"""
import argparse
import io
from pathlib import Path
import re
import tarfile
import tempfile
import tomllib
from check_acceleration import acceleration_fingerprint, extract
from check_distribution import digest, fingerprint


def stage(candidate_core, candidate_adapter, release_core, version, output):
    output = Path(output)
    if output.exists() or not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("new staged archive and stable companion version required")
    with tempfile.TemporaryDirectory(prefix="vecnook-adapter-stage-") as temporary:
        root = Path(temporary)
        core = extract(candidate_core, root / "core", "vecnook")
        stable = extract(release_core, root / "stable", "vecnook")
        adapter = extract(candidate_adapter, root / "adapter", "vecnook-accel")
        old_core = tomllib.loads((core / "Cargo.toml").read_text())["package"]["version"]
        new_core = tomllib.loads((stable / "Cargo.toml").read_text())["package"]["version"]
        old_adapter = tomllib.loads((adapter / "Cargo.toml").read_text())["package"]["version"]
        if (not old_core.startswith(new_core+"-rc.") or not old_adapter.startswith(version+"-rc.")
                or fingerprint(core) != fingerprint(stable)):
            raise ValueError("core runtime changed or candidate/release version pairing is invalid")
        expected = acceleration_fingerprint(adapter, old_core, digest(candidate_core))
        for name in ["Cargo.toml", "Cargo.toml.orig"]:
            path = adapter / name
            path.write_text(path.read_text().replace(old_adapter, version).replace(old_core, new_core))
        lock_path = adapter / "Cargo.lock"
        blocks = re.split(r"(?m)(?=^\[\[package\]\])", lock_path.read_text())
        for index, block in enumerate(blocks):
            if not block.startswith("[[package]]"):
                continue
            package = tomllib.loads(block)["package"][0]
            updates = ({"version": version} if package["name"] == "vecnook-accel" else
                       {"version": new_core, "checksum": digest(release_core)} if package["name"] == "vecnook" else {})
            for field, replacement in updates.items():
                block, count = re.subn(r'(?m)^'+field+r'\s*=\s*"[^"\n]*"', field+' = "'+replacement+'"', block)
                if count != 1:
                    raise ValueError("ambiguous companion lock field")
            blocks[index] = block
        lock_path.write_text("".join(blocks))
        if acceleration_fingerprint(adapter, new_core, digest(release_core)) != expected:
            raise ValueError("staged companion changes more than checked paired release identities")
        with tarfile.open(output, "x:gz") as archive:
            for path in sorted(adapter.rglob("*")):
                if not path.is_file():
                    continue
                body = path.read_bytes()
                member = tarfile.TarInfo(f"vecnook-accel-{version}/"+path.relative_to(adapter).as_posix())
                member.size, member.mode = len(body), 0o644
                archive.addfile(member, io.BytesIO(body))
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate-crate", required=True)
    parser.add_argument("--adapter-crate", required=True)
    parser.add_argument("--release-crate", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    print("Prepared review-only version-equivalent adapter: "+str(stage(args.candidate_crate, args.adapter_crate,
          args.release_crate, args.version, args.output)))


if __name__ == "__main__":
    main()
