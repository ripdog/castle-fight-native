#!/usr/bin/env python3
"""Report native runtime coverage for extracted Castle Fight effects.

The extractor output is the inventory; the native-effect binding registry is the
implementation ledger. An inventory row is implemented only when a binding for
its stable source key covers the requested map version.
"""

from __future__ import annotations

import argparse
import csv
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


@dataclass(frozen=True, order=True)
class MapVersion:
    major: int
    minor: int

    @classmethod
    def parse(cls, value: str) -> "MapVersion":
        parts = value.split(".")
        if len(parts) != 2 or not all(part.isdigit() for part in parts):
            raise ValueError(f"invalid map version {value!r}; expected e.g. 9.27")
        return cls(*(int(part) for part in parts))

    def __str__(self) -> str:
        return f"{self.major}.{self.minor}"


@dataclass(frozen=True)
class Binding:
    source_kind: str
    source_key: str
    implementation: str
    valid_from: MapVersion
    valid_through: MapVersion

    def covers(self, version: MapVersion) -> bool:
        return self.valid_from <= version <= self.valid_through


@dataclass(frozen=True)
class InventoryItem:
    source_kind: str
    source_key: str
    name: str
    references: int


@dataclass(frozen=True)
class CoverageRow:
    item: InventoryItem
    implementation: str | None
    valid_from: MapVersion | None
    valid_through: MapVersion | None

    @property
    def status(self) -> str:
        return "implemented" if self.implementation is not None else "unimplemented"


def _read_tsv(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def _inventory_from_rows(
    rows: Iterable[dict[str, str]],
    source_kind: str,
    key_field: str,
    name_field: str,
) -> list[InventoryItem]:
    grouped: dict[str, tuple[str, int]] = {}
    for row in rows:
        key = row.get(key_field, "").strip()
        if not key:
            continue
        name = row.get(name_field, "").strip() or key
        previous_name, count = grouped.get(key, (name, 0))
        grouped[key] = (previous_name or name, count + 1)
    return [
        InventoryItem(source_kind, key, name, count)
        for key, (name, count) in sorted(grouped.items())
    ]


def load_inventory(resolved_dir: Path) -> list[InventoryItem]:
    items: list[InventoryItem] = []
    items.extend(
        _inventory_from_rows(
            _read_tsv(resolved_dir / "production-unit-abilities.tsv"),
            "unit-ability",
            "ability_rawcode",
            "name",
        )
    )
    items.extend(
        _inventory_from_rows(
            _read_tsv(resolved_dir / "unit-spell-semantics.tsv"),
            "unit-spell",
            "ability_rawcode",
            "ability_name",
        )
    )
    items.extend(
        _inventory_from_rows(
            _read_tsv(resolved_dir / "building-spell-mechanics.tsv"),
            "building-spell",
            "ability_rawcode",
            "ability_name",
        )
    )

    special_rows = _read_tsv(resolved_dir / "production-unit-special-mechanics.tsv")
    grouped_special: dict[str, tuple[str, int]] = {}
    for row in special_rows:
        unit = row.get("unit_rawcode", "").strip()
        mechanic = row.get("mechanic_kind", "").strip()
        if not unit or not mechanic:
            continue
        key = f"{unit}/{mechanic}"
        unit_name = row.get("unit_names", "").strip() or unit
        name = f"{unit_name}: {mechanic}"
        previous_name, count = grouped_special.get(key, (name, 0))
        grouped_special[key] = (previous_name or name, count + 1)
    items.extend(
        InventoryItem("unit-special", key, name, count)
        for key, (name, count) in sorted(grouped_special.items())
    )
    return sorted(items, key=lambda item: (item.source_kind, item.source_key))


def load_bindings(path: Path) -> list[Binding]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if payload.get("schema_version") != 1:
        raise ValueError(f"unsupported binding schema in {path}")
    bindings = [
        Binding(
            source_kind=record["source_kind"],
            source_key=record["source_key"],
            implementation=record["implementation"],
            valid_from=MapVersion.parse(record["valid_from"]),
            valid_through=MapVersion.parse(record["valid_through"]),
        )
        for record in payload.get("bindings", [])
    ]
    for binding in bindings:
        if binding.valid_through < binding.valid_from:
            raise ValueError(
                f"inverted native-effect range for {binding.source_kind} {binding.source_key}"
            )
    for index, left in enumerate(bindings):
        for right in bindings[index + 1 :]:
            if (left.source_kind, left.source_key) != (right.source_kind, right.source_key):
                continue
            if left.valid_from <= right.valid_through and right.valid_from <= left.valid_through:
                raise ValueError(
                    f"overlapping native-effect ranges for {left.source_kind} {left.source_key}"
                )
    return bindings


def build_coverage(
    inventory: Iterable[InventoryItem], bindings: Iterable[Binding], version: MapVersion
) -> list[CoverageRow]:
    by_key: dict[tuple[str, str], list[Binding]] = {}
    for binding in bindings:
        by_key.setdefault((binding.source_kind, binding.source_key), []).append(binding)

    rows = []
    for item in inventory:
        binding = next(
            (
                candidate
                for candidate in by_key.get((item.source_kind, item.source_key), [])
                if candidate.covers(version)
            ),
            None,
        )
        rows.append(
            CoverageRow(
                item=item,
                implementation=binding.implementation if binding else None,
                valid_from=binding.valid_from if binding else None,
                valid_through=binding.valid_through if binding else None,
            )
        )
    return rows


def write_tsv(rows: Iterable[CoverageRow], destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    with destination.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
        writer.writerow(
            [
                "source_kind",
                "source_key",
                "name",
                "references",
                "status",
                "implementation",
                "valid_from",
                "valid_through",
            ]
        )
        for row in rows:
            writer.writerow(
                [
                    row.item.source_kind,
                    row.item.source_key,
                    row.item.name,
                    row.item.references,
                    row.status,
                    row.implementation or "",
                    str(row.valid_from) if row.valid_from else "",
                    str(row.valid_through) if row.valid_through else "",
                ]
            )


def print_summary(rows: list[CoverageRow], version: MapVersion) -> None:
    print(f"Castle Fight {version} native-effect coverage")
    kinds = sorted({row.item.source_kind for row in rows})
    for kind in kinds:
        kind_rows = [row for row in rows if row.item.source_kind == kind]
        implemented = sum(row.status == "implemented" for row in kind_rows)
        print(f"  {kind}: {implemented}/{len(kind_rows)} implemented")
    implemented = sum(row.status == "implemented" for row in rows)
    print(f"  total: {implemented}/{len(rows)} implemented")


def main() -> int:
    script_dir = Path(__file__).resolve().parent
    repo_root = script_dir.parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--map-version", default="9.27")
    parser.add_argument(
        "--revision",
        help="exact retained extraction revision (required when a map version has multiple revisions)",
    )
    parser.add_argument(
        "--resolved-dir",
        type=Path,
        help="explicit resolved extraction directory; otherwise resolve it from releases.json",
    )
    parser.add_argument(
        "--bindings",
        type=Path,
        default=repo_root / "crates/sim/data/castle-fight/native-effect-bindings.json",
    )
    parser.add_argument("--output-tsv", type=Path)
    parser.add_argument(
        "--show-unimplemented",
        action="store_true",
        help="print every extracted inventory key with no native binding for this version",
    )
    args = parser.parse_args()

    version = MapVersion.parse(args.map_version)
    resolved_dir = args.resolved_dir
    if resolved_dir is None:
        import release_manifest

        try:
            release = release_manifest.resolve_release(
                release_manifest.load_manifest(), args.map_version, args.revision
            )
        except release_manifest.ManifestError as error:
            parser.error(str(error))
        if release["extraction"]["status"] != "retained":
            parser.error(
                f"Castle Fight {args.map_version}/{release['revision']} has no retained extraction"
            )
        resolved_dir = repo_root / release["extraction"]["working_alias"] / "resolved"

    rows = build_coverage(load_inventory(resolved_dir), load_bindings(args.bindings), version)
    print_summary(rows, version)
    if args.output_tsv:
        write_tsv(rows, args.output_tsv)
    if args.show_unimplemented:
        for row in rows:
            if row.status == "unimplemented":
                print(f"UNIMPLEMENTED\t{row.item.source_kind}\t{row.item.source_key}\t{row.item.name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
