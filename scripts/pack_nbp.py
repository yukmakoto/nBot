import argparse
import gzip
import os
import tarfile
from pathlib import Path


DEFAULT_EXCLUDES = {
    ".git",
    ".github",
    "node_modules",
    "dist",
    "__pycache__",
}


def should_exclude(path: Path, excludes: set[str]) -> bool:
    for part in path.parts:
        if part in excludes:
            return True
    return False


def to_posix_relpath(path: Path) -> str:
    return str(path).replace("\\", "/").lstrip("./")


def pack_nbp(src_dir: Path, out_file: Path, excludes: set[str]) -> None:
    src_dir = src_dir.resolve()
    if not src_dir.is_dir():
        raise SystemExit(f"src_dir not found or not a directory: {src_dir}")

    manifest = src_dir / "manifest.json"
    if not manifest.is_file():
        raise SystemExit(f"manifest.json not found in: {src_dir}")

    out_file.parent.mkdir(parents=True, exist_ok=True)

    # Write tar.gz with POSIX paths (nbp parser expects '/' separators).
    with out_file.open("wb") as raw:
        with gzip.GzipFile(fileobj=raw, mode="wb") as gz:
            with tarfile.open(fileobj=gz, mode="w") as tf:
                for p in sorted(src_dir.rglob("*")):
                    if p.is_dir():
                        continue
                    rel = p.relative_to(src_dir)
                    if should_exclude(rel, excludes):
                        continue
                    arcname = to_posix_relpath(rel)
                    info = tf.gettarinfo(str(p), arcname=arcname)
                    with p.open("rb") as f:
                        tf.addfile(info, fileobj=f)


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Pack a plugin directory into nBot .nbp format (tar.gz with manifest.json + files)."
    )
    ap.add_argument("--src", required=True, help="Plugin directory containing manifest.json")
    ap.add_argument("--out", required=True, help="Output .nbp path")
    ap.add_argument(
        "--exclude",
        action="append",
        default=[],
        help="Exclude path segment (repeatable), e.g. --exclude node_modules",
    )
    args = ap.parse_args()

    src_dir = Path(args.src)
    out_file = Path(args.out)
    excludes = set(DEFAULT_EXCLUDES)
    excludes.update([x.strip() for x in args.exclude if x and x.strip()])

    pack_nbp(src_dir, out_file, excludes)
    print(f"ok: wrote {out_file}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

