#!/usr/bin/env python3
"""Package an already built native CLI with a checksum and verify its restart flow."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import tempfile
import tomllib
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    version = tomllib.loads((root/'Cargo.toml').read_text())['package']['version']
    binary = Path(args.binary).resolve()
    cpu = {'arm64':'aarch64', 'aarch64':'aarch64', 'amd64':'x86_64', 'x86_64':'x86_64'}[platform.machine().lower()]
    suffix = {'Darwin':'apple-darwin', 'Linux':'unknown-linux-gnu', 'Windows':'pc-windows-msvc'}[platform.system()]
    target = cpu+'-'+suffix
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    archive = output/f'vecnook-{version}-{target}.zip'
    if archive.exists():
        raise ValueError('binary archive already exists')
    actual = subprocess.run([binary, '--version'], check=True, capture_output=True, text=True).stdout.strip()
    if actual != 'vecnook '+version:
        raise ValueError('binary/manifest version mismatch')
    with zipfile.ZipFile(archive, 'x', compression=zipfile.ZIP_DEFLATED) as package:
        package.write(binary, binary.name)
        package.write(root/'LICENSE', 'LICENSE')
        package.writestr('README.txt', f'Vecnook {version}\nTarget: {target}\nRun vecnook demo data/demo\nDocument commands require Python 3.10+; sync and semantic queries also need local Ollama.\nhttps://github.com/gay00ung/vecnook\n')
    with tempfile.TemporaryDirectory(prefix='vecnook-binary-') as temporary:
        folder = Path(temporary)
        with zipfile.ZipFile(archive) as package:
            package.extractall(folder)
        executable = folder/binary.name
        executable.chmod(0o755)
        for command in [['--version'], ['demo', folder/'demo'], ['demo', folder/'demo', '1'], ['docs-sync', '--help']]:
            subprocess.run([str(executable), *map(str,command)], check=True, capture_output=True, text=True)
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    (output/(archive.name+'.sha256')).write_text(checksum+'  '+archive.name+'\n')
    receipt = dict(schema_version=1, version=version, target=target, archive=archive.name,
                   sha256=checksum, binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                   platform=platform.platform(), checks=['extracted_version','offline_demo','restart','embedded_client_help'], passed=True)
    (output/(archive.name+'.receipt.json')).write_text(json.dumps(receipt, indent=2)+'\n')
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
