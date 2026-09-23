#!/usr/bin/env python3
"""Package prebuilt Convenient Codex binaries and atomically select a local release."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import uuid


REPO_ROOT = Path(__file__).resolve().parents[1]
OWNER_MARKER = "# Managed by convenient-codex: scripts/convenient-install.py"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def package_contents(directory: Path) -> dict:
    return {
        str(path.relative_to(directory)): (sha256(path), path.stat().st_mode & 0o777)
        for path in sorted(directory.rglob("*"))
        if path.is_file()
    }


def check_launcher(path: Path) -> None:
    if path.is_symlink():
        raise RuntimeError(f"Refusing to replace an existing symlink: {path}")
    if path.exists():
        if not path.is_file():
            raise RuntimeError(f"Launcher path is not a file: {path}")
        with path.open("rb") as source:
            header = source.read(256).decode("utf-8", errors="replace").splitlines()
        if OWNER_MARKER not in header:
            raise RuntimeError(f"Refusing to replace an unrelated launcher: {path}")


def install_launcher(path: Path, content: str) -> None:
    check_launcher(path)
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}-", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as output:
            output.write(content)
        os.chmod(temporary, 0o755)
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    binaries = REPO_ROOT / "codex-rs" / "target" / "debug"
    parser.add_argument("--entrypoint-bin", type=Path, default=binaries / "codex")
    parser.add_argument(
        "--code-mode-host-bin", type=Path, default=binaries / "codex-code-mode-host"
    )
    parser.add_argument("--bwrap-bin", type=Path, default=binaries / "bwrap")
    parser.add_argument(
        "--rg-bin", type=Path, help="Optional prebuilt ripgrep override"
    )
    parser.add_argument("--zsh-bin", type=Path, help="Optional patched zsh override")
    parser.add_argument(
        "--target", help="Host Rust target triple; defaults to native macOS/GNU Linux"
    )
    parser.add_argument(
        "--install-root",
        type=Path,
        default=Path.home() / ".local" / "share" / "convenient-codex",
    )
    parser.add_argument("--bin-dir", type=Path, default=Path.home() / ".local" / "bin")
    parser.add_argument(
        "--set-default",
        action="store_true",
        help="Also install an owned codex launcher; refuse unrelated files and symlinks",
    )
    args = parser.parse_args()

    if sys.version_info < (3, 10):
        raise RuntimeError(
            "The upstream package builder requires Python 3.10+; rerun with python3.12."
        )
    if sys.platform not in ("darwin", "linux"):
        raise RuntimeError("Local installation currently supports macOS and Linux.")
    machine = {"arm64": "aarch64", "amd64": "x86_64"}.get(
        platform.machine().lower(), platform.machine().lower()
    )
    target = (
        args.target
        or f"{machine}-{'apple-darwin' if sys.platform == 'darwin' else 'unknown-linux-gnu'}"
    )
    if (
        not target.startswith(f"{machine}-")
        or (sys.platform == "darwin" and not target.endswith("-apple-darwin"))
        or (sys.platform == "linux" and "-linux-" not in target)
    ):
        raise RuntimeError(f"Target {target!r} does not match this installation host.")

    manifest_path = REPO_ROOT / "CONVENIENT_CODEX.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    upstream_version = manifest["upstream"]["version"]
    revision = manifest["product"]["patch_revision"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", upstream_version):
        raise RuntimeError(
            "Manifest upstream.version must identify an official stable release."
        )
    if type(revision) is not int or revision < 1:
        raise RuntimeError(
            "Manifest product.patch_revision must be a positive integer."
        )
    version = f"{upstream_version}+convenient.{revision}"

    overrides = {
        "--entrypoint-bin": args.entrypoint_bin,
        "--code-mode-host-bin": args.code_mode_host_bin,
    }
    if sys.platform == "linux":
        overrides["--bwrap-bin"] = args.bwrap_bin
    for flag, path in (("--rg-bin", args.rg_bin), ("--zsh-bin", args.zsh_bin)):
        if path is not None:
            overrides[flag] = path
    for flag, path in list(overrides.items()):
        path = path.expanduser().resolve()
        overrides[flag] = path
        if not path.is_file() or not os.access(path, os.X_OK):
            raise RuntimeError(f"{flag} must name an existing executable: {path}")

    install_root = args.install_root.expanduser().resolve()
    bin_dir = args.bin_dir.expanduser().resolve()
    current = install_root / "current"
    if current.exists() and not current.is_symlink():
        raise RuntimeError(f"Refusing to replace a non-symlink current path: {current}")
    launchers = [bin_dir / "convenient-codex"]
    if args.set_default:
        launchers.append(bin_dir / "codex")
    for launcher in launchers:
        check_launcher(launcher)

    releases = install_root / "releases"
    releases.mkdir(parents=True, exist_ok=True)
    destination = releases / version
    source_commit = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=REPO_ROOT, text=True
    ).strip()
    source_dirty = bool(
        subprocess.check_output(
            ["git", "status", "--porcelain", "--untracked-files=normal"], cwd=REPO_ROOT
        ).strip()
    )
    with tempfile.TemporaryDirectory(prefix=f".{version}-", dir=releases) as temporary:
        package = Path(temporary) / "package"
        command = [
            sys.executable,
            str(REPO_ROOT / "scripts" / "build_codex_package.py"),
            "--variant",
            "codex",
            "--target",
            target,
            "--package-version",
            version,
            "--package-dir",
            str(package),
        ]
        for flag, path in overrides.items():
            command.extend([flag, str(path.expanduser().resolve())])
        subprocess.run(
            command,
            check=True,
            cwd=REPO_ROOT,
            env={**os.environ, "CODEX_REPO_ROOT": str(REPO_ROOT)},
        )
        executables = [
            package / "bin" / "codex",
            package / "bin" / "codex-code-mode-host",
            package / "codex-path" / "rg",
            package / "codex-resources" / "zsh" / "bin" / "zsh",
        ]
        for executable in executables:
            if not executable.is_file():
                raise RuntimeError(
                    f"Required packaged executable is missing: {executable}"
                )
            if sys.platform == "darwin":
                verified = subprocess.run(
                    ["codesign", "--verify", "--strict", str(executable)],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
                if verified.returncode:
                    subprocess.run(
                        [
                            "codesign",
                            "--force",
                            "--sign",
                            "-",
                            "--timestamp=none",
                            str(executable),
                        ],
                        check=True,
                    )
                    subprocess.run(
                        ["codesign", "--verify", "--strict", str(executable)],
                        check=True,
                    )
        shutil.copyfile(manifest_path, package / "CONVENIENT_CODEX.json")
        build_info = {
            **manifest,
            "installation": {
                "version": version,
                "target": target,
                "source_commit": source_commit,
                "source_dirty": source_dirty,
                "sha256": {
                    str(path.relative_to(package)): sha256(path) for path in executables
                },
            },
        }
        (package / "build-info.json").write_text(
            json.dumps(build_info, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        if destination.is_symlink() or (
            destination.exists()
            and (
                not destination.is_dir()
                or package_contents(destination) != package_contents(package)
            )
        ):
            raise RuntimeError(
                f"Release {destination} already exists with different content. "
                "Increment product.patch_revision in CONVENIENT_CODEX.json before installing."
            )
        if not destination.exists():
            package.rename(destination)

    launcher_content = f"""#!/bin/sh
{OWNER_MARKER}
set -eu
release=$(CDPATH= cd -- {shlex.quote(str(current))} && pwd -P)
if [ "$#" -eq 1 ] && [ "$1" = "--build-info" ]; then
    exec cat "$release/build-info.json"
fi
exec "$release/bin/codex" "$@"
"""
    bin_dir.mkdir(parents=True, exist_ok=True)
    for launcher in launchers:
        install_launcher(launcher, launcher_content)
    temporary_link = install_root / f".current-{uuid.uuid4().hex}"
    try:
        temporary_link.symlink_to(Path("releases") / version)
        os.replace(temporary_link, current)
    finally:
        temporary_link.unlink(missing_ok=True)
    print(f"Installed {version} at {destination}")
    print(f"Launcher: {launchers[0]} (metadata: --build-info)")
    print(f"Selected release: {current} -> releases/{version}")
    if args.set_default:
        print(
            f"Default launcher: {bin_dir / 'codex'} (ensure {bin_dir} precedes npm in PATH)"
        )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (
        OSError,
        RuntimeError,
        KeyError,
        ValueError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"convenient-install: {error}", file=sys.stderr)
        raise SystemExit(1)
