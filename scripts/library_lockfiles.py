"""Prepare ignored library locks once, including independent example workspaces."""
from pathlib import Path
import argparse
import json
import subprocess
import tomllib


def ensure_library_lockfiles(root=None):
    root = Path(root or Path(__file__).resolve().parents[1]).resolve()
    tracked = subprocess.check_output(["git", "-C", str(root), "ls-files", "*Cargo.toml"], text=True).splitlines()
    manifests = {root / "Cargo.toml"}
    for relative in tracked:
        manifest = root / relative
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
        if "workspace" in data:
            manifests.add(manifest)
            for excluded in data["workspace"].get("exclude", []):
                candidate = manifest.parent / excluded / "Cargo.toml"
                if candidate.is_file():
                    manifests.add(candidate)
    for manifest in sorted(manifests):
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--manifest-path", str(manifest)],
            cwd=root, text=True, encoding="utf-8"))
        lock = Path(metadata["workspace_root"]) / "Cargo.lock"
        if not lock.exists():
            subprocess.run(["cargo", "--config", 'resolver.incompatible-rust-versions="fallback"',
                            "generate-lockfile", "--manifest-path", str(manifest)], cwd=root, check=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path)
    ensure_library_lockfiles(parser.parse_args().root)
