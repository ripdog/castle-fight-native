#!/usr/bin/env python3
"""Extract only the Warcraft III terrain textures referenced by a W3E decode.

The map extractor already commits the terrain palette rawcodes in terrain.json.
This tool resolves those rawcodes through the local Warcraft installation's
Terrain.slk/CliffTypes.slk and copies only the matching classic terrain atlases.
It deliberately does not scan or export unit/model assets.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_INSTALL = Path(os.environ.get("WC3_INSTALL", "/mnt/gamessd_linux/Games/Warcraft3"))
DEFAULT_TERRAIN = REPO_ROOT / "docs/original_map/extracted/terrain.json"
DEFAULT_OUTPUT = REPO_ROOT / "assets/wc3/terrain"
DEFAULT_CASC = REPO_ROOT / ".local-tools/bin/casc_extract"
BOOTSTRAP_CASC = REPO_ROOT / "tools/wc3-map/bootstrap-casclib.sh"

GROUND_METADATA = r"war3.w3mod:terrainart\terrain.slk"
CLIFF_METADATA = r"war3.w3mod:terrainart\clifftypes.slk"
TEXTURE_EXTENSIONS = ("dds", "blp", "tga", "png")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Extract Castle Fight's referenced Warcraft III terrain textures",
    )
    parser.add_argument(
        "--wc3-install",
        type=Path,
        default=DEFAULT_INSTALL,
        help=f"Warcraft III CASC install (default: {DEFAULT_INSTALL})",
    )
    parser.add_argument(
        "--terrain",
        type=Path,
        default=DEFAULT_TERRAIN,
        help="decoded W3E terrain.json whose tile palettes should be exported",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT,
        help=f"generated asset directory (default: {DEFAULT_OUTPUT})",
    )
    parser.add_argument(
        "--casc",
        type=Path,
        default=DEFAULT_CASC,
        help="casc_extract helper; bootstrapped automatically at the default path",
    )
    parser.add_argument(
        "--keep-source",
        action="store_true",
        help="retain the original Blizzard DDS/BLP/TGA files beside converted PNGs",
    )
    return parser.parse_args()


def parse_sylk(path: Path) -> dict[str, dict[int, str | int | float]]:
    """Parse the small row/column subset used by Warcraft's Terrain SLKs."""

    rows: dict[int, dict[int, str | int | float]] = {}
    current_x: int | None = None
    current_y: int | None = None
    with path.open("r", encoding="utf-8-sig", errors="strict", newline="") as handle:
        for raw_line in handle:
            line = raw_line.rstrip("\r\n")
            if not line.startswith("C;"):
                continue
            fields = line.split(";")[1:]
            value: str | int | float | None = None
            has_value = False
            for field in fields:
                if field.startswith("X") and field[1:].isdigit():
                    current_x = int(field[1:])
                elif field.startswith("Y") and field[1:].isdigit():
                    current_y = int(field[1:])
                elif field.startswith("K"):
                    value = parse_sylk_value(field[1:])
                    has_value = True
            if has_value and current_x is not None and current_y is not None:
                rows.setdefault(current_y, {})[current_x] = value  # type: ignore[assignment]

    by_rawcode: dict[str, dict[int, str | int | float]] = {}
    for row in rows.values():
        rawcode = row.get(1)
        if isinstance(rawcode, str) and len(rawcode) == 4:
            by_rawcode[rawcode] = row
    return by_rawcode


def parse_sylk_value(value: str) -> str | int | float:
    if len(value) >= 2 and value[0] == '"' and value[-1] == '"':
        return value[1:-1].replace('""', '"')
    try:
        return int(value, 10)
    except ValueError:
        try:
            return float(value)
        except ValueError:
            return value


def ensure_casc(casc: Path) -> None:
    if casc.is_file() and os.access(casc, os.X_OK):
        return
    if casc != DEFAULT_CASC:
        raise FileNotFoundError(f"casc_extract is not executable: {casc}")
    subprocess.run([str(BOOTSTRAP_CASC)], cwd=REPO_ROOT, check=True)
    if not casc.is_file():
        raise FileNotFoundError(f"bootstrap did not create casc_extract: {casc}")


def casc_extract(casc: Path, install: Path, logical_path: str, output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(
        [str(casc), str(install), "extract", logical_path, str(output)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "unknown CASC extraction failure"
        raise RuntimeError(f"failed to extract {logical_path}: {detail}")


def try_casc_extract(
    casc: Path,
    install: Path,
    logical_path: str,
    output: Path,
) -> bool:
    output.parent.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(
        [str(casc), str(install), "extract", logical_path, str(output)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if result.returncode == 0 and output.is_file():
        return True
    output.unlink(missing_ok=True)
    return False


def metadata_string(row: dict[int, Any], column: int, rawcode: str, table: str) -> str:
    value = row.get(column)
    if not isinstance(value, str) or not value:
        raise ValueError(f"{table} row {rawcode} is missing string column X{column}")
    return value


def texture_request(
    rawcode: str,
    palette_index: int,
    row: dict[int, Any],
    kind: str,
) -> dict[str, Any]:
    if kind == "ground":
        directory = metadata_string(row, 3, rawcode, "Terrain.slk")
        stem = metadata_string(row, 4, rawcode, "Terrain.slk")
        display_name = str(row.get(5, rawcode))
    elif kind == "cliff":
        directory = metadata_string(row, 4, rawcode, "CliffTypes.slk")
        stem = metadata_string(row, 5, rawcode, "CliffTypes.slk")
        display_name = str(row.get(6, rawcode))
    else:
        raise ValueError(f"unknown terrain texture kind {kind!r}")

    logical_stem = f"{directory}\\{stem}"
    return {
        "rawcode": rawcode,
        "palette_index": palette_index,
        "display_name": display_name,
        "logical_stem": logical_stem,
    }


def find_texture_source(
    casc: Path,
    install: Path,
    request: dict[str, Any],
    scratch: Path,
) -> tuple[str, Path, str]:
    logical_stem = request["logical_stem"]
    for extension in TEXTURE_EXTENSIONS:
        logical_path = f"war3.w3mod:{logical_stem}.{extension}"
        source_path = scratch / f"{request['rawcode']}.{extension}"
        if try_casc_extract(casc, install, logical_path, source_path):
            return logical_path, source_path, extension
    raise FileNotFoundError(
        f"no classic terrain texture found for {request['rawcode']} ({logical_stem}); "
        f"tried {', '.join(TEXTURE_EXTENSIONS)}"
    )


def find_converter() -> tuple[str, str] | None:
    magick = shutil.which("magick")
    if magick:
        return ("magick", magick)
    convert = shutil.which("convert")
    if convert:
        return ("convert", convert)
    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg:
        return ("ffmpeg", ffmpeg)
    return None


def convert_to_png(source: Path, source_extension: str, destination: Path) -> str:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if source_extension == "png":
        shutil.copyfile(source, destination)
        return "copy"

    converter = find_converter()
    if converter is None:
        raise RuntimeError(
            "terrain texture conversion needs ImageMagick (`magick`) or ffmpeg; "
            "install ImageMagick or ffmpeg and rerun"
        )

    kind, executable = converter
    if kind == "magick":
        command = [executable, f"{source}[0]", str(destination)]
    elif kind == "convert":
        command = [executable, f"{source}[0]", str(destination)]
    else:
        command = [executable, "-loglevel", "error", "-y", "-i", str(source), "-frames:v", "1", str(destination)]

    result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if result.returncode != 0 or not destination.is_file():
        detail = result.stderr.strip() or result.stdout.strip() or "unknown converter failure"
        if source_extension == "blp":
            detail += (
                "; this install exposes classic terrain as BLP. The general wc3 asset extractor "
                "owns BLP decoding; terrain extraction intentionally does not duplicate that decoder"
            )
        raise RuntimeError(f"failed converting {source.name} to PNG: {detail}")
    return kind


def image_dimensions(path: Path) -> tuple[int, int]:
    with path.open("rb") as handle:
        header = handle.read(32)
    if header.startswith(b"\x89PNG\r\n\x1a\n") and len(header) >= 24:
        return struct.unpack(">II", header[16:24])
    if header.startswith(b"DDS ") and len(header) >= 20:
        height, width = struct.unpack("<II", header[12:20])
        return width, height
    raise ValueError(f"cannot determine dimensions for {path}")


def validate_ground_atlas(width: int, height: int, rawcode: str) -> dict[str, Any]:
    if height <= 0 or height % 4 != 0:
        raise ValueError(f"ground texture {rawcode} has unsupported dimensions {width}x{height}")
    tile_pixels = height // 4
    if width not in (height, height * 2) or width % tile_pixels != 0:
        raise ValueError(
            f"ground texture {rawcode} has unsupported atlas shape {width}x{height}; "
            "expected Warcraft square or 2:1 extended terrain atlas"
        )
    return {
        "tile_pixels": tile_pixels,
        "extended": width > height,
        "blend_columns": 4,
        "blend_rows": 4,
        "full_variant_columns": 4 if width > height else 0,
        "full_variant_rows": 4 if width > height else 0,
    }


def export_texture(
    casc: Path,
    install: Path,
    request: dict[str, Any],
    output: Path,
    scratch: Path,
    keep_source: bool,
    kind: str,
) -> dict[str, Any]:
    source_casc_path, source_path, source_extension = find_texture_source(
        casc, install, request, scratch
    )
    relative_png = Path(kind) / f"{request['rawcode']}.png"
    png_path = output / relative_png
    converter = convert_to_png(source_path, source_extension, png_path)
    width, height = image_dimensions(png_path)

    entry = {
        "rawcode": request["rawcode"],
        "palette_index": request["palette_index"],
        "display_name": request["display_name"],
        "source_casc_path": source_casc_path,
        "source_format": source_extension,
        "png": relative_png.as_posix(),
        "width": width,
        "height": height,
        "conversion": converter,
    }
    if kind == "ground":
        entry["atlas"] = validate_ground_atlas(width, height, request["rawcode"])

    if keep_source:
        relative_source = Path("source") / kind / f"{request['rawcode']}.{source_extension}"
        source_destination = output / relative_source
        source_destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source_path, source_destination)
        entry["source_copy"] = relative_source.as_posix()
    return entry


def relative_to_repo(path: Path) -> str:
    try:
        return path.resolve().relative_to(REPO_ROOT).as_posix()
    except ValueError:
        return str(path.resolve())


def main() -> int:
    args = parse_args()
    install = args.wc3_install.resolve()
    terrain_path = args.terrain.resolve()
    output = args.output.resolve()
    casc = args.casc.resolve()

    if not (install / ".build.info").is_file():
        raise FileNotFoundError(
            f"Warcraft III CASC install not found at {install}; pass --wc3-install or set WC3_INSTALL"
        )
    if not terrain_path.is_file():
        raise FileNotFoundError(f"terrain JSON not found: {terrain_path}")

    ensure_casc(casc)
    terrain = json.loads(terrain_path.read_text(encoding="utf-8"))
    ground_palette = terrain.get("tilePalette")
    cliff_palette = terrain.get("cliffTilePalette")
    if not isinstance(ground_palette, list) or not all(isinstance(value, str) for value in ground_palette):
        raise ValueError("terrain JSON has no valid tilePalette")
    if not isinstance(cliff_palette, list) or not all(isinstance(value, str) for value in cliff_palette):
        raise ValueError("terrain JSON has no valid cliffTilePalette")

    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="cf-terrain-") as temp_name:
        temp = Path(temp_name)
        terrain_slk = temp / "terrain.slk"
        cliff_slk = temp / "clifftypes.slk"
        casc_extract(casc, install, GROUND_METADATA, terrain_slk)
        casc_extract(casc, install, CLIFF_METADATA, cliff_slk)
        terrain_rows = parse_sylk(terrain_slk)
        cliff_rows = parse_sylk(cliff_slk)

        ground_requests = []
        for index, rawcode in enumerate(ground_palette):
            row = terrain_rows.get(rawcode)
            if row is None:
                raise KeyError(f"ground rawcode {rawcode} is missing from Terrain.slk")
            ground_requests.append(texture_request(rawcode, index, row, "ground"))

        cliff_requests = []
        for index, rawcode in enumerate(cliff_palette):
            row = cliff_rows.get(rawcode)
            if row is None:
                raise KeyError(f"cliff rawcode {rawcode} is missing from CliffTypes.slk")
            cliff_requests.append(texture_request(rawcode, index, row, "cliff"))

        scratch_ground = temp / "ground"
        scratch_cliff = temp / "cliff"
        ground_entries = [
            export_texture(
                casc,
                install,
                request,
                output,
                scratch_ground,
                args.keep_source,
                "ground",
            )
            for request in ground_requests
        ]
        cliff_entries = [
            export_texture(
                casc,
                install,
                request,
                output,
                scratch_cliff,
                args.keep_source,
                "cliff",
            )
            for request in cliff_requests
        ]

    map_info = terrain.get("map", {})
    manifest = {
        "schema_version": 1,
        "source": {
            "terrain_json": relative_to_repo(terrain_path),
            "wc3_build_info": str((install / ".build.info").resolve()),
            "asset_policy": "generated locally from the user's Warcraft III installation; do not commit",
        },
        "map": {
            "width": map_info.get("width"),
            "height": map_info.get("height"),
            "tile_palette": ground_palette,
            "cliff_tile_palette": cliff_palette,
            "ground_variation_encoding": "upper five bits of W3E detail byte; decoded as value >> 3",
        },
        "ground": ground_entries,
        "cliff": cliff_entries,
    }
    (output / "manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )

    print(
        f"exported {len(ground_entries)} ground and {len(cliff_entries)} cliff terrain textures "
        f"to {output}"
    )
    for entry in ground_entries + cliff_entries:
        print(
            f"{entry['rawcode']}\t{entry['width']}x{entry['height']}\t"
            f"{entry['source_casc_path']}\t{entry['png']}"
        )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (FileNotFoundError, KeyError, RuntimeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
