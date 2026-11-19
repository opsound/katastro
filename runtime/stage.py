"""Stage the locked release for an ordinary macOS Cargo build."""
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def stage(profile, lock, cache):
    checksum = lock["sha256"]
    destination = profile / ("katastro-runtime-" + checksum)
    cache.mkdir(parents=True, exist_ok=True)
    with (cache / (checksum + ".lock")).open("a") as guard:
        fcntl.flock(guard, fcntl.LOCK_EX)
        archive = cache / (checksum + ".tar.gz")
        if not archive.exists() or sha256(archive) != checksum:
            if os.getenv("KATASTRO_OFFLINE") == "1" or os.getenv("CARGO_NET_OFFLINE") == "true":
                raise ValueError("offline build requires the verified runtime in " + str(cache))
            with tempfile.NamedTemporaryFile(dir=cache) as part:
                subprocess.run(["/usr/bin/curl", "--fail", "--location", "--retry", "3",
                                "-H", "Accept: application/octet-stream", lock["url"],
                                "-o", part.name], check=True)
                if sha256(Path(part.name)) != checksum:
                    raise ValueError("runtime download checksum mismatch")
                # Publish only a complete verified archive.
                path = Path(part.name)
                with contextlib.suppress(FileNotFoundError):
                    archive.unlink()
                os.link(path, archive)
        with tempfile.TemporaryDirectory(dir=profile) as directory:
            tmp = Path(directory)
            with tarfile.open(archive) as tar:
                for member in tar:
                    path = Path(member.name)
                    if path.is_absolute() or ".." in path.parts or path.parts[0] not in ("Helpers", "Resources"):
                        raise ValueError("unexpected runtime archive path")
                    if member.isdir():
                        continue
                    if not member.isfile():
                        raise ValueError("unexpected runtime archive entry")
                    out = tmp / path
                    out.parent.mkdir(parents=True, exist_ok=True)
                    with tar.extractfile(member) as src, out.open("wb") as dst:
                        shutil.copyfileobj(src, dst)
                    out.chmod(0o755 if path == Path("Helpers/katago") else 0o644)
            for path in ("Helpers/katago", "Resources/Models/review.bin.gz",
                         "Resources/Models/b18c384nbt-humanv0.bin.gz", "Resources/Notices/models.LICENSE"):
                if not (tmp / path).is_file():
                    raise ValueError("incomplete runtime: " + path)
            if not destination.exists():
                tmp.rename(destination)
            else:
                # Other builds may be using this exact release. Replace individual
                # files atomically when Cargo asks for missing or changed outputs.
                for item in tmp.rglob("*"):
                    target = destination / item.relative_to(tmp)
                    if item.is_dir():
                        target.mkdir(exist_ok=True)
                    else:
                        item.replace(target)
    return destination


if __name__ == "__main__":
    lock = json.loads(Path(__file__).with_name("macos.json").read_text())
    cache = Path(os.getenv("KATASTRO_RUNTIME_CACHE", Path.home() / "Library/Caches/Katastro/build-runtime"))
    stage(Path(sys.argv[1]), lock, cache)
