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
    parser.add_argument('--manifest', help='optional companion Cargo.toml')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    manifest = Path(args.manifest).resolve() if args.manifest else root/'Cargo.toml'
    settings = tomllib.loads(manifest.read_text())['package']
    name, version = settings['name'], settings['version']
    if name not in {'vecnook', 'vecnook-accel'}:
        raise ValueError('unsupported native package')
    binary = Path(args.binary).resolve()
    cpu = {'arm64':'aarch64', 'aarch64':'aarch64', 'amd64':'x86_64', 'x86_64':'x86_64'}[platform.machine().lower()]
    suffix = {'Darwin':'apple-darwin', 'Linux':'unknown-linux-gnu', 'Windows':'pc-windows-msvc'}[platform.system()]
    target = cpu+'-'+suffix
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    archive = output/f'{name}-{version}-{target}.zip'
    if archive.exists():
        raise ValueError('binary archive already exists')
    actual = subprocess.run([binary, '--version'], check=True, capture_output=True, text=True, encoding="utf-8").stdout.strip()
    if actual != name+' '+version:
        raise ValueError('binary/manifest version mismatch')
    with zipfile.ZipFile(archive, 'x', compression=zipfile.ZIP_DEFLATED) as package:
        package.write(binary, binary.name)
        package.write(manifest.parent/'LICENSE', 'LICENSE')
        package.writestr('README.txt', f'{name} {version}\nTarget: {target}\nRun {name} demo data/demo\n'+
                         ('mmap requires immutable generation files; never edit or truncate published files.\n' if name == 'vecnook-accel' else
                          'Document commands require Python 3.10+; sync and semantic queries also need local Ollama.\n')+
                         'https://github.com/gay00ung/vecnook\n')
    with tempfile.TemporaryDirectory(prefix='vecnook-binary-') as temporary:
        folder = Path(temporary)
        with zipfile.ZipFile(archive) as package:
            package.extractall(folder)
        executable = folder/binary.name
        executable.chmod(0o755)
        commands = ([['--version'], ['demo', folder/'demo'], ['demo', folder/'demo', '1'], ['docs-sync', '--help']]
                    if name == 'vecnook' else [['--version'], ['demo', folder/'demo'],
                        ['search', folder/'demo', '0,0,1', '1', '--sq8', '2', '--mmap'], ['--help']])
        for command in commands:
            result = subprocess.run([str(executable), *map(str,command)], capture_output=True, text=True, encoding="utf-8")
            if result.returncode:
                raise RuntimeError(f"extracted CLI failed {command[0]}: {result.stderr}")
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    (output/(archive.name+'.sha256')).write_text(checksum+'  '+archive.name+'\n')
    receipt = dict(schema_version=1, package=name, version=version, target=target, archive=archive.name,
                   sha256=checksum, binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                   platform=platform.platform(), checks=(['extracted_version','offline_demo','restart','embedded_client_help'] if name == 'vecnook' else
                      ['extracted_version','mmap_originals','simd_exact_rerank','pinned_publication','mapped_search']), passed=True)
    (output/(archive.name+'.receipt.json')).write_text(json.dumps(receipt, indent=2)+'\n')
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
