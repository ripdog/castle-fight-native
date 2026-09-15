#!/usr/bin/env python3
"""Lex protected Warcraft III Lua without executing it.

The original map script is W3P-protected and effectively minified onto one very
large line. Regex-only indexing is misleading because it can match encrypted
string contents, misses method declarations, and reports character offsets
rather than byte offsets. This module performs a small Lua-aware lexical scan so
script indexes remain source-accurate without evaluating map code.
"""

from __future__ import annotations

from collections import Counter, defaultdict, deque
from dataclasses import dataclass
from decimal import Decimal, InvalidOperation
import re
from typing import Iterable, Iterator


@dataclass(frozen=True, slots=True)
class LuaToken:
    kind: str
    text: str
    start: int
    end: int
    integer_value: int | None = None


@dataclass(slots=True)
class Block:
    kind: str
    name: str | None = None
    start: int = 0
    awaiting_do: bool = False


class TokenStream:
    def __init__(self, tokens: Iterable[LuaToken]):
        self._tokens = iter(tokens)
        self._buffer: deque[LuaToken] = deque()

    def peek(self, index: int = 0) -> LuaToken | None:
        while len(self._buffer) <= index:
            try:
                self._buffer.append(next(self._tokens))
            except StopIteration:
                return None
        return self._buffer[index]

    def pop(self) -> LuaToken | None:
        if self._buffer:
            return self._buffer.popleft()
        return next(self._tokens, None)


def _long_bracket_close(data: bytes, start: int) -> tuple[bytes, int] | None:
    if start >= len(data) or data[start] != ord("["):
        return None
    index = start + 1
    while index < len(data) and data[index] == ord("="):
        index += 1
    if index >= len(data) or data[index] != ord("["):
        return None
    equals = index - start - 1
    return b"]" + (b"=" * equals) + b"]", index + 1


def iter_lua_tokens(data: bytes) -> Iterator[LuaToken]:
    """Yield significant Lua tokens while skipping comments and string bodies."""
    length = len(data)
    index = 0
    while index < length:
        byte = data[index]

        if byte in b" \t\r\n\v\f":
            index += 1
            continue

        if data.startswith(b"--", index):
            long_comment = _long_bracket_close(data, index + 2)
            if long_comment is not None:
                close, body_start = long_comment
                close_at = data.find(close, body_start)
                index = length if close_at < 0 else close_at + len(close)
            else:
                newline = data.find(b"\n", index + 2)
                index = length if newline < 0 else newline + 1
            continue

        if byte in (ord("'"), ord('"')):
            quote = byte
            index += 1
            while index < length:
                current = data[index]
                if current == ord("\\"):
                    index = min(length, index + 2)
                elif current == quote:
                    index += 1
                    break
                else:
                    index += 1
            continue

        long_string = _long_bracket_close(data, index)
        if long_string is not None:
            close, body_start = long_string
            close_at = data.find(close, body_start)
            index = length if close_at < 0 else close_at + len(close)
            continue

        if byte == ord("_") or ord("A") <= byte <= ord("Z") or ord("a") <= byte <= ord("z"):
            start = index
            index += 1
            while index < length:
                current = data[index]
                if not (
                    current == ord("_")
                    or ord("A") <= current <= ord("Z")
                    or ord("a") <= current <= ord("z")
                    or ord("0") <= current <= ord("9")
                ):
                    break
                index += 1
            yield LuaToken("ident", data[start:index].decode("ascii"), start, index)
            continue

        if ord("0") <= byte <= ord("9"):
            start = index
            is_integer = True
            if data.startswith((b"0x", b"0X"), index):
                index += 2
                while index < length and (
                    ord("0") <= data[index] <= ord("9")
                    or ord("A") <= data[index] <= ord("F")
                    or ord("a") <= data[index] <= ord("f")
                ):
                    index += 1
                is_integer = False
            else:
                while index < length and ord("0") <= data[index] <= ord("9"):
                    index += 1
                if index < length and data[index] == ord(".") and not data.startswith(b"..", index):
                    is_integer = False
                    index += 1
                    while index < length and ord("0") <= data[index] <= ord("9"):
                        index += 1
                if index < length and data[index] in (ord("e"), ord("E")):
                    is_integer = False
                    index += 1
                    if index < length and data[index] in (ord("+"), ord("-")):
                        index += 1
                    while index < length and ord("0") <= data[index] <= ord("9"):
                        index += 1
            raw = data[start:index].decode("ascii")
            yield LuaToken("number", raw, start, index, int(raw) if is_integer else None)
            continue

        if data.startswith(b"...", index):
            yield LuaToken("symbol", "...", index, index + 3)
            index += 3
            continue
        if data.startswith((b"..", b"//", b"<<", b">>", b"<=", b">=", b"==", b"~=", b"::"), index):
            yield LuaToken("symbol", data[index:index + 2].decode("ascii"), index, index + 2)
            index += 2
            continue

        yield LuaToken("symbol", chr(byte), index, index + 1)
        index += 1


def _current_function(blocks: list[Block]) -> str:
    for block in reversed(blocks):
        if block.kind == "function":
            return block.name or f"<anonymous@{block.start}>"
    return "<top-level>"


def _parse_function_name(stream: TokenStream) -> tuple[str | None, LuaToken | None]:
    first = stream.peek()
    if first is None:
        return None, None
    if first.kind != "ident":
        return None, stream.pop()

    parts = [stream.pop().text]
    while True:
        separator = stream.peek()
        member = stream.peek(1)
        if (
            separator is None
            or member is None
            or separator.kind != "symbol"
            or separator.text not in (".", ":")
            or member.kind != "ident"
        ):
            break
        parts.append(stream.pop().text)
        parts.append(stream.pop().text)
    return "".join(parts), stream.pop()


def _is_runtime_mutator(callee: str) -> tuple[bool, str]:
    base = callee.replace(":", ".").rsplit(".", 1)[-1]
    normalized = base.removeprefix("__wurst_safe_")
    if normalized.startswith("BlzSetUnit") or normalized.startswith("BlzSetAbility"):
        return True, normalized
    if normalized in {
        "SetUnitAbilityLevel",
        "SetUnitMoveSpeed",
        "SetUnitState",
        "UnitAddAbility",
        "UnitRemoveAbility",
    }:
        return True, normalized
    return False, normalized


def _function_body_tokens(
    data: bytes,
    functions: list[dict[str, object]],
    name: str,
) -> tuple[int, list[LuaToken]] | None:
    matches = [function for function in functions if function["name"] == name]
    if not matches:
        return None
    if len(matches) != 1:
        raise ValueError(f"expected exactly one Lua function named {name!r}, found {len(matches)}")
    start = int(matches[0]["start"])
    end = int(matches[0]["end"])
    return start, list(iter_lua_tokens(data[start:end]))


def _parenthesized_arguments(tokens: list[LuaToken], opening_index: int) -> tuple[list[list[LuaToken]], int]:
    if opening_index >= len(tokens) or tokens[opening_index].text != "(":
        raise ValueError("expected Lua opening parenthesis")
    args: list[list[LuaToken]] = []
    current: list[LuaToken] = []
    paren_depth = 1
    bracket_depth = 0
    brace_depth = 0
    index = opening_index + 1
    while index < len(tokens):
        token = tokens[index]
        if token.kind == "symbol":
            if token.text == "(":
                paren_depth += 1
            elif token.text == ")":
                paren_depth -= 1
                if paren_depth == 0:
                    args.append(current)
                    return args, index + 1
            elif token.text == "[":
                bracket_depth += 1
            elif token.text == "]":
                bracket_depth -= 1
            elif token.text == "{":
                brace_depth += 1
            elif token.text == "}":
                brace_depth -= 1
            elif token.text == "," and paren_depth == 1 and bracket_depth == 0 and brace_depth == 0:
                args.append(current)
                current = []
                index += 1
                continue
        if paren_depth <= 0 or bracket_depth < 0 or brace_depth < 0:
            raise ValueError(f"malformed Lua parenthesized expression near byte {tokens[opening_index].start}")
        current.append(token)
        index += 1
    raise ValueError(f"unterminated Lua parenthesized expression near byte {tokens[opening_index].start}")


def _call_arguments(tokens: list[LuaToken], callee_index: int) -> tuple[list[list[LuaToken]], int]:
    if callee_index + 1 >= len(tokens) or tokens[callee_index + 1].text != "(":
        raise ValueError(f"expected call after {tokens[callee_index].text!r}")
    return _parenthesized_arguments(tokens, callee_index + 1)


def _numeric_literal_text(tokens: list[LuaToken]) -> str:
    if not tokens:
        raise ValueError("empty Lua numeric literal")
    text = "".join(token.text for token in tokens)
    allowed = all(
        token.kind == "number" or (token.kind == "symbol" and token.text in {"+", "-", "."})
        for token in tokens
    )
    if not allowed:
        raise ValueError(f"non-literal Lua numeric expression: {text!r}")
    try:
        Decimal(text)
    except InvalidOperation as error:
        raise ValueError(f"invalid Lua numeric literal: {text!r}") from error
    return text


def _protected_row_reference(tokens: list[LuaToken]) -> tuple[int, int]:
    if len(tokens) < 7 or tokens[0].kind != "ident" or tokens[0].text != "_I" or tokens[1].text != "[":
        raise ValueError("protected ability row is not referenced through _I[...](rawcode, level)")
    if tokens[-1].text != ")":
        raise ValueError("protected ability row reference does not end in a call")
    depth = 0
    opening = None
    for index in range(len(tokens) - 1, -1, -1):
        token = tokens[index]
        if token.text == ")":
            depth += 1
        elif token.text == "(":
            depth -= 1
            if depth == 0:
                opening = index
                break
    if opening is None or opening == 0 or tokens[opening - 1].text != "]":
        raise ValueError("protected ability row reference has unexpected registry-call shape")
    row_args = tokens[opening + 1 : -1]
    if (
        len(row_args) != 3
        or row_args[0].kind != "number"
        or row_args[0].integer_value is None
        or row_args[1].text != ","
        or row_args[2].kind != "number"
        or row_args[2].integer_value is None
    ):
        raise ValueError("protected ability row reference requires integer rawcode and level index")
    rawcode = int(row_args[0].integer_value)
    level_index = int(row_args[2].integer_value)
    if rawcode <= 0 or level_index < 0:
        raise ValueError("protected ability row has invalid rawcode or level index")
    return rawcode, level_index


def _extract_protected_ability_fields(
    data: bytes,
    functions: list[dict[str, object]],
) -> list[dict[str, object]]:
    body = _function_body_tokens(data, functions, "xD")
    if body is None:
        return []
    function_start, tokens = body
    callees = {
        "AbilityLevelFields_AbilityLevelFields_cd": "cooldown",
        "AbilityLevelFields_AbilityLevelFields_mana": "mana_cost",
    }
    fields: list[dict[str, object]] = []
    seen: set[tuple[int, int, str]] = set()
    for index, token in enumerate(tokens):
        if token.kind != "ident" or token.text not in callees:
            continue
        args, _next = _call_arguments(tokens, index)
        if len(args) != 2:
            raise ValueError(f"{token.text} in xD must have exactly two arguments")
        rawcode, level_index = _protected_row_reference(args[0])
        value_text = _numeric_literal_text(args[1])
        field = callees[token.text]
        value = Decimal(value_text)
        if field == "mana_cost" and value != value.to_integral_value():
            raise ValueError(f"protected mana cost must be an integer, got {value_text!r}")
        key = (rawcode, level_index, field)
        if key in seen:
            raise ValueError(f"duplicate protected ability field in xD: {key}")
        seen.add(key)
        fields.append({
            "ability_id": rawcode,
            "level_index": level_index,
            "field": field,
            "value_text": value_text,
            "byte_offset": function_start + token.start,
            "source_function": "xD",
            "jass_add_restore": False,
        })
    return fields


def _extract_jass_add_protected_fields(
    data: bytes,
    functions: list[dict[str, object]],
) -> list[dict[str, object]]:
    function_name = "applyProtectedAbilityFieldsForJassAdd"
    body = _function_body_tokens(data, functions, function_name)
    if body is None:
        return []
    function_start, tokens = body
    mutators = {
        "__wurst_safe_BlzSetAbilityRealLevelField": ("ABILITY_RLF_COOLDOWN", "cooldown"),
        "BlzSetAbilityRealLevelField": ("ABILITY_RLF_COOLDOWN", "cooldown"),
        "__wurst_safe_BlzSetAbilityIntegerLevelField": ("ABILITY_ILF_MANA_COST", "mana_cost"),
        "BlzSetAbilityIntegerLevelField": ("ABILITY_ILF_MANA_COST", "mana_cost"),
    }
    current_rawcode: int | None = None
    guard_variable: str | None = None
    fields: list[dict[str, object]] = []
    seen: set[tuple[int, int, str]] = set()

    for index, token in enumerate(tokens):
        if token.kind == "ident" and token.text in {"if", "elseif"}:
            condition = tokens[index + 1 : index + 7]
            if (
                len(condition) == 6
                and condition[0].text == "("
                and condition[1].kind == "ident"
                and condition[2].text == "=="
                and condition[3].kind == "number"
                and condition[3].integer_value is not None
                and condition[4].text == ")"
                and condition[5].kind == "ident"
                and condition[5].text == "then"
            ):
                variable = condition[1].text
                if guard_variable is None:
                    guard_variable = variable
                elif variable != guard_variable:
                    continue
                current_rawcode = int(condition[3].integer_value)
            continue

        if token.kind != "ident" or token.text not in mutators:
            continue
        if current_rawcode is None:
            raise ValueError(f"protected JASS-add mutator at byte {function_start + token.start} has no rawcode guard")
        args, _next = _call_arguments(tokens, index)
        if len(args) != 4:
            raise ValueError(f"{token.text} in {function_name} must have four arguments")
        field_constant, field = mutators[token.text]
        if len(args[1]) != 1 or args[1][0].kind != "ident" or args[1][0].text != field_constant:
            raise ValueError(f"unexpected protected JASS-add field constant in {token.text}")
        if len(args[2]) != 1 or args[2][0].kind != "number" or args[2][0].integer_value is None:
            raise ValueError(f"protected JASS-add level index must be an integer in {token.text}")
        level_index = int(args[2][0].integer_value)
        value_text = _numeric_literal_text(args[3])
        value = Decimal(value_text)
        if field == "mana_cost" and value != value.to_integral_value():
            raise ValueError(f"protected JASS-add mana cost must be an integer, got {value_text!r}")
        key = (current_rawcode, level_index, field)
        if key in seen:
            raise ValueError(f"duplicate protected JASS-add field: {key}")
        seen.add(key)
        fields.append({
            "ability_id": current_rawcode,
            "level_index": level_index,
            "field": field,
            "value_text": value_text,
            "byte_offset": function_start + token.start,
            "source_function": function_name,
        })
    return fields


def _cross_check_protected_ability_fields(
    protected_fields: list[dict[str, object]],
    jass_add_fields: list[dict[str, object]],
) -> None:
    """Mark overlapping JASS-add restores and require exact agreement.

    The generated JASS-add compatibility helper contains one field (`A010`
    cooldown) whose static object value is already correct, so it is not
    present in the canonical protected table. Such JASS-only rows are retained
    for downstream comparison with resolved object data rather than rejected.
    """
    protected = {
        (int(row["ability_id"]), int(row["level_index"]), str(row["field"])): row
        for row in protected_fields
    }
    for jass_row in jass_add_fields:
        key = (
            int(jass_row["ability_id"]),
            int(jass_row["level_index"]),
            str(jass_row["field"]),
        )
        table_row = protected.get(key)
        if table_row is None:
            jass_row["canonical_relation"] = "jass-only"
            continue
        if Decimal(str(table_row["value_text"])) != Decimal(str(jass_row["value_text"])):
            raise ValueError(
                f"JASS-add protected field disagrees with xD table for {key}: "
                f"{jass_row['value_text']} != {table_row['value_text']}"
            )
        table_row["jass_add_restore"] = True
        jass_row["canonical_relation"] = "canonical-match"


def _decimal_text(value: Decimal) -> str:
    text = format(value, "f")
    if "." in text:
        text = text.rstrip("0").rstrip(".")
    return text or "0"


def _decimal_literal_value(tokens: list[LuaToken]) -> Decimal:
    literal_tokens = tokens
    if len(literal_tokens) >= 2 and literal_tokens[0].text == "(" and literal_tokens[-1].text == ")":
        literal_tokens = literal_tokens[1:-1]
    return Decimal(_numeric_literal_text(literal_tokens))


def _integer_literal_value(tokens: list[LuaToken]) -> int:
    value = _decimal_literal_value(tokens)
    if value != value.to_integral_value():
        raise ValueError(f"expected integer Lua literal, got {value}")
    return int(value)


def _boolean_literal_value(tokens: list[LuaToken]) -> bool:
    if len(tokens) != 1 or tokens[0].kind != "ident" or tokens[0].text not in {"true", "false"}:
        text = "".join(token.text for token in tokens)
        raise ValueError(f"expected boolean Lua literal, got {text!r}")
    return tokens[0].text == "true"


def _expect_token(tokens: list[LuaToken], index: int, text: str) -> LuaToken:
    if index >= len(tokens) or tokens[index].text != text:
        actual = tokens[index].text if index < len(tokens) else "<eof>"
        raise ValueError(f"expected Lua token {text!r}, got {actual!r}")
    return tokens[index]


def _parse_int_to_real(tokens: list[LuaToken], index: int) -> tuple[Decimal, int]:
    _expect_token(tokens, index, "int_toReal")
    _expect_token(tokens, index + 1, "(")
    value = tokens[index + 2]
    if value.kind != "number" or value.integer_value is None:
        raise ValueError("int_toReal effective-stat source must use an integer literal")
    _expect_token(tokens, index + 3, ")")
    return Decimal(int(value.integer_value)), index + 4


def _parse_scaled_int_to_real(tokens: list[LuaToken], index: int) -> tuple[Decimal, int]:
    _expect_token(tokens, index, "(")
    numerator, next_index = _parse_int_to_real(tokens, index + 1)
    _expect_token(tokens, next_index, "/")
    denominator_tokens = [tokens[next_index + 1]]
    denominator = Decimal(_numeric_literal_text(denominator_tokens))
    if denominator == 0:
        raise ValueError("effective-stat scale denominator must be nonzero")
    _expect_token(tokens, next_index + 2, ")")
    return numerator / denominator, next_index + 3


def _wurst_registry_call_arguments(tokens: list[LuaToken], index: int) -> tuple[list[list[LuaToken]], int]:
    _expect_token(tokens, index, "_I")
    _expect_token(tokens, index + 1, "[")
    depth = 0
    closing = None
    cursor = index + 1
    while cursor < len(tokens):
        token = tokens[cursor]
        if token.text == "[":
            depth += 1
        elif token.text == "]":
            depth -= 1
            if depth == 0:
                closing = cursor
                break
        cursor += 1
    if closing is None or closing + 1 >= len(tokens) or tokens[closing + 1].text != "(":
        raise ValueError(f"Wurst registry reference at byte {tokens[index].start} has no call")
    return _parenthesized_arguments(tokens, closing + 1)


def _extract_unit_object_metadata(
    data: bytes,
    functions: list[dict[str, object]],
) -> tuple[list[dict[str, object]], int]:
    """Recover the generated CFBuilding/UnitObjectMeta gameplay metadata table.

    String literals are intentionally skipped by the lexer, but all gameplay
    arguments to UnitObjectMeta_new_UnitObjectMeta remain exact numeric/boolean
    literals: spawned unit, costs, food/legendary marker, spawn interval,
    attack/defense indexes, and air/melee/mechanical/caster flags.
    """
    function_name = "ensureUnitObjectMetadataRegistered"
    body = _function_body_tokens(data, functions, function_name)
    if body is None:
        return [], 41
    function_start, tokens = body
    rows: list[dict[str, object]] = []
    seen_buildings: set[int] = set()

    for index, token in enumerate(tokens):
        if token.kind != "ident" or token.text != "UnitObjectMeta_new_UnitObjectMeta":
            continue
        if index < 6:
            raise ValueError("UnitObjectMeta constructor has no enclosing PR:HashMap_put")
        prefix = tokens[index - 6 : index]
        if not (
            prefix[0].text == "PR"
            and prefix[1].text == ":"
            and prefix[2].text == "HashMap_put"
            and prefix[3].text == "("
            and prefix[4].kind == "number"
            and prefix[4].integer_value is not None
            and prefix[5].text == ","
        ):
            raise ValueError(
                f"UnitObjectMeta constructor at byte {function_start + token.start} is not directly stored in PR"
            )
        building_id = int(prefix[4].integer_value)
        args, next_index = _call_arguments(tokens, index)
        if len(args) != 16:
            raise ValueError(f"UnitObjectMeta constructor must have 16 arguments, got {len(args)}")
        if any(args[arg_index] for arg_index in range(1, 6)):
            raise ValueError("UnitObjectMeta presentation-string arguments unexpectedly produced lexical tokens")
        if next_index >= len(tokens) or tokens[next_index].text != ")":
            raise ValueError("UnitObjectMeta constructor is not the second argument of PR:HashMap_put")

        unit_id = _integer_literal_value(args[0])
        gold_cost = _integer_literal_value(args[6])
        lumber_cost = _integer_literal_value(args[7])
        food_used = _integer_literal_value(args[8])
        spawn_build_time = _integer_literal_value(args[9])
        attack_index = _integer_literal_value(args[10])
        defense_index = _integer_literal_value(args[11])
        is_air = _boolean_literal_value(args[12])
        is_melee = _boolean_literal_value(args[13])
        is_mechanical = _boolean_literal_value(args[14])
        is_caster = _boolean_literal_value(args[15])

        if building_id in seen_buildings:
            raise ValueError(f"duplicate UnitObjectMeta building ID: {building_id}")
        seen_buildings.add(building_id)
        rows.append({
            "building_id": building_id,
            "unit_id": unit_id,
            "gold_cost": gold_cost,
            "lumber_cost": lumber_cost,
            "food_used": food_used,
            "spawn_build_time": spawn_build_time,
            "attack_index": attack_index,
            "defense_index": defense_index,
            "is_air": is_air,
            "is_melee": is_melee,
            "is_mechanical": is_mechanical,
            "is_caster": is_caster,
            "byte_offset": function_start + token.start,
            "source_function": function_name,
        })

    # Mirror unitObjectMetaMix so downstream validation has a stable source
    # fingerprint independent of the obfuscated presentation strings.
    modulus = 1_000_003
    fingerprint = 41

    def wurst_mod(value: int, divisor: int) -> int:
        return value % divisor if value >= 0 else -(abs(value) % divisor)

    def mix(accumulator: int, value: int) -> int:
        encoded = wurst_mod(value, modulus)
        if encoded < 0:
            encoded = -encoded + 17
        return wurst_mod(accumulator * 131 + encoded + 17, modulus)

    for row in rows:
        for value in (
            int(row["building_id"]), int(row["unit_id"]), int(row["gold_cost"]), int(row["lumber_cost"]),
            int(row["food_used"]), int(row["spawn_build_time"]), int(row["attack_index"]), int(row["defense_index"]),
            int(bool(row["is_air"])), int(bool(row["is_melee"])), int(bool(row["is_mechanical"])), int(bool(row["is_caster"])),
        ):
            fingerprint = mix(fingerprint, value)

    return rows, fingerprint


def _extract_unit_object_upgrades(
    data: bytes,
    functions: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Recover authored building upgrade edges from the generated metadata table."""
    function_name = "ensureUnitObjectUpgradeMetadataRegistered"
    body = _function_body_tokens(data, functions, function_name)
    if body is None:
        return []
    function_start, tokens = body
    rows: list[dict[str, object]] = []
    seen: set[tuple[int, int]] = set()
    index = 0
    while index + 19 < len(tokens):
        if not (
            tokens[index].text == "IR"
            and tokens[index + 1].text == "["
            and tokens[index + 2].text == "ER"
            and tokens[index + 3].text == "]"
            and tokens[index + 4].text == "="
            and tokens[index + 5].kind == "number"
            and tokens[index + 5].integer_value is not None
            and tokens[index + 6].text == "HR"
            and tokens[index + 7].text == "["
            and tokens[index + 8].text == "ER"
            and tokens[index + 9].text == "]"
            and tokens[index + 10].text == "="
            and tokens[index + 11].kind == "number"
            and tokens[index + 11].integer_value is not None
            and tokens[index + 12].text == "ER"
            and tokens[index + 13].text == "="
            and tokens[index + 14].text == "("
            and tokens[index + 15].text == "ER"
            and tokens[index + 16].text == "+"
            and tokens[index + 17].kind == "number"
            and tokens[index + 17].integer_value == 1
            and tokens[index + 18].text == ")"
        ):
            index += 1
            continue
        source_id = int(tokens[index + 5].integer_value)
        target_id = int(tokens[index + 11].integer_value)
        edge = (source_id, target_id)
        if edge in seen:
            raise ValueError(f"duplicate authored building upgrade edge: {edge}")
        seen.add(edge)
        rows.append({
            "source_building_id": source_id,
            "target_building_id": target_id,
            "source_function": function_name,
            "byte_offset": function_start + tokens[index].start,
        })
        index += 19
    return rows


def _extract_race_buildings(
    data: bytes,
    functions: list[dict[str, object]],
    call_edges: Counter[tuple[str, str]],
) -> list[dict[str, object]]:
    """Recover race→building membership from generated CFRace initializers."""
    registrar = "CFRace_CFRace_registerBuildings__w3p_vmProtect"
    callers = {
        caller
        for (caller, callee), count in call_edges.items()
        if callee == registrar and count > 0 and caller != "<top-level>"
    }
    function_order = {str(row["name"]): int(row["start"]) for row in functions}
    rows: list[dict[str, object]] = []
    seen_buildings: set[int] = set()

    for race_index, function_name in enumerate(sorted(callers, key=lambda name: function_order.get(name, 1 << 62))):
        body = _function_body_tokens(data, functions, function_name)
        if body is None:
            raise ValueError(f"race registrar caller has no function body: {function_name}")
        function_start, tokens = body
        builder_id: int | None = None
        campaign_only = any(
            token.kind == "ident" and token.text == "CFRace_CFRace_markCampaignOnly"
            for token in tokens
        )

        for index in range(len(tokens) - 4):
            if (
                tokens[index].kind == "ident"
                and tokens[index + 1].text == "."
                and tokens[index + 2].text == "CFRace_builderId"
                and tokens[index + 3].text == "="
                and tokens[index + 4].kind == "number"
                and tokens[index + 4].integer_value is not None
                and int(tokens[index + 4].integer_value) > 0
            ):
                builder_id = int(tokens[index + 4].integer_value)

        if builder_id is None:
            raise ValueError(f"race initializer {function_name} has no positive builder rawcode")

        building_order = 0
        for index, token in enumerate(tokens):
            if token.kind != "ident" or token.text != "_I":
                continue
            args, _next = _wurst_registry_call_arguments(tokens, index)
            if len(args) not in {1, 2}:
                continue
            try:
                building_id = _integer_literal_value(args[0])
                unit_id = _integer_literal_value(args[1]) if len(args) == 2 else 0
            except ValueError:
                continue
            if building_id <= 0:
                continue
            if building_id in seen_buildings:
                raise ValueError(f"building {building_id} appears in multiple generated race catalogs")
            seen_buildings.add(building_id)
            rows.append({
                "race_index": race_index,
                "race_function": function_name,
                "builder_id": builder_id,
                "campaign_only": campaign_only,
                "building_order": building_order,
                "building_id": building_id,
                "unit_id": unit_id,
                "byte_offset": function_start + token.start,
            })
            building_order += 1

    return rows


def _extract_income_factor_constants(
    data: bytes,
    functions: list[dict[str, object]],
) -> dict[str, str]:
    """Recover the five CFBuilding income-factor constants initialized by LE."""
    body = _function_body_tokens(data, functions, "LE")
    if body is None:
        return {}
    _function_start, tokens = body
    expected = {"gvb", "fvb", "evb", "dvb", "cvb"}
    values: dict[str, str] = {}
    for index in range(len(tokens) - 2):
        token = tokens[index]
        if token.kind != "ident" or token.text not in expected or tokens[index + 1].text != "=":
            continue
        if token.text in values:
            continue
        value = _decimal_literal_value([tokens[index + 2]])
        values[token.text] = _decimal_text(value)
    if values and set(values) != expected:
        raise ValueError(f"incomplete CFBuilding income-factor constants: {sorted(values)}")
    return values


def _building_id_from_expression(tokens: list[LuaToken]) -> int | None:
    found: set[int] = set()
    for index, token in enumerate(tokens):
        if token.kind != "ident" or token.text != "_I":
            continue
        try:
            args, _next = _wurst_registry_call_arguments(tokens, index)
        except ValueError:
            continue
        if len(args) not in {1, 2}:
            continue
        try:
            rawcode = _integer_literal_value(args[0])
        except ValueError:
            continue
        if rawcode > 0:
            found.add(rawcode)
    if len(found) > 1:
        raise ValueError(f"building expression contains multiple registry rawcodes: {sorted(found)}")
    return next(iter(found), None)


def _extract_race_building_semantics(
    data: bytes,
    functions: list[dict[str, object]],
    race_buildings: list[dict[str, object]],
    income_factors: dict[str, str],
) -> list[dict[str, object]]:
    """Recover CFBuilding wrapper semantics attached by generated race initializers."""
    if not race_buildings or not income_factors:
        return []

    rows_by_function: dict[str, list[dict[str, object]]] = defaultdict(list)
    for row in race_buildings:
        rows_by_function[str(row["race_function"])].append(row)

    flag_wrappers = {
        "CFBuilding_CFBuilding_isLegendaryLine": "is_legendary_line",
        "CFBuilding_CFBuilding_isAntiAir": "is_anti_air",
        "CFBuilding_CFBuilding_isSiege": "is_siege",
        "CFBuilding_CFBuilding_isArtillery": "is_artillery",
        "CFBuilding_CFBuilding_isNAOnly": "is_na_only",
        "CFBuilding_CFBuilding_isUltimateOnly": "is_ultimate_only",
        "CFBuilding_CFBuilding_noPP": "no_pp",
        "CFBuilding_CFBuilding_isAiShouldIgnore": "ai_should_ignore",
        "CFBuilding_CFBuilding_providesActiveTargetedSpellShield": "provides_active_targeted_spell_shield",
        "CFBuilding_CFBuilding_areaSpell": "area_spell",
    }
    value_wrappers = {
        "CFBuilding_CFBuilding_multiTarget": "multi_target_mult",
        "CFBuilding_CFBuilding_cagePressure": "cage_pressure",
        "CFBuilding_CFBuilding_placementStrat": "placement_strat",
        "CFBuilding_CFBuilding_spellDps": "spell_dps",
        "CFBuilding_CFBuilding_aiTowerStrength": "ai_tower_strength",
        "CFBuilding_CFBuilding_combatPowerFactor": "combat_power_factor",
    }
    tag_wrappers = {
        "CFBuilding_CFBuilding_tags": "tags",
        "CFBuilding_CFBuilding_extraTags": "extra_tags",
        "CFBuilding_CFBuilding_overrideTags": "override_tags",
    }

    output: list[dict[str, object]] = []
    for function_name, race_rows in rows_by_function.items():
        body = _function_body_tokens(data, functions, function_name)
        if body is None:
            raise ValueError(f"race semantics source function is missing: {function_name}")
        function_start, tokens = body
        known_buildings = {int(row["building_id"]) for row in race_rows}
        variable_buildings: dict[str, int] = {}

        # Race code assigns each constructed CFBuilding to a local variable.
        # Resolve those aliases first so later standalone wrappers such as
        # extraTags(building, ...) can still be tied back to an exact rawcode.
        for index in range(len(tokens) - 2):
            if tokens[index].kind != "ident" or tokens[index + 1].text != "=":
                continue
            expression_start = index + 2
            try:
                if tokens[expression_start].kind == "ident" and tokens[expression_start].text == "_I":
                    _args, expression_end = _wurst_registry_call_arguments(tokens, expression_start)
                elif (
                    tokens[expression_start].kind == "ident"
                    and expression_start + 1 < len(tokens)
                    and tokens[expression_start + 1].text == "("
                ):
                    _args, expression_end = _call_arguments(tokens, expression_start)
                else:
                    continue
            except ValueError:
                continue
            building_id = _building_id_from_expression(tokens[expression_start:expression_end])
            if building_id is not None and building_id in known_buildings:
                variable_buildings[tokens[index].text] = building_id

        semantics: dict[int, dict[str, object]] = {
            building_id: {
                "building_id": building_id,
                "income_factor_symbol": None,
                "income_factor": None,
                "precursor_building_id": None,
                "has_tier_assignment": False,
                "is_legendary_line": False,
                "is_anti_air": False,
                "is_siege": False,
                "is_artillery": False,
                "is_na_only": False,
                "is_ultimate_only": False,
                "no_pp": False,
                "ai_should_ignore": False,
                "provides_active_targeted_spell_shield": False,
                "area_spell": False,
                "multi_target_mult": None,
                "cage_pressure": None,
                "placement_strat": None,
                "spell_dps": None,
                "ai_tower_strength": None,
                "combat_power_factor": None,
                "tags": [],
                "extra_tags": [],
                "override_tags": [],
                "source_function": function_name,
                "first_wrapper_byte_offset": None,
            }
            for building_id in known_buildings
        }

        def building_for_argument(argument: list[LuaToken]) -> int | None:
            building_id = _building_id_from_expression(argument)
            if building_id is not None:
                return building_id
            if len(argument) == 1 and argument[0].kind == "ident":
                return variable_buildings.get(argument[0].text)
            return None

        def set_once(record: dict[str, object], field: str, value: object) -> None:
            prior = record[field]
            if prior is not None and prior != value:
                raise ValueError(
                    f"conflicting {field} wrappers for building {record['building_id']}: {prior!r} != {value!r}"
                )
            record[field] = value

        wrappers = set(flag_wrappers) | set(value_wrappers) | set(tag_wrappers) | {
            "CFBuilding_CFBuilding_incomeFactor",
            "CFBuilding_CFBuilding_precursor",
            "CFBuilding_CFBuilding_tier",
        }
        for index, token in enumerate(tokens):
            if token.kind != "ident" or token.text not in wrappers:
                continue
            args, _next = _call_arguments(tokens, index)
            if not args:
                raise ValueError(f"{token.text} in {function_name} has no building argument")
            building_id = building_for_argument(args[0])
            if building_id is None or building_id not in semantics:
                continue
            record = semantics[building_id]
            if record["first_wrapper_byte_offset"] is None:
                record["first_wrapper_byte_offset"] = function_start + token.start

            if token.text == "CFBuilding_CFBuilding_incomeFactor":
                if len(args) != 2 or len(args[1]) != 1 or args[1][0].kind != "ident":
                    raise ValueError(f"incomeFactor wrapper has unexpected argument shape in {function_name}")
                symbol = args[1][0].text
                if symbol not in income_factors:
                    raise ValueError(f"unknown CFBuilding income factor {symbol!r} in {function_name}")
                set_once(record, "income_factor_symbol", symbol)
                set_once(record, "income_factor", income_factors[symbol])
            elif token.text == "CFBuilding_CFBuilding_precursor":
                if len(args) != 2:
                    raise ValueError(f"precursor wrapper has unexpected argument count in {function_name}")
                parent = building_for_argument(args[1])
                if parent is None:
                    raise ValueError(f"precursor wrapper cannot resolve parent building in {function_name}")
                set_once(record, "precursor_building_id", parent)
            elif token.text == "CFBuilding_CFBuilding_tier":
                record["has_tier_assignment"] = True
            elif token.text in flag_wrappers:
                record[flag_wrappers[token.text]] = True
            elif token.text in value_wrappers:
                if len(args) != 2:
                    raise ValueError(f"{token.text} has unexpected argument count in {function_name}")
                value = _decimal_text(_decimal_literal_value(args[1]))
                set_once(record, value_wrappers[token.text], value)
            elif token.text in tag_wrappers:
                values = [_integer_literal_value(argument) for argument in args[1:]]
                field = tag_wrappers[token.text]
                existing = list(record[field])
                if token.text == "CFBuilding_CFBuilding_overrideTags":
                    existing = []
                for value in values:
                    if value not in existing:
                        existing.append(value)
                record[field] = existing

        for building_id, record in semantics.items():
            if record["income_factor"] is None:
                raise ValueError(f"race building {building_id} has no incomeFactor wrapper in {function_name}")
            for field in ("tags", "extra_tags", "override_tags"):
                record[field] = tuple(record[field])
            output.append(record)

    output.sort(key=lambda row: int(row["building_id"]))
    return output


def _extract_element_building_buckets(
    data: bytes,
    functions: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Recover the Elemental race building-count buckets consumed by Master of Elements.

    The generated Elemental initializer stores a compact per-building bucket in
    ``vtb[buildingTypeIndex(rawcode)]``. Master of Elements later indexes the
    corresponding per-player counters in ``wtb``. Keeping this mapping as
    source evidence avoids exposing anonymous numeric bucket indexes to the
    native importer.
    """
    body = _function_body_tokens(data, functions, "wK")
    if body is None:
        return []
    function_start, tokens = body
    rows: list[dict[str, object]] = []
    seen_buildings: set[int] = set()
    index = 0
    while index + 8 < len(tokens):
        if not (
            tokens[index].kind == "ident"
            and tokens[index].text == "vtb"
            and tokens[index + 1].text == "["
            and tokens[index + 2].kind == "ident"
            and tokens[index + 2].text == "buildingTypeIndex"
            and tokens[index + 3].text == "("
            and tokens[index + 4].kind == "number"
            and tokens[index + 4].integer_value is not None
            and tokens[index + 5].text == ")"
            and tokens[index + 6].text == "]"
            and tokens[index + 7].text == "="
            and tokens[index + 8].kind == "number"
            and tokens[index + 8].integer_value is not None
        ):
            index += 1
            continue
        building_id = int(tokens[index + 4].integer_value)
        bucket = int(tokens[index + 8].integer_value)
        if building_id in seen_buildings:
            raise ValueError(f"duplicate Elemental building bucket assignment for {building_id}")
        if bucket < 1 or bucket > 5:
            raise ValueError(f"unexpected Elemental building bucket {bucket} for {building_id}")
        seen_buildings.add(building_id)
        rows.append({
            "building_id": building_id,
            "bucket": bucket,
            "source_function": "wK",
            "byte_offset": function_start + tokens[index].start,
        })
        index += 9

    if rows:
        counts = Counter(int(row["bucket"]) for row in rows)
        if len(rows) != 12 or counts != Counter({1: 2, 2: 2, 3: 2, 4: 3, 5: 3}):
            raise ValueError(f"Elemental building bucket structure changed: rows={len(rows)} counts={dict(counts)}")
    rows.sort(key=lambda row: (int(row["bucket"]), int(row["building_id"])))
    return rows


def _extract_effective_unit_stats(
    data: bytes,
    functions: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Recover Wurst's generated building→spawned-unit effective stat catalog."""
    body = _function_body_tokens(data, functions, "xO")
    if body is None:
        return []
    function_start, tokens = body
    rows: list[dict[str, object]] = []
    seen_buildings: set[int] = set()
    seen_units: set[int] = set()
    index = 0

    def assignment_prefix(at: int, variable: str, field: str) -> int:
        _expect_token(tokens, at, variable)
        _expect_token(tokens, at + 1, ".")
        _expect_token(tokens, at + 2, field)
        _expect_token(tokens, at + 3, "=")
        return at + 4

    while index + 6 < len(tokens):
        token = tokens[index]
        if not (
            token.kind == "ident"
            and tokens[index + 1].text == "="
            and tokens[index + 2].text == "PB"
            and tokens[index + 3].text == ":"
            and tokens[index + 4].text == "create1139"
            and tokens[index + 5].text == "("
            and tokens[index + 6].text == ")"
        ):
            index += 1
            continue

        variable = token.text
        cursor = index + 7

        cursor = assignment_prefix(cursor, variable, "UnitEffectiveStat_unitId")
        unit_token = tokens[cursor]
        if unit_token.kind != "number" or unit_token.integer_value is None:
            raise ValueError("effective unit stat unitId must be an integer literal")
        unit_id = int(unit_token.integer_value)
        cursor += 1

        cursor = assignment_prefix(cursor, variable, "UnitEffectiveStat_hp")
        hp, cursor = _parse_int_to_real(tokens, cursor)

        cursor = assignment_prefix(cursor, variable, "UnitEffectiveStat_armor")
        armor, cursor = _parse_scaled_int_to_real(tokens, cursor)

        cursor = assignment_prefix(cursor, variable, "UnitEffectiveStat_dps")
        dps, cursor = _parse_scaled_int_to_real(tokens, cursor)

        cursor = assignment_prefix(cursor, variable, "UnitEffectiveStat_attackRange")
        attack_range, cursor = _parse_int_to_real(tokens, cursor)

        cursor = assignment_prefix(cursor, variable, "UnitEffectiveStat_moveSpeed")
        move_speed, cursor = _parse_int_to_real(tokens, cursor)

        cursor = assignment_prefix(cursor, variable, "UnitEffectiveStat_spawnsPerCycle")
        spawn_token = tokens[cursor]
        if spawn_token.kind != "number" or spawn_token.integer_value is None:
            raise ValueError("effective unit stat spawnsPerCycle must be an integer literal")
        spawns_per_cycle = int(spawn_token.integer_value)
        cursor += 1

        cursor = assignment_prefix(cursor, variable, "UnitEffectiveStat_canHitAir")
        air_token = tokens[cursor]
        if air_token.kind != "ident" or air_token.text not in {"true", "false"}:
            raise ValueError("effective unit stat canHitAir must be a boolean literal")
        can_hit_air = air_token.text == "true"
        cursor += 1

        _expect_token(tokens, cursor, "ZR")
        _expect_token(tokens, cursor + 1, ":")
        _expect_token(tokens, cursor + 2, "HashMap_put")
        _expect_token(tokens, cursor + 3, "(")
        building_token = tokens[cursor + 4]
        if building_token.kind != "number" or building_token.integer_value is None:
            raise ValueError("effective unit stat building ID must be an integer literal")
        building_id = int(building_token.integer_value)
        _expect_token(tokens, cursor + 5, ",")
        _expect_token(tokens, cursor + 6, variable)
        _expect_token(tokens, cursor + 7, ")")
        cursor += 8

        if building_id in seen_buildings:
            raise ValueError(f"duplicate effective stat building ID in xO: {building_id}")
        if unit_id in seen_units:
            raise ValueError(f"duplicate effective stat unit ID in xO: {unit_id}")
        seen_buildings.add(building_id)
        seen_units.add(unit_id)
        rows.append({
            "building_id": building_id,
            "unit_id": unit_id,
            "hp": _decimal_text(hp),
            "armor": _decimal_text(armor),
            "dps": _decimal_text(dps),
            "attack_range": _decimal_text(attack_range),
            "move_speed": _decimal_text(move_speed),
            "spawns_per_cycle": spawns_per_cycle,
            "can_hit_air": can_hit_air,
            "byte_offset": function_start + token.start,
            "source_function": "xO",
        })
        index = cursor

    return rows


UNIT_STAT_SENTINEL = 2_147_483_647
UNIT_STAT_FIELDS = (
    "hp",
    "armor",
    "defense_type",
    "move_speed",
    "attack1_base_damage",
    "attack1_dice_number",
    "attack1_dice_sides",
    "attack1_cooldown_microseconds",
    "attack1_range",
    "attack2_base_damage",
    "attack2_dice_number",
    "attack2_dice_sides",
    "attack2_cooldown_microseconds",
    "attack2_range",
)


def _unit_stat_decode_key(unit_id: int, field_index: int) -> int:
    """Mirror the visible Wurst cP() key function used by dP()."""
    return ((unit_id // (field_index + 1) + field_index * 977 + 7331) % 4093) + 101


def _extract_protected_unit_stats(
    data: bytes,
    functions: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Recover the protected UnitStat source rows encoded in jP.

    The generated jP initializer calls one protected row-loader through `_I`
    with sixteen integer arguments: unit rawcode, source fingerprint, then the
    fourteen UnitStat fields consumed by UnitStat_applyTo. The visible cP/dP
    helpers define the exact per-unit/per-column encoding. 2147483647 is the
    encoded "no override" sentinel; cooldown overrides are stored in integer
    microseconds and converted by the protected loader before application.
    """
    body = _function_body_tokens(data, functions, "jP")
    if body is None:
        return []
    function_start, tokens = body
    rows: list[dict[str, object]] = []
    seen_units: set[int] = set()
    index = 0

    while index < len(tokens):
        token = tokens[index]
        if token.kind != "ident" or token.text != "_I":
            index += 1
            continue
        if index + 1 >= len(tokens) or tokens[index + 1].text != "[":
            raise ValueError(f"UnitStat jP registry call missing [ at byte {function_start + token.start}")

        depth = 0
        cursor = index + 1
        closing = None
        while cursor < len(tokens):
            current = tokens[cursor]
            if current.text == "[":
                depth += 1
            elif current.text == "]":
                depth -= 1
                if depth == 0:
                    closing = cursor
                    break
            cursor += 1
        if closing is None:
            raise ValueError(f"UnitStat jP registry lookup is unterminated at byte {function_start + token.start}")
        opening = closing + 1
        if opening >= len(tokens) or tokens[opening].text != "(":
            raise ValueError(f"UnitStat jP registry lookup is not called at byte {function_start + token.start}")
        args, next_index = _parenthesized_arguments(tokens, opening)
        if len(args) != 16:
            raise ValueError(f"UnitStat jP row must contain 16 integer arguments, got {len(args)}")

        integers: list[int] = []
        for arg in args:
            if len(arg) != 1 or arg[0].kind != "number" or arg[0].integer_value is None:
                raise ValueError(f"UnitStat jP row contains non-integer argument near byte {function_start + token.start}")
            integers.append(int(arg[0].integer_value))

        unit_id = integers[0]
        fingerprint = integers[1]
        if unit_id <= 0:
            raise ValueError(f"UnitStat jP row has invalid unit ID {unit_id}")
        if unit_id in seen_units:
            raise ValueError(f"duplicate UnitStat jP row for unit ID {unit_id}")
        seen_units.add(unit_id)

        encoded_values = integers[2:]
        decoded: dict[str, int | None] = {}
        for field_index, (field, encoded) in enumerate(zip(UNIT_STAT_FIELDS, encoded_values, strict=True)):
            decoded[field] = (
                None
                if encoded == UNIT_STAT_SENTINEL
                else encoded - _unit_stat_decode_key(unit_id, field_index)
            )

        row: dict[str, object] = {
            "unit_id": unit_id,
            "source_fingerprint": fingerprint,
            "encoded_values": encoded_values,
            "byte_offset": function_start + token.start,
            "source_function": "jP",
        }
        row.update(decoded)
        for attack in (1, 2):
            micros = decoded[f"attack{attack}_cooldown_microseconds"]
            row[f"attack{attack}_cooldown"] = (
                None
                if micros is None
                else _decimal_text(Decimal(micros) / Decimal(1_000_000))
            )
        rows.append(row)
        index = next_index

    return rows


def _rawcode_mutator_traces(
    functions: list[dict[str, object]],
    call_edges: Counter[tuple[str, str]],
    function_rawcodes: Counter[tuple[str, int]],
    runtime_mutators: list[dict[str, object]],
) -> tuple[list[dict[str, object]], int]:
    """Find deterministic shortest lexical call paths from rawcodes to mutators.

    These are static reachability traces, not data-flow proofs. A rawcode and a
    mutator connected by one of these paths can participate in the same direct
    call chain, but the scan does not prove that the rawcode is passed to that
    mutator, that the relevant branches execute, or that an indirect function
    value/callback target has been resolved.
    """
    defined_functions = {str(function["name"]) for function in functions}
    adjacency: dict[str, set[str]] = defaultdict(set)
    resolved_edges = 0
    for (caller, callee), _count in call_edges.items():
        # The caller may be top-level or an anonymous function, both of which
        # can still provide useful source context. The callee must resolve to a
        # named function definition before we can follow it safely.
        if callee in defined_functions:
            adjacency[caller].add(callee)
            resolved_edges += 1

    rawcodes_by_function: dict[str, dict[int, int]] = defaultdict(dict)
    for (function, rawcode), count in function_rawcodes.items():
        rawcodes_by_function[function][rawcode] = count

    mutators_by_function: dict[str, list[dict[str, object]]] = defaultdict(list)
    for site in runtime_mutators:
        mutators_by_function[str(site["function"])].append(site)
    mutator_functions = set(mutators_by_function)

    traces: list[dict[str, object]] = []
    for source_function in sorted(rawcodes_by_function):
        # BFS gives minimum hop count. Sorted neighbors make the one retained
        # shortest path stable when several equally short paths exist.
        queue: deque[str] = deque([source_function])
        paths: dict[str, tuple[str, ...]] = {source_function: (source_function,)}
        remaining_targets = set(mutator_functions)
        while queue and remaining_targets:
            current = queue.popleft()
            if current in remaining_targets:
                remaining_targets.remove(current)
            for callee in sorted(adjacency.get(current, ())):
                if callee in paths:
                    continue
                paths[callee] = (*paths[current], callee)
                queue.append(callee)

        for mutation_function in sorted(mutator_functions.intersection(paths)):
            call_path = paths[mutation_function]
            hops = len(call_path) - 1
            for rawcode, reference_count in sorted(rawcodes_by_function[source_function].items()):
                for site in mutators_by_function[mutation_function]:
                    traces.append({
                        "rawcode_integer": rawcode,
                        "source_function": source_function,
                        "source_reference_count": reference_count,
                        "mutation_function": mutation_function,
                        "mutator_callee": site["callee"],
                        "normalized_mutator": site["normalized_callee"],
                        "mutator_byte_offset": site["byte_offset"],
                        "hop_count": hops,
                        "call_path": call_path,
                        "evidence_kind": "direct-same-function" if hops == 0 else "static-call-path",
                    })

    traces.sort(key=lambda trace: (
        int(trace["rawcode_integer"]),
        int(trace["hop_count"]),
        str(trace["source_function"]),
        int(trace["mutator_byte_offset"]),
        tuple(trace["call_path"]),
    ))
    return traces, resolved_edges


def _function_value_links(
    data: bytes,
    functions: list[dict[str, object]],
) -> tuple[list[dict[str, object]], list[dict[str, object]]]:
    """Recover exact named-function aliases and function-valued call arguments.

    Wurst emits large prototype/dispatch tables such as
    `Class.someEvent=Concrete_handler`. These assignments are strong lexical
    evidence that the generated slot points at that function, but they are not
    direct call edges: later virtual dispatch may select the slot through a
    different receiver expression. Bare named functions passed as call
    arguments are recorded separately for the same reason.
    """
    defined = {str(function["name"]) for function in functions}
    reserved = {
        "and", "break", "do", "else", "elseif", "end", "false", "for", "function",
        "goto", "if", "in", "local", "nil", "not", "or", "repeat", "return", "then",
        "true", "until", "while",
    }
    stream = TokenStream(iter_lua_tokens(data))
    parens: list[str | None] = []
    pending_call: str | None = None
    aliases: list[dict[str, object]] = []
    value_arguments: list[dict[str, object]] = []

    def identifier_chain(first: LuaToken) -> str:
        parts = [first.text]
        while True:
            separator = stream.peek()
            member = stream.peek(1)
            if (
                separator is None
                or member is None
                or separator.kind != "symbol"
                or separator.text not in (".", ":")
                or member.kind != "ident"
            ):
                break
            parts.append(stream.pop().text)
            parts.append(stream.pop().text)
        return "".join(parts)

    while (token := stream.pop()) is not None:
        if token.kind == "ident":
            if token.text == "function":
                declared = stream.peek()
                if declared is not None and declared.kind == "ident":
                    identifier_chain(stream.pop())
                opening = stream.peek()
                if opening is not None and opening.kind == "symbol" and opening.text == "(":
                    stream.pop()
                    parens.append(None)
                pending_call = None
                continue
            if token.text in reserved:
                pending_call = None
                continue

            name = identifier_chain(token)
            opening = stream.peek()
            if opening is not None and opening.kind == "symbol" and opening.text == "(":
                pending_call = name
                continue

            containing_call = next((call for call in reversed(parens) if call is not None), "")
            if containing_call and name in defined:
                value_arguments.append({
                    "target_function": name,
                    "byte_offset": token.start,
                    "containing_call": containing_call,
                })

            if opening is not None and opening.kind == "symbol" and opening.text == "=":
                stream.pop()
                rhs = stream.peek()
                if rhs is not None and rhs.kind == "ident" and rhs.text not in reserved:
                    rhs = stream.pop()
                    target = identifier_chain(rhs)
                    after_target = stream.peek()
                    if target in defined and not (
                        after_target is not None
                        and after_target.kind == "symbol"
                        and after_target.text == "("
                    ):
                        aliases.append({
                            "alias": name,
                            "target_function": target,
                            "alias_byte_offset": token.start,
                            "target_byte_offset": rhs.start,
                        })
                    if (
                        after_target is not None
                        and after_target.kind == "symbol"
                        and after_target.text == "("
                    ):
                        pending_call = target
                    else:
                        pending_call = None
                else:
                    pending_call = None
                continue

            pending_call = None
            continue

        if token.kind == "symbol":
            if token.text == "(":
                parens.append(pending_call)
            elif token.text == ")":
                if not parens:
                    raise ValueError(f"unexpected Lua ) while indexing function values at byte {token.start}")
                parens.pop()
            pending_call = None
        else:
            pending_call = None

    if parens:
        raise ValueError("unterminated Lua parenthesis stack while indexing function values")

    aliases.sort(key=lambda alias: (int(alias["alias_byte_offset"]), str(alias["alias"])))
    value_arguments.sort(key=lambda value: int(value["byte_offset"]))
    return aliases, value_arguments


def _extract_building_spell_registrations(
    data: bytes,
    functions: list[dict[str, object]],
    function_aliases: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Recover generated building-spell registration tuples and concrete handlers.

    Wurst allocates a closure object (`localVar=Class:createNNN()`) and passes it
    to a protected registry call `_I[...](buildingRawcode, abilityRawcode,
    localVar)`. The class prototype table separately assigns
    `Class.BuildingSpellClosure_cast=Concrete_handler`. Joining those two exact
    lexical facts recovers a building/ability/handler registration without
    executing the map or guessing virtual dispatch.
    """
    handler_by_class: dict[str, str] = {}
    for alias in function_aliases:
        alias_name = str(alias["alias"])
        suffix = ".BuildingSpellClosure_cast"
        if not alias_name.endswith(suffix):
            continue
        class_name = alias_name[: -len(suffix)]
        target = str(alias["target_function"])
        prior = handler_by_class.get(class_name)
        if prior is not None and prior != target:
            raise ValueError(f"conflicting BuildingSpellClosure_cast handlers for {class_name}: {prior} != {target}")
        handler_by_class[class_name] = target
    if not handler_by_class:
        return []

    rows: list[dict[str, object]] = []
    for function in functions:
        function_name = str(function["name"])
        body = _function_body_tokens(data, functions, function_name)
        if body is None:
            continue
        function_start, tokens = body
        variable_classes: dict[str, str] = {}
        integer_variables: dict[str, int] = {}

        # Resolve simple generated integer aliases in source order. Wurst often
        # hoists a rawcode into one variable and copies it into a local before
        # the registry call (for example `XQ=1093683278; zis=XQ`).
        for index in range(len(tokens) - 2):
            if tokens[index].kind != "ident" or tokens[index + 1].text != "=":
                continue
            rhs = tokens[index + 2]
            if rhs.kind == "number" and rhs.integer_value is not None:
                integer_variables[tokens[index].text] = int(rhs.integer_value)
            elif rhs.kind == "ident" and rhs.text in integer_variables:
                integer_variables[tokens[index].text] = integer_variables[rhs.text]

        for index in range(len(tokens) - 6):
            if (
                tokens[index].kind == "ident"
                and tokens[index + 1].text == "="
                and tokens[index + 2].kind == "ident"
                and tokens[index + 2].text in handler_by_class
                and tokens[index + 3].text == ":"
                and tokens[index + 4].kind == "ident"
                and tokens[index + 4].text.startswith("create")
                and tokens[index + 5].text == "("
            ):
                variable_classes[tokens[index].text] = tokens[index + 2].text
        if not variable_classes:
            continue

        def integer_argument(argument: list[LuaToken]) -> int | None:
            try:
                return _integer_literal_value(argument)
            except ValueError:
                pass
            if len(argument) == 1 and argument[0].kind == "ident":
                return integer_variables.get(argument[0].text)
            return None

        registered_closures: set[str] = set()
        for index, token in enumerate(tokens):
            if token.kind != "ident" or token.text != "_I":
                continue
            try:
                args, _next = _wurst_registry_call_arguments(tokens, index)
            except ValueError:
                continue
            if len(args) != 3 or len(args[2]) != 1 or args[2][0].kind != "ident":
                continue
            closure_variable = args[2][0].text
            closure_class = variable_classes.get(closure_variable)
            if closure_class is None:
                continue
            building_id = integer_argument(args[0])
            ability_id = integer_argument(args[1])
            if building_id is None or ability_id is None:
                continue
            if building_id <= 0 or ability_id <= 0:
                continue
            rows.append({
                "building_id": building_id,
                "ability_id": ability_id,
                "closure_variable": closure_variable,
                "closure_class": closure_class,
                "handler_function": handler_by_class[closure_class],
                "registration_function": function_name,
                "evidence_kind": "protected-registry-call",
                "byte_offset": function_start + token.start,
            })
            registered_closures.add(closure_variable)

        # Some generated building spells bypass the protected three-argument
        # registry and construct the spell-effect EventListener explicitly. The
        # listener stores the same exact tuple in visible fields: unitTypeId,
        # abilId and a BuildingSpellClosure callback. Require an exact
        # EVENT_PLAYER_UNIT_SPELL_EFFECT EventListener_add site before promoting
        # this second representation to a registration.
        listener_unit_ids: dict[str, int] = {}
        listener_ability_ids: dict[str, int] = {}
        listener_closures: dict[str, str] = {}
        spell_effect_event_variables: set[str] = {"EVENT_PLAYER_UNIT_SPELL_EFFECT"}
        index = 0
        while index < len(tokens):
            token = tokens[index]
            if token.kind == "ident" and index + 2 < len(tokens) and tokens[index + 1].text == "=":
                rhs = tokens[index + 2]
                if rhs.kind == "ident" and rhs.text in spell_effect_event_variables:
                    spell_effect_event_variables.add(token.text)

            if (
                token.kind == "ident"
                and index + 4 < len(tokens)
                and tokens[index + 1].text == "."
                and tokens[index + 2].kind == "ident"
                and tokens[index + 3].text == "="
            ):
                listener = token.text
                member = tokens[index + 2].text
                rhs = tokens[index + 4]
                if member in {"unitTypeId", "abilId"}:
                    value = integer_argument([rhs])
                    if value is not None:
                        target = listener_unit_ids if member == "unitTypeId" else listener_ability_ids
                        target[listener] = value
                elif member == "cb" and rhs.kind == "ident" and rhs.text in variable_classes:
                    listener_closures[listener] = rhs.text

            if token.kind == "ident" and token.text == "EventListener_add":
                try:
                    args, next_index = _call_arguments(tokens, index)
                except ValueError:
                    index += 1
                    continue
                if (
                    len(args) == 2
                    and len(args[0]) == 1
                    and args[0][0].kind == "ident"
                    and args[0][0].text in spell_effect_event_variables
                    and len(args[1]) == 1
                    and args[1][0].kind == "ident"
                ):
                    listener = args[1][0].text
                    closure_variable = listener_closures.get(listener)
                    closure_class = variable_classes.get(closure_variable) if closure_variable is not None else None
                    building_id = listener_unit_ids.get(listener)
                    ability_id = listener_ability_ids.get(listener)
                    if (
                        closure_variable is not None
                        and closure_variable not in registered_closures
                        and closure_class is not None
                        and building_id is not None
                        and building_id > 0
                        and ability_id is not None
                        and ability_id > 0
                    ):
                        rows.append({
                            "building_id": building_id,
                            "ability_id": ability_id,
                            "closure_variable": closure_variable,
                            "closure_class": closure_class,
                            "handler_function": handler_by_class[closure_class],
                            "registration_function": function_name,
                            "evidence_kind": "direct-spell-effect-event-listener",
                            "byte_offset": function_start + token.start,
                        })
                        registered_closures.add(closure_variable)
                index = max(index + 1, next_index)
                continue
            index += 1

    rows.sort(key=lambda row: (int(row["byte_offset"]), int(row["building_id"]), int(row["ability_id"])))
    return rows


def _extract_unit_spell_registrations(
    data: bytes,
    functions: list[dict[str, object]],
    function_aliases: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Recover generated scripted unit-spell registrations without executing Lua.

    The protected map routes unit spells through a generated five-argument
    registry helper whose fields are exposed by the map's own E2E arrays:
    unit rawcode, ability rawcode, target-mode enum, order ID/expression and a
    UnitSpellClosure callback. One special registration is emitted inline and
    additionally records an expected immediate unit rawcode. Prototype aliases
    provide the concrete closure handler exactly as with building spells.
    """
    handler_by_class: dict[str, str] = {}
    for alias in function_aliases:
        alias_name = str(alias["alias"])
        suffix = ".UnitSpellClosure_cast1"
        if not alias_name.endswith(suffix):
            continue
        class_name = alias_name[: -len(suffix)]
        target = str(alias["target_function"])
        prior = handler_by_class.get(class_name)
        if prior is not None and prior != target:
            raise ValueError(f"conflicting UnitSpellClosure_cast1 handlers for {class_name}: {prior} != {target}")
        handler_by_class[class_name] = target
    if not handler_by_class:
        return []

    target_modes = {
        "Tsb": (0, "enemy-ground-combat-sapper"),
        "Ssb": (1, "enemy-flying-combat-sapper"),
        "Rsb": (2, "ally-ground"),
        "Qsb": (3, "ally-structure"),
        "Psb": (4, "enemy-structure"),
        "Osb": (5, "enemy-mechanical"),
        "Nsb": (6, "immediate-enemy-special-unit"),
        "Msb": (7, "ally-any"),
    }
    mode_labels = {value: label for value, label in target_modes.values()}

    rows: list[dict[str, object]] = []
    seen_registration_keys: set[tuple[int, int, str, int]] = set()

    def add_row(
        *,
        function_name: str,
        function_start: int,
        byte_offset: int,
        unit_id: int,
        ability_id: int,
        target_mode: int,
        order_id: int | None,
        order_expression_kind: str,
        expected_immediate_unit_id: int,
        closure_variable: str,
        closure_class: str,
        evidence_kind: str,
    ) -> None:
        if unit_id <= 0 or ability_id <= 0:
            return
        if target_mode not in mode_labels:
            raise ValueError(
                f"unit-spell registration has unknown target mode {target_mode} "
                f"at byte {function_start + byte_offset}"
            )
        key = (unit_id, ability_id, closure_class, function_start + byte_offset)
        if key in seen_registration_keys:
            raise ValueError(f"duplicate unit-spell registration evidence at {key}")
        seen_registration_keys.add(key)
        rows.append({
            "unit_id": unit_id,
            "ability_id": ability_id,
            "target_mode": target_mode,
            "target_mode_label": mode_labels[target_mode],
            "order_id": order_id,
            "order_expression_kind": order_expression_kind,
            "expected_immediate_unit_id": expected_immediate_unit_id,
            "closure_variable": closure_variable,
            "closure_class": closure_class,
            "handler_function": handler_by_class[closure_class],
            "registration_function": function_name,
            "evidence_kind": evidence_kind,
            "byte_offset": function_start + byte_offset,
        })

    for function in functions:
        function_name = str(function["name"])
        body = _function_body_tokens(data, functions, function_name)
        if body is None:
            continue
        function_start, tokens = body
        variable_classes: dict[str, str] = {}
        integer_variables: dict[str, int] = {name: value for name, (value, _label) in target_modes.items()}
        used_closures: set[str] = set()
        array_slots: dict[str, dict[str, int]] = defaultdict(dict)
        array_offsets: dict[str, int] = {}
        listener_slots: dict[str, str] = {}
        listener_closures: dict[str, str] = {}

        def integer_argument(argument: list[LuaToken]) -> int | None:
            try:
                return _integer_literal_value(argument)
            except ValueError:
                pass
            if len(argument) == 1 and argument[0].kind == "ident":
                return integer_variables.get(argument[0].text)
            return None

        index = 0
        while index < len(tokens):
            token = tokens[index]

            if token.kind == "ident" and index + 2 < len(tokens) and tokens[index + 1].text == "=":
                rhs = tokens[index + 2]
                if rhs.kind == "number" and rhs.integer_value is not None:
                    integer_variables[token.text] = int(rhs.integer_value)
                elif rhs.kind == "ident" and rhs.text in integer_variables:
                    integer_variables[token.text] = integer_variables[rhs.text]
                elif (
                    rhs.kind == "ident"
                    and rhs.text == "__wurst_ensureInt"
                    and index + 5 < len(tokens)
                    and tokens[index + 3].text == "("
                    and tokens[index + 4].kind == "number"
                    and tokens[index + 4].integer_value is not None
                    and tokens[index + 5].text == ")"
                ):
                    integer_variables[token.text] = int(tokens[index + 4].integer_value)

                if (
                    rhs.kind == "ident"
                    and rhs.text in handler_by_class
                    and index + 5 < len(tokens)
                    and tokens[index + 3].text == ":"
                    and tokens[index + 4].kind == "ident"
                    and tokens[index + 4].text.startswith("create")
                    and tokens[index + 5].text == "("
                ):
                    variable_classes[token.text] = rhs.text

            if token.kind == "ident" and token.text == "_I":
                try:
                    args, next_index = _wurst_registry_call_arguments(tokens, index)
                except ValueError:
                    args = []
                    next_index = index + 1
                if len(args) == 5 and len(args[4]) == 1 and args[4][0].kind == "ident":
                    closure_variable = args[4][0].text
                    closure_class = variable_classes.get(closure_variable)
                    if closure_class is not None:
                        unit_id = integer_argument(args[0])
                        ability_id = integer_argument(args[1])
                        target_mode = integer_argument(args[2])
                        if unit_id is not None and ability_id is not None and target_mode is not None:
                            order_id = integer_argument(args[3])
                            add_row(
                                function_name=function_name,
                                function_start=function_start,
                                byte_offset=token.start,
                                unit_id=unit_id,
                                ability_id=ability_id,
                                target_mode=target_mode,
                                order_id=order_id,
                                order_expression_kind="integer" if order_id is not None else "protected-order-expression",
                                expected_immediate_unit_id=0,
                                closure_variable=closure_variable,
                                closure_class=closure_class,
                                evidence_kind="protected-registry-call",
                            )
                            used_closures.add(closure_variable)
                index = max(index + 1, next_index)
                continue

            if (
                token.kind == "ident"
                and token.text in {"usb", "tsb", "ssb", "rsb", "qsb"}
                and index + 5 < len(tokens)
                and tokens[index + 1].text == "["
                and tokens[index + 2].kind == "ident"
                and tokens[index + 3].text == "]"
                and tokens[index + 4].text == "="
            ):
                slot_variable = tokens[index + 2].text
                value_token = tokens[index + 5]
                value: int | None = None
                if value_token.kind == "number" and value_token.integer_value is not None:
                    value = int(value_token.integer_value)
                elif value_token.kind == "ident":
                    value = integer_variables.get(value_token.text)
                if value is not None:
                    array_slots[slot_variable][token.text] = value
                    array_offsets.setdefault(slot_variable, token.start)

            if (
                token.kind == "ident"
                and index + 4 < len(tokens)
                and tokens[index + 1].text == "."
                and tokens[index + 2].kind == "ident"
                and tokens[index + 3].text == "="
                and tokens[index + 4].kind == "ident"
            ):
                listener = token.text
                member = tokens[index + 2].text
                value = tokens[index + 4].text
                if member == "slot":
                    listener_slots[listener] = value
                elif member == "cb":
                    listener_closures[listener] = value

            index += 1

        for listener, closure_variable in listener_closures.items():
            if closure_variable in used_closures:
                continue
            closure_class = variable_classes.get(closure_variable)
            slot_variable = listener_slots.get(listener)
            if closure_class is None or slot_variable is None:
                continue
            fields = array_slots.get(slot_variable, {})
            if not {"usb", "tsb", "ssb", "rsb"} <= fields.keys():
                continue
            add_row(
                function_name=function_name,
                function_start=function_start,
                byte_offset=array_offsets.get(slot_variable, 0),
                unit_id=fields["usb"],
                ability_id=fields["tsb"],
                target_mode=fields["ssb"],
                order_id=fields["rsb"],
                order_expression_kind="integer",
                expected_immediate_unit_id=fields.get("qsb", 0),
                closure_variable=closure_variable,
                closure_class=closure_class,
                evidence_kind="inlined-registration",
            )

    rows.sort(key=lambda row: (int(row["byte_offset"]), int(row["unit_id"]), int(row["ability_id"])))
    return rows


def _extract_unit_spell_mechanics(
    data: bytes,
    functions: list[dict[str, object]],
    unit_spell_registrations: list[dict[str, object]],
    function_aliases: list[dict[str, object]],
    call_edges: Counter[tuple[str, str]],
    function_rawcodes: Counter[tuple[str, int]],
) -> list[dict[str, object]]:
    """Build a complete lexical mechanics profile for every scripted unit spell.

    Unlike the smaller building-spell set, many unit handlers hand work to a
    named helper or a generated delayed callback object. This index preserves
    those exact implementation links, primitive calls, timing literals and
    bounded rawcode reachability without pretending dynamic callbacks are
    ordinary synchronous calls.
    """
    if not unit_spell_registrations:
        return []

    defined = {str(function["name"]) for function in functions}
    calls_by_function: dict[str, list[str]] = defaultdict(list)
    anonymous_callees_by_parent: dict[str, set[str]] = defaultdict(set)
    for (caller, callee), _count in call_edges.items():
        calls_by_function[caller].append(callee)
        if caller.startswith("<anonymous@") and caller.endswith(">"):
            try:
                anonymous_offset = int(caller[len("<anonymous@"):-1])
            except ValueError:
                continue
            parent = _enclosing_named_function(functions, anonymous_offset)
            if parent != "<top-level>":
                anonymous_callees_by_parent[parent].add(callee)
    rawcodes_by_function: dict[str, set[int]] = defaultdict(set)
    for (function_name, integer_id), _count in function_rawcodes.items():
        rawcodes_by_function[function_name].add(int(integer_id))

    callback_dispatch_by_class: dict[str, list[tuple[str, str]]] = defaultdict(list)
    for alias in function_aliases:
        alias_name = str(alias["alias"])
        if "." not in alias_name:
            continue
        class_name, slot = alias_name.split(".", 1)
        target = str(alias["target_function"])
        if not target.startswith(slot):
            continue
        dispatch_kind = ""
        if "CallbackSingle_doAfter" in slot and "_call" in slot:
            dispatch_kind = "doAfter"
        elif "CallbackPeriodic_doPeriodically" in slot and "_call" in slot:
            dispatch_kind = "doPeriodically"
        elif "ForGroupCallback_forUnitsInRange" in slot and "_callback" in slot:
            dispatch_kind = "forUnitsInRange"
        elif "ForGroupCallback_forUnitsInRect" in slot and "_callback" in slot:
            dispatch_kind = "forUnitsInRect"
        elif "ForGroupCallback_forUnitsOfPlayer" in slot and "_callback" in slot:
            dispatch_kind = "forUnitsOfPlayer"
        if dispatch_kind:
            callback_dispatch_by_class[class_name].append((dispatch_kind, target))
    for class_name in callback_dispatch_by_class:
        callback_dispatch_by_class[class_name] = sorted(set(callback_dispatch_by_class[class_name]))

    primitive_priority = (
        ("dummyCastTargetWithVision", "dummy-target-ability-with-vision"),
        ("dummyCastTargetFrom1", "dummy-target-ability-from-owner"),
        ("dummyCastTargetFrom", "dummy-target-ability-from-caster"),
        ("dummyCastPointFrom", "dummy-point-ability"),
        ("dummyCastImmediateFrom1", "dummy-immediate-ability-from-owner"),
        ("dummyCastImmediateFrom", "dummy-immediate-ability-from-caster"),
        ("dummyCarrierWithAbilities1", "multi-ability-carrier"),
        ("dummyCarrierWithAbility", "ability-carrier"),
        ("createUnit", "spawn-unit"),
        ("addProtectedAbility", "apply-protected-ability"),
        ("unit_issueTargetOrderById", "unit-target-order"),
        ("unit_issueImmediateOrderById", "unit-immediate-order"),
        ("unit_issuePointOrderById", "unit-point-order"),
        ("doAfter", "scheduled-callback"),
    )
    infrastructure_calls = {
        "unit_getOwner", "unit_getPos", "unit_getX", "unit_getY", "unit_getTypeId",
        "player_getId", "tupleCopy1", "tupleCopy2", "real_asAngleDegrees", "angle_degrees",
        "vec2_distanceTo", "vec2_polarOffset", "GetRandomReal", "GetRandomInt", "OrderId",
        "doAfter", "doPeriodically", "createUnit", "addEffect1", "addEffect",
        "__wurst_safe_DestroyEffect", "__wurst_safe_UnitApplyTimedLife", "__wurst_ensureInt",
        "unit_issueTargetOrderById", "unit_issueImmediateOrderById", "unit_issuePointOrderById",
        "orderCodeAttack", "addProtectedAbility", "unit_removeAbility", "widget_getLife",
        "forUnitsInRange", "forUnitsInRect", "forUnitsOfPlayer",
    }

    def call_arguments(tokens: list[LuaToken], callee: str) -> list[list[list[LuaToken]]]:
        found: list[list[list[LuaToken]]] = []
        for index, token in enumerate(tokens):
            if token.kind != "ident" or token.text != callee:
                continue
            try:
                args, _next = _call_arguments(tokens, index)
            except ValueError:
                continue
            found.append(args)
        return found

    def numeric_arg(argument: list[LuaToken]) -> str | None:
        try:
            return _decimal_text(_decimal_literal_value(argument))
        except ValueError:
            return None

    rows: list[dict[str, object]] = []
    for registration in unit_spell_registrations:
        handler = str(registration["handler_function"])
        body = _function_body_tokens(data, functions, handler)
        if body is None:
            raise ValueError(f"unit-spell handler body is missing: {handler}")
        handler_start, tokens = body
        direct_calls = sorted(set(calls_by_function.get(handler, [])))

        mechanic_kind = "script-handler"
        for primitive, kind in primitive_priority:
            if primitive in direct_calls:
                mechanic_kind = kind
                break

        helper_calls = sorted({
            callee
            for callee in direct_calls
            if callee in defined
            and callee not in infrastructure_calls
            and ":create" not in callee
            and not callee.startswith("dummyCast")
            and not callee.startswith("dummyCarrier")
        })
        if mechanic_kind == "script-handler" and helper_calls:
            mechanic_kind = "delegated-helper"

        def dynamic_successors(function_tokens: list[LuaToken]) -> list[tuple[str, str]]:
            callback_classes: set[str] = set()
            for index in range(len(function_tokens) - 4):
                if (
                    function_tokens[index].kind == "ident"
                    and function_tokens[index + 1].text == ":"
                    and function_tokens[index + 2].kind == "ident"
                    and function_tokens[index + 2].text.startswith("create")
                    and function_tokens[index + 3].text == "("
                ):
                    callback_classes.add(function_tokens[index].text)
            return sorted({
                dispatch
                for class_name in callback_classes
                for dispatch in callback_dispatch_by_class.get(class_name, [])
            })

        direct_dynamic_dispatch = dynamic_successors(tokens)
        delayed_callbacks = sorted(target for kind, target in direct_dynamic_dispatch if kind == "doAfter")
        dynamic_callbacks = sorted(target for _kind, target in direct_dynamic_dispatch)

        scheduled_delays: list[str] = []
        for args in call_arguments(tokens, "doAfter"):
            if args:
                value = numeric_arg(args[0])
                if value is not None:
                    scheduled_delays.append(value)
        periodic_intervals: list[str] = []
        for args in call_arguments(tokens, "doPeriodically"):
            if args:
                value = numeric_arg(args[0])
                if value is not None:
                    periodic_intervals.append(value)
        random_real_ranges: list[list[str]] = []
        for args in call_arguments(tokens, "GetRandomReal"):
            if len(args) == 2:
                low = numeric_arg(args[0])
                high = numeric_arg(args[1])
                if low is not None and high is not None:
                    random_real_ranges.append([low, high])

        direct_rawcodes = sorted(rawcodes_by_function.get(handler, set()))

        # Follow exact named call edges plus generated doAfter callback dispatch
        # whose closure class is visible at each visited function. Callback
        # chains are bounded to four total edges so multi-stage spell state
        # machines are retained without turning this into unbounded decompilation.
        paths: dict[int, tuple[str, ...]] = {}
        function_paths: dict[str, tuple[str, ...]] = {handler: (handler,)}
        frontier: list[tuple[str, tuple[str, ...]]] = [(handler, (handler,))]
        seen_depth: dict[str, int] = {handler: 0}
        while frontier:
            function_name, path = frontier.pop(0)
            depth = len(path) - 1
            for integer_id in rawcodes_by_function.get(function_name, set()):
                prior = paths.get(integer_id)
                if prior is None or len(path) < len(prior) or (len(path) == len(prior) and path < prior):
                    paths[integer_id] = path
            if depth >= 4:
                continue
            successors = {
                callee
                for callee in (*calls_by_function.get(function_name, []), *anonymous_callees_by_parent.get(function_name, set()))
                if callee in defined
                and callee not in infrastructure_calls
                and not callee.startswith("__wurst_safe_")
                and not callee.startswith("dummyCast")
                and not callee.startswith("dummyCarrier")
            }
            function_body = _function_body_tokens(data, functions, function_name)
            if function_body is not None:
                dispatch_successors = dynamic_successors(function_body[1])
                successors.update(target for _kind, target in dispatch_successors)
                if function_name != handler:
                    delayed_callbacks.extend(target for kind, target in dispatch_successors if kind == "doAfter")
                    dynamic_callbacks.extend(target for _kind, target in dispatch_successors)
            successors = sorted(successors)
            for successor in successors:
                next_depth = depth + 1
                if seen_depth.get(successor, 99) < next_depth:
                    continue
                seen_depth[successor] = next_depth
                next_path = (*path, successor)
                function_paths[successor] = next_path
                frontier.append((successor, next_path))

        delayed_callbacks = sorted(set(delayed_callbacks))
        dynamic_callbacks = sorted(set(dynamic_callbacks))

        semantic_callees = {
            "__wurst_safe_UnitDamageTarget", "__wurst_safe_SetWidgetLife", "__wurst_safe_SetUnitState",
            "__wurst_safe_BlzSetUnitMaxHP", "__wurst_safe_SetUnitAbilityLevel",
            "__wurst_safe_BlzStartUnitAbilityCooldown", "__wurst_safe_UnitApplyTimedLife",
            "addProtectedAbility", "unit_removeAbility", "createUnit", "CreateDestructable", "RemoveDestructable",
            "dummyCastTargetFrom", "dummyCastTargetFrom1", "dummyCastTargetWithVision", "dummyCastPointFrom",
            "dummyCastImmediateFrom", "dummyCastImmediateFrom1", "dummyCarrierWithAbility",
            "dummyCarrierWithAbilities1", "dummyCarrierCastImmediate", "forUnitsInRange",
            "doAfter", "doPeriodically", "unit_issueTargetOrderById", "unit_issueImmediateOrderById",
            "unit_issuePointOrderById", "orderCodeAttack",
        }
        semantic_effect_sites: list[dict[str, object]] = []
        source_numeric_literals: list[dict[str, object]] = []
        known_referenced_rawcodes = {integer_id for values in rawcodes_by_function.values() for integer_id in values}
        for function_name, function_path in sorted(function_paths.items(), key=lambda item: (len(item[1]), item[1])):
            if function_name in semantic_callees or function_name.startswith("__wurst_safe_"):
                continue
            function_body = _function_body_tokens(data, functions, function_name)
            if function_body is None:
                continue
            function_start, function_tokens = function_body
            literals = []
            for number_token in function_tokens:
                if number_token.kind != "number":
                    continue
                if number_token.integer_value is not None and int(number_token.integer_value) in known_referenced_rawcodes:
                    continue
                literals.append(number_token.text)
            if literals:
                source_numeric_literals.append({
                    "function": function_name,
                    "path": list(function_path),
                    "hops": len(function_path) - 1,
                    "literals": literals,
                })
            for token_index, token in enumerate(function_tokens):
                if token.kind != "ident" or token.text not in semantic_callees:
                    continue
                try:
                    args, _next = _call_arguments(function_tokens, token_index)
                except ValueError:
                    continue
                argument_text = ["".join(part.text for part in argument) for argument in args]
                numeric_literals = [
                    [part.text for part in argument if part.kind == "number"]
                    for argument in args
                ]
                semantic_effect_sites.append({
                    "function": function_name,
                    "path": list(function_path),
                    "hops": len(function_path) - 1,
                    "callee": token.text,
                    "arguments": argument_text,
                    "numeric_literals": numeric_literals,
                    "byte_offset": function_start + token.start,
                })
        semantic_effect_sites.sort(key=lambda site: (int(site["byte_offset"]), str(site["callee"])))

        rawcode_paths = [
            {"rawcode_integer": integer_id, "path": list(path), "hops": len(path) - 1}
            for integer_id, path in sorted(paths.items())
        ]
        rows.append({
            "unit_id": int(registration["unit_id"]),
            "ability_id": int(registration["ability_id"]),
            "mechanic_kind": mechanic_kind,
            "handler_function": handler,
            "direct_calls": tuple(direct_calls),
            "helper_functions": tuple(helper_calls),
            "delayed_callback_functions": tuple(delayed_callbacks),
            "dynamic_callback_functions": tuple(dynamic_callbacks),
            "scheduled_delays": tuple(scheduled_delays),
            "periodic_intervals": tuple(periodic_intervals),
            "random_real_ranges": tuple(tuple(values) for values in random_real_ranges),
            "direct_map_rawcodes": tuple(direct_rawcodes),
            "reachable_map_rawcode_paths": tuple(rawcode_paths),
            "semantic_effect_sites": tuple(semantic_effect_sites),
            "source_numeric_literals": tuple(source_numeric_literals),
            "evidence_kind": "static-named-call-and-dispatch-evidence",
            "byte_offset": handler_start,
        })

    rows.sort(key=lambda row: (int(row["unit_id"]), int(row["ability_id"])))
    if len(rows) != len(unit_spell_registrations):
        raise ValueError("unit-spell mechanic profile coverage mismatch")
    return rows


def _extract_building_spell_evidence(
    data: bytes,
    functions: list[dict[str, object]],
    building_spell_registrations: list[dict[str, object]],
    function_aliases: list[dict[str, object]],
    call_edges: Counter[tuple[str, str]],
    function_rawcodes: Counter[tuple[str, int]],
) -> list[dict[str, object]]:
    """Reuse the generic spell-handler profiler for every building spell.

    Building and unit closure handlers use the same generated callback/runtime
    primitives. Converting the registration key temporarily lets the mature unit
    profiler preserve exact helper/callback/timing/rawcode evidence for the full
    building catalog, including direct EventListener registrations that are not
    yet represented by the stricter hand-normalized semantic layer.
    """
    proxy_registrations = [
        {
            "unit_id": int(row["building_id"]),
            "ability_id": int(row["ability_id"]),
            "handler_function": row["handler_function"],
        }
        for row in building_spell_registrations
    ]
    evidence_rows = _extract_unit_spell_mechanics(
        data,
        functions,
        proxy_registrations,
        function_aliases,
        call_edges,
        function_rawcodes,
    )
    rows: list[dict[str, object]] = []
    for evidence in evidence_rows:
        row = dict(evidence)
        row["building_id"] = row.pop("unit_id")
        rows.append(row)
    rows.sort(key=lambda row: (int(row["building_id"]), int(row["ability_id"])))
    return rows


def _extract_corpse_building_mechanics(
    data: bytes,
    functions: list[dict[str, object]],
    building_spell_registrations: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Recover the scripted building mechanics that explicitly select/consume corpses.

    This deliberately models only control flow that is visible and rigid in the
    generated Lua: the two Undead building handlers that call `raiseFromCorpse`
    and Vessel of Purity's `vesselOfPuritySpell`. Warcraft's ordinary `udea`
    raise/decay flags are not used by these handlers, so that distinction is
    retained explicitly for downstream native-content import.
    """

    def body_tokens(name: str) -> tuple[int, list[LuaToken]]:
        body = _function_body_tokens(data, functions, name)
        if body is None:
            raise ValueError(f"corpse-mechanic source function is missing: {name}")
        return body

    def calls(tokens: list[LuaToken], callee: str) -> list[tuple[int, list[list[LuaToken]]]]:
        found: list[tuple[int, list[list[LuaToken]]]] = []
        for index, token in enumerate(tokens):
            if token.kind != "ident" or token.text != callee:
                continue
            try:
                args, _next = _call_arguments(tokens, index)
            except ValueError:
                continue
            found.append((index, args))
        return found

    def has_token_sequence(tokens: list[LuaToken], sequence: tuple[str, ...]) -> bool:
        texts = [token.text for token in tokens]
        width = len(sequence)
        return any(tuple(texts[index:index + width]) == sequence for index in range(len(texts) - width + 1))

    relevant_handlers = {
        str(row["handler_function"])
        for row in building_spell_registrations
        if "RaceUndeadAbilities" in str(row["handler_function"])
        or "VesselOfPurity" in str(row["handler_function"])
    }
    if not relevant_handlers:
        return []

    # Validate the common Undead corpse-selection predicate and recover the
    # inherited Invulnerable ability rawcode used as an exclusion marker.
    undead_filter_name = "ForGroupCallback_forUnitsInRect_RaceUndeadAbilities_callback_forUnitsInRect_RaceUndeadAbilities"
    _filter_start, undead_filter = body_tokens(undead_filter_name)
    if len(calls(undead_filter, "isDyingCombatSapper")) != 1:
        raise ValueError("Undead raise filter no longer has exactly one isDyingCombatSapper predicate")
    ability_checks = calls(undead_filter, "unit_getAbilityLevel")
    if len(ability_checks) != 1 or len(ability_checks[0][1]) != 2:
        raise ValueError("Undead raise filter no longer has exactly one ability-level exclusion")
    invulnerable_ability_id = _integer_literal_value(ability_checks[0][1][1])
    if not any(token.kind == "ident" and token.text == "UNIT_TYPE_UNDEAD" for token in undead_filter):
        raise ValueError("Undead raise filter no longer excludes UNIT_TYPE_UNDEAD")

    random_source_name = "randomDyingSapper"
    _random_start, random_source = body_tokens(random_source_name)
    if len(calls(random_source, "__wurst_safe_GroupEnumUnitsInRect")) != 1:
        raise ValueError("randomDyingSapper no longer enumerates exactly one rect")
    if not any(token.kind == "ident" and token.text == "uIb" for token in random_source):
        raise ValueError("randomDyingSapper no longer enumerates the uIb battlefield rect")

    # Validate the exact 10/30/30/30 branch in raiseFromCorpse. The first
    # supplied unit type wins when a 0..99 roll is below 10; otherwise a 0..2
    # roll selects one of the remaining three uniformly.
    raise_start, raise_tokens = body_tokens("raiseFromCorpse")
    random_calls = calls(raise_tokens, "GetRandomInt")
    random_ranges = [
        (_integer_literal_value(args[0]), _integer_literal_value(args[1]))
        for _index, args in random_calls
        if len(args) == 2
    ]
    if random_ranges != [(0, 99), (0, 2)]:
        raise ValueError(f"raiseFromCorpse random structure changed: {random_ranges}")
    if not has_token_sequence(raise_tokens, ("GetRandomInt", "(", "0", ",", "99", ")", ">=", "10")):
        raise ValueError("raiseFromCorpse no longer uses the expected 10% first-outcome threshold")
    if len(calls(raise_tokens, "__wurst_safe_RemoveUnit")) != 1:
        raise ValueError("raiseFromCorpse no longer consumes exactly one selected corpse/unit")

    rows: list[dict[str, object]] = []
    for registration in building_spell_registrations:
        handler = str(registration["handler_function"])
        handler_body = _function_body_tokens(data, functions, handler)
        if handler_body is None:
            continue
        handler_start, handler_tokens = handler_body
        raise_calls = calls(handler_tokens, "raiseFromCorpse")
        if not raise_calls:
            continue
        if len(raise_calls) != 1 or len(raise_calls[0][1]) != 5:
            raise ValueError(f"building raise handler {handler} has unexpected raiseFromCorpse call shape")
        call_index, args = raise_calls[0]
        outcomes = [_integer_literal_value(argument) for argument in args[1:]]
        rows.append({
            "building_id": int(registration["building_id"]),
            "ability_id": int(registration["ability_id"]),
            "mechanic_kind": "scripted-raise-random",
            "corpse_phase": "dying",
            "selection_predicate": "combat-sapper;life<0.405;not-undead;missing-invulnerable-ability",
            "selection_rect_symbol": "uIb",
            "selection_function": random_source_name,
            "requires_wc3_can_raise": False,
            "consumption_mode": "selected-only",
            "consume_radius": None,
            "effect_radius": None,
            "damage": None,
            "attack_type": None,
            "damage_type": None,
            "auxiliary_ability_id": None,
            "invulnerable_ability_id": invulnerable_ability_id,
            "summon_outcomes": tuple(zip(outcomes, (10, 30, 30, 30), strict=True)),
            "handler_function": handler,
            "predicate_function": undead_filter_name,
            "effect_function": "raiseFromCorpse",
            "byte_offset": handler_start + handler_tokens[call_index].start,
        })

    # Vessel of Purity has a separate corpse predicate and consumes all matching
    # corpses around one randomly selected corpse before applying its AoE.
    vessel_registrations = [
        row for row in building_spell_registrations
        if str(row["handler_function"])
        == "BuildingSpellClosure_registerBuildingSpell_VesselOfPurity_cast_registerBuildingSpell_VesselOfPurity"
    ]
    if vessel_registrations:
        if len(vessel_registrations) != 1:
            raise ValueError("Vessel of Purity has multiple building-spell registrations")
        vessel_registration = vessel_registrations[0]
        vessel_handler = str(vessel_registration["handler_function"])
        vessel_handler_start, vessel_handler_tokens = body_tokens(vessel_handler)
        vessel_calls = calls(vessel_handler_tokens, "vesselOfPuritySpell")
        if len(vessel_calls) != 1:
            raise ValueError("Vessel of Purity handler no longer calls vesselOfPuritySpell exactly once")

        _predicate_start, vessel_predicate = body_tokens("isVesselCorpse")
        for callee in ("widget_isAliveTrick", "isCombatSapper", "isVulnerable", "unit_isType"):
            if len(calls(vessel_predicate, callee)) != 1:
                raise ValueError(f"isVesselCorpse no longer has exactly one {callee} predicate")
        if not any(token.kind == "ident" and token.text == "UNIT_TYPE_UNDEAD" for token in vessel_predicate):
            raise ValueError("isVesselCorpse no longer excludes UNIT_TYPE_UNDEAD")

        _consume_start, consume_tokens = body_tokens("consumeVesselCorpses")
        if len(calls(consume_tokens, "forUnitsInRange")) != 1 or len(calls(consume_tokens, "__wurst_safe_RemoveUnit")) != 1:
            raise ValueError("consumeVesselCorpses no longer enumerates a radius and removes each matched corpse")
        _find_start, find_tokens = body_tokens("findRandomVesselCorpse")
        if len(calls(find_tokens, "__wurst_safe_GroupEnumUnitsInRect")) != 1 or not any(
            token.kind == "ident" and token.text == "uIb" for token in find_tokens
        ):
            raise ValueError("findRandomVesselCorpse no longer selects from the uIb battlefield rect")

        # mP initializes the constants consumed by vesselOfPuritySpell and its
        # callbacks: auxiliary Far Sight, effect radius, consume radius, damage.
        _initializer_start, initializer = body_tokens("mP")
        constants: dict[str, int] = {}
        for index in range(len(initializer) - 2):
            if initializer[index].kind != "ident" or initializer[index + 1].text != "=":
                continue
            value = initializer[index + 2]
            if value.kind != "number":
                continue
            decimal_value = _decimal_literal_value([value])
            if decimal_value == decimal_value.to_integral_value():
                constants[initializer[index].text] = int(decimal_value)
        required_constants = {"WQ", "VQ", "UQ", "TQ"}
        if not required_constants <= constants.keys():
            raise ValueError(f"Vessel of Purity constants missing from mP: {sorted(required_constants - constants.keys())}")

        _effect_start, effect_tokens = body_tokens("vesselOfPuritySpell")
        for callee in ("findRandomVesselCorpse", "consumeVesselCorpses", "InstantDummyCaster_castPoint1", "forUnitsInRange"):
            if len(calls(effect_tokens, callee)) != 1:
                raise ValueError(f"vesselOfPuritySpell no longer calls {callee} exactly once")
        damage_callback_name = "ForGroupCallback_forUnitsInRange_VesselOfPurity_callback_forUnitsInRange_VesselOfPurity1"
        _damage_start, damage_callback = body_tokens(damage_callback_name)
        if len(calls(damage_callback, "__wurst_safe_UnitDamageTarget")) != 1:
            raise ValueError("Vessel of Purity damage callback no longer has exactly one UnitDamageTarget call")
        for symbol in ("ATTACK_TYPE_NORMAL", "DAMAGE_TYPE_UNIVERSAL"):
            if not any(token.kind == "ident" and token.text == symbol for token in damage_callback):
                raise ValueError(f"Vessel of Purity damage callback no longer uses {symbol}")

        call_index, _args = vessel_calls[0]
        rows.append({
            "building_id": int(vessel_registration["building_id"]),
            "ability_id": int(vessel_registration["ability_id"]),
            "mechanic_kind": "consume-area-and-damage",
            "corpse_phase": "dead",
            "selection_predicate": "dead;combat-sapper;vulnerable;not-undead",
            "selection_rect_symbol": "uIb",
            "selection_function": "findRandomVesselCorpse",
            "requires_wc3_can_raise": False,
            "consumption_mode": "all-qualifying-within-radius",
            "consume_radius": constants["UQ"],
            "effect_radius": constants["VQ"],
            "damage": constants["TQ"],
            "attack_type": "normal",
            "damage_type": "universal",
            "auxiliary_ability_id": constants["WQ"],
            "invulnerable_ability_id": None,
            "summon_outcomes": (),
            "handler_function": vessel_handler,
            "predicate_function": "isVesselCorpse",
            "effect_function": "vesselOfPuritySpell",
            "byte_offset": vessel_handler_start + vessel_handler_tokens[call_index].start,
        })

    rows.sort(key=lambda row: (int(row["building_id"]), int(row["ability_id"]), str(row["mechanic_kind"])))
    return rows


def _extract_building_spell_mechanics(
    data: bytes,
    functions: list[dict[str, object]],
    building_spell_registrations: list[dict[str, object]],
    corpse_building_mechanics: list[dict[str, object]],
    protected_filter_bindings: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Normalize concrete mechanics behind generated scripted building spells.

    The output intentionally keeps engine-object effects as rawcode links. Lua
    is used to prove targeting, delivery, timing, direct damage and branch
    structure; ability/unit object data remains a separate evidence source that
    the resolver joins later. This avoids silently treating tooltip prose or a
    protected object field as script-verified behavior.
    """
    if not building_spell_registrations:
        return []
    # This normalizer intentionally validates the generated Castle Fight
    # building-spell catalog as a whole. Tiny synthetic fixtures exercise the
    # lower-level registration extractor without needing every real-map helper.
    if len(building_spell_registrations) < 10:
        return []

    def rawcode(text: str) -> int:
        encoded = text.encode("latin1")
        if len(encoded) != 4:
            raise ValueError(f"expected four-byte rawcode, got {text!r}")
        return int.from_bytes(encoded, "big")

    def body(name: str) -> tuple[int, list[LuaToken]]:
        result = _function_body_tokens(data, functions, name)
        if result is None:
            raise ValueError(f"building-spell mechanic source function is missing: {name}")
        return result

    def call_sites(tokens: list[LuaToken], callee: str) -> list[tuple[int, list[list[LuaToken]]]]:
        sites: list[tuple[int, list[list[LuaToken]]]] = []
        for index, token in enumerate(tokens):
            if token.kind != "ident" or token.text != callee:
                continue
            try:
                args, _next = _call_arguments(tokens, index)
            except ValueError:
                continue
            sites.append((index, args))
        return sites

    def one_call(tokens: list[LuaToken], callee: str) -> tuple[int, list[list[LuaToken]]]:
        sites = call_sites(tokens, callee)
        if len(sites) != 1:
            raise ValueError(f"expected exactly one {callee} call, found {len(sites)}")
        return sites[0]

    def decimal_argument(argument: list[LuaToken]) -> Decimal:
        return _decimal_literal_value(argument)

    def integer_argument(argument: list[LuaToken]) -> int:
        return _integer_literal_value(argument)

    def literal_assignments(function_name: str) -> dict[str, Decimal]:
        _start, tokens = body(function_name)
        values: dict[str, Decimal] = {}
        for index in range(len(tokens) - 2):
            if tokens[index].kind != "ident" or tokens[index + 1].text != "=":
                continue
            rhs = tokens[index + 2 : index + 6]
            candidates: list[list[LuaToken]] = []
            if rhs and rhs[0].kind == "number":
                candidates.append([rhs[0]])
            if len(rhs) >= 2 and rhs[0].text in {"+", "-"} and rhs[1].kind == "number":
                candidates.append(rhs[:2])
            if len(rhs) >= 3 and rhs[0].text == "(" and rhs[2].text == ")" and rhs[1].kind == "number":
                candidates.append([rhs[1]])
            if (
                len(rhs) >= 4
                and rhs[0].text == "("
                and rhs[1].text in {"+", "-"}
                and rhs[2].kind == "number"
                and rhs[3].text == ")"
            ):
                candidates.append(rhs[1:3])
            if (
                len(rhs) >= 4
                and rhs[0].kind == "ident"
                and rhs[0].text == "__wurst_ensureInt"
                and rhs[1].text == "("
                and rhs[2].kind == "number"
                and rhs[3].text == ")"
            ):
                candidates.append([rhs[2]])
            for candidate in candidates:
                try:
                    values[tokens[index].text] = _decimal_literal_value(candidate)
                    break
                except ValueError:
                    continue
        return values

    registrations_by_building = {
        int(row["building_id"]): row for row in building_spell_registrations
    }
    if len(registrations_by_building) != len(building_spell_registrations):
        raise ValueError("multiple scripted building-spell registrations share one building rawcode")

    rows: list[dict[str, object]] = []

    def add(
        building_rawcode: str,
        mechanic_kind: str,
        target_selector: str,
        target_predicate: str,
        effect_rawcodes: Iterable[int],
        parameters: dict[str, object],
        source_functions: Iterable[str],
        *,
        evidence_kind: str = "script-direct",
    ) -> None:
        building_id = rawcode(building_rawcode)
        registration = registrations_by_building.get(building_id)
        if registration is None:
            return
        handler = str(registration["handler_function"])
        handler_body = _function_body_tokens(data, functions, handler)
        byte_offset = int(registration["byte_offset"])
        if handler_body is not None:
            byte_offset = handler_body[0]
        rows.append({
            "building_id": building_id,
            "ability_id": int(registration["ability_id"]),
            "mechanic_kind": mechanic_kind,
            "target_selector": target_selector,
            "target_predicate": target_predicate,
            "effect_rawcode_ids": tuple(dict.fromkeys(int(value) for value in effect_rawcodes)),
            "parameters": parameters,
            "source_functions": tuple(dict.fromkeys((handler, *source_functions))),
            "evidence_kind": evidence_kind,
            "byte_offset": byte_offset,
        })

    def protected_filter_evidence(*symbols: str) -> list[dict[str, object]]:
        """Retain protected-filter provenance without trusting coarse pairing as full semantics."""
        by_symbol = {str(row["symbol"]): row for row in protected_filter_bindings}
        result: list[dict[str, object]] = []
        for symbol in symbols:
            row = by_symbol.get(symbol)
            result.append({
                "symbol": symbol,
                "resolution_status": row["resolution_status"] if row is not None else "not-indexed",
                "resolved_function": row["resolved_function"] if row is not None else "",
                "coarse_predicate": row["predicate"] if row is not None else "",
                "semantic_use": "deferred-protected-filter-audit",
            })
        return result

    # The map has a second building-spell registration family built directly on
    # EVENT_PLAYER_UNIT_SPELL_EFFECT. These handlers are ordinary generated
    # closures rather than protected-registry calls, but their implementation is
    # just as statically visible and belongs in the same importer-facing catalog.

    # Gjallarhorn: every living allied combat sapper within 500 receives A016.
    # The applied level is capped at four and derives from the owner's generated
    # Gjallarhorn/team count table. DummyCaster.delay is a recycle delay, not a
    # cast delay (same DummyCaster convention used by item spell extraction).
    gjallar_handler = registrations_by_building.get(rawcode("h010"))
    if gjallar_handler is not None:
        gjallar_name = str(gjallar_handler["handler_function"])
        _gjallar_start, gjallar_tokens = body(gjallar_name)
        _range_index, range_call = one_call(gjallar_tokens, "forUnitsInRange")
        if len(range_call) != 4 or decimal_argument(range_call[1]) != Decimal(500):
            raise ValueError("Gjallarhorn range enumeration changed")
        gjallar_callback_name = "ForGroupCallback_forUnitsInRange_GjallarHorn_callback_forUnitsInRange_GjallarHorn"
        _callback_start, gjallar_callback = body(gjallar_callback_name)
        for required in ("unit_isAlive", "unit_isAllyOf", "isCombatSapper"):
            if len(call_sites(gjallar_callback, required)) != 1:
                raise ValueError(f"Gjallarhorn target predicate changed: {required}")
        _cast_index, gjallar_cast = one_call(gjallar_callback, "DummyCaster_DummyCaster_castTarget")
        if len(gjallar_cast) != 5 or integer_argument(gjallar_cast[1]) != rawcode("A016") or integer_argument(gjallar_cast[3]) != 852101:
            raise ValueError("Gjallarhorn buff cast changed")
        add(
            "h010",
            "area-allied-buff",
            "all-units-within-500-of-building",
            "alive;ally-of-owner;combat-sapper",
            (rawcode("A016"),),
            {
                "radius": 500,
                "effect_ability_id": rawcode("A016"),
                "effect_level_formula": "min(4, Icb[lGb[player_getId(owner)]])",
                "order_id": 852101,
                "dummy_recycle_delay_seconds": "1",
                "effect_parameters_source": "linked-ability-object-data",
            },
            (gjallar_callback_name,),
        )

    # Withering Hall: prioritize one protected generated filter, fall back to a
    # second, burn up to 200 mana, then deal twice the amount burned as sonic
    # damage. The exact protected predicates are deliberately kept as symbols
    # until the filter-pairing audit is complete.
    _withering_start, withering = body("spellWitheringTouch")
    _withering_damage_index, withering_damage = one_call(withering, "__wurst_safe_UnitDamageTarget")
    withering_constants = literal_assignments("pJ")
    if withering_constants.get("H6") != Decimal(200):
        raise ValueError("Withering Hall mana cap changed")
    if "DAMAGE_TYPE_SONIC" not in {token.text for token in withering_damage[6]}:
        raise ValueError("Withering Hall damage type changed")
    add(
        "h07T",
        "mana-burn-random-target",
        "random-from-G6-filter;fallback-random-from-F6-filter",
        "protected-generated-filters-G6/F6;shield-check-passes",
        (),
        {
            "target_filters": protected_filter_evidence("G6", "F6"),
            "mana_burn_cap": 200,
            "damage_formula": "2 * min(target_mana, 200)",
            "attack_type": "normal",
            "damage_type": "sonic",
            "attack_flag": True,
            "ranged_flag": False,
        },
        ("spellWitheringTouch",),
        evidence_kind="script-direct-with-protected-target-filters",
    )

    # Magic Ruin: one random live enemy, shield-gated, followed by a uniform
    # ten-way branch table. Branches 8 and 9 deliberately have no gameplay
    # effect beyond the E2E bookkeeping call.
    _ruin_start, ruin = body("magicRuinSpell")
    _ruin_random_index, ruin_random = one_call(ruin, "GetRandomInt")
    if [integer_argument(argument) for argument in ruin_random] != [0, 9]:
        raise ValueError("Magic Ruin branch range changed")
    ruin_damage_calls = call_sites(ruin, "__wurst_safe_UnitDamageTarget")
    ruin_damage_amounts = [decimal_argument(args[2]) for _index, args in ruin_damage_calls]
    if ruin_damage_amounts != [Decimal(50000), Decimal(500)]:
        raise ValueError(f"Magic Ruin direct damage branches changed: {ruin_damage_amounts}")
    add(
        "h05I",
        "uniform-random-effect-table",
        "random-alive-enemy",
        "randomAliveEnemy-helper;shield-check-passes;ground-retarget-only-for-stun-branch",
        (rawcode("A018"), rawcode("A0BL"), rawcode("A06S"), rawcode("A06T"), rawcode("A06U"), rawcode("n01S")),
        {
            "selection": "uniform-GetRandomInt(0,9)",
            "branch_probability_percent": 10,
            "branches": [
                {"roll": 0, "effect": "explode-target", "increments_o0": True},
                {"roll": 1, "effect": "direct-damage", "damage": 50000, "attack_type": "normal", "damage_type": "death"},
                {"roll": 2, "effect": "direct-damage", "damage": 500, "attack_type": "normal", "damage_type": "death"},
                {"roll": 3, "effect": "add-A06T-and-A06U;heal-to-max", "ability_ids": [rawcode("A06T"), rawcode("A06U")]},
                {"roll": 4, "effect": "dummy-target-hex", "ability_id": rawcode("A018"), "order_id": 852502, "dummy_lifetime_seconds": "1", "reengage": True},
                {"roll": 5, "effect": "dummy-target-banish", "ability_id": rawcode("A0BL"), "order_id": 852486, "dummy_lifetime_seconds": "1", "reengage": True},
                {"roll": 6, "effect": "dummy-target-stun;retarget-ground-if-needed", "ability_id": rawcode("A06S"), "order_id": 852095, "dummy_lifetime_seconds": "2", "reengage": False},
                {"roll": 7, "effect": "replace-target-with-mutation", "unit_id": rawcode("n01S"), "increments_o0": True},
                {"roll": 8, "effect": "no-gameplay-effect"},
                {"roll": 9, "effect": "no-gameplay-effect"},
            ],
        },
        ("magicRuinSpell", "chaosControlDummy"),
    )

    # Eraser tiers share one implementation. A random live enemy chooses the
    # unit type; every live enemy of that type across the battlefield is then
    # hit for lethal chaos/death damage if it passes the callback exclusions.
    _eraser_start, eraser = body("eraserSpell")
    if len(call_sites(eraser, "randomAliveEnemy")) != 1 or len(call_sites(eraser, "__wurst_safe_GroupEnumUnitsInRect")) != 1:
        raise ValueError("Eraser selection structure changed")
    eraser_callback_name = "ForGroupCallback_forUnitsInRect_RaceChaosAbilities_callback_forUnitsInRect_RaceChaosAbilities"
    _eraser_callback_start, eraser_callback = body(eraser_callback_name)
    _eraser_damage_index, eraser_damage = one_call(eraser_callback, "__wurst_safe_UnitDamageTarget")
    if decimal_argument(eraser_damage[2]) != Decimal(100500):
        raise ValueError("Eraser damage changed")
    eraser_callback_text = {token.text for token in eraser_callback}
    if not {"ATTACK_TYPE_CHAOS", "DAMAGE_TYPE_DEATH"} <= eraser_callback_text:
        raise ValueError("Eraser attack/damage type changed")
    for eraser_building in ("h01M", "h01W", "h01X"):
        add(
            eraser_building,
            "same-unit-type-battlefield-wipe",
            "random-alive-enemy-selects-unit-type;then-all-battlefield-matches",
            "life>0.405;enemy;same-unit-type;passes-Id-exclusion;missing-A070",
            (rawcode("A070"),),
            {
                "damage": 100500,
                "attack_type": "chaos",
                "damage_type": "death",
                "attack_flag": True,
                "ranged_flag": False,
                "protected_or_helper_exclusion": "not Id(unit)",
                "excluded_ability_id": rawcode("A070"),
            },
            ("eraserSpell", eraser_callback_name),
        )

    # Volcano: point-cast Flame Strike at a random living ground enemy. The
    # artillery-mode switch substitutes the alternate -na object but leaves the
    # script delivery constants unchanged.
    _volcano_start, volcano = body("volcanoSpell")
    _volcano_cast_index, volcano_cast = one_call(volcano, "dummyCastPointFrom")
    if integer_argument(volcano_cast[2]) != 852488 or decimal_argument(volcano_cast[5]) != Decimal(6):
        raise ValueError("Volcano dummy cast changed")
    artillery_switch_text = {token.text for token in body("setArtilleryModeAbilityIds")[1]}
    for integer_id in (rawcode("A01I"), rawcode("A05H")):
        if str(integer_id) not in artillery_switch_text:
            raise ValueError("Volcano artillery-mode ability mapping changed")
    add(
        "h01F",
        "dummy-point-ability",
        "random-alive-enemy-ground",
        "randomAliveEnemyGround-helper;linked-ability-target-mask",
        (rawcode("A01I"), rawcode("A05H")),
        {
            "normal_ability_id": rawcode("A01I"),
            "alternate_mode_ability_id": rawcode("A05H"),
            "order_id": 852488,
            "dummy_lifetime_seconds": "6",
            "effect_parameters_source": "selected-linked-ability-object-data",
        },
        ("volcanoSpell", "setArtilleryModeAbilityIds"),
    )

    # Shrine of Destruction: choose a random enemy structure with explicit
    # castle/legendary exclusions, create the SoD attack dummy, then scale its
    # freeze/DoT ability levels at 20/25/35 round-minute thresholds.
    _sod_start, sod = body("shrineOfDestructionSpell")
    sod_callback_name = "ForGroupCallback_forUnitsInRect_RaceChaosAbilities_callback_forUnitsInRect_RaceChaosAbilities1"
    _sod_filter_start, sod_filter = body(sod_callback_name)
    sod_filter_text = {token.text for token in sod_filter}
    if not {"UNIT_TYPE_STRUCTURE", "bj_MAX_PLAYERS"} <= sod_filter_text:
        raise ValueError("Shrine of Destruction structure filter changed")
    _sod_life_index, sod_life = one_call(sod, "__wurst_safe_UnitApplyTimedLife")
    _sod_order_index, sod_order = one_call(sod, "unit_issueTargetOrderById")
    if decimal_argument(sod_life[2]) != Decimal(10) or integer_argument(sod_order[1]) != 851983:
        raise ValueError("Shrine of Destruction dummy lifetime/order changed")
    add(
        "h01E",
        "spawn-attack-dummy-with-round-scaling",
        "random-battlefield-enemy-structure",
        "alive;enemy;structure;normal-player;not-hcas/h06M/h07J/h07K;missing-A06V",
        (rawcode("h06C"), rawcode("A0EU"), rawcode("A0FE"), rawcode("A06V"), rawcode("hcas"), rawcode("h06M"), rawcode("h07J"), rawcode("h07K")),
        {
            "dummy_unit_id": rawcode("h06C"),
            "freeze_ability_id": rawcode("A0EU"),
            "damage_over_time_ability_id": rawcode("A0FE"),
            "timed_life_seconds": "10",
            "order_id": 851983,
            "ability_level_schedule": [
                {"round_minutes_gt": 19, "level": 2},
                {"round_minutes_gt": 24, "level": 3},
                {"round_minutes_gt": 34, "level": 4},
            ],
            "excluded_unit_ids": [rawcode("hcas"), rawcode("h06M"), rawcode("h07J"), rawcode("h07K")],
            "excluded_ability_id": rawcode("A06V"),
            "effect_parameters_source": "dummy-unit-and-linked-ability-object-data",
        },
        ("shrineOfDestructionSpell", sod_callback_name),
    )

    # Forgotten One: six temporary dummies form a radius-64 hexagon around a
    # random living ground enemy. The dummy rawcode switches in -na mode.
    _forgotten_start, forgotten = body("forgottenOneSpell")
    _forgotten_life_index, forgotten_life = one_call(forgotten, "__wurst_safe_UnitApplyTimedLife")
    if decimal_argument(forgotten_life[2]) != Decimal(9):
        raise ValueError("Forgotten One dummy lifetime changed")
    add(
        "h04R",
        "six-dummy-ring",
        "random-alive-enemy-ground",
        "randomAliveEnemyGround-helper",
        (rawcode("n01C"), rawcode("n021")),
        {
            "dummy_count": 6,
            "ring_radius": 64,
            "angle_step_degrees": 60,
            "normal_dummy_unit_id": rawcode("n01C"),
            "alternate_mode_dummy_unit_id": rawcode("n021"),
            "timed_life_seconds": "9",
            "effect_parameters_source": "selected-dummy-unit-object-data",
        },
        ("forgottenOneSpell", "setArtilleryModeAbilityIds"),
    )

    # Well of Pain: wounded enemies are preferred, with a random-live fallback.
    # A successful shield-gated hit deals 160 sonic damage; a lethal hit spawns
    # one Ghost of Sorrow at the victim position.
    _pain_start, pain = body("wellOfPainSpell")
    _pain_damage_index, pain_damage = one_call(pain, "__wurst_safe_UnitDamageTarget")
    _pain_spawn_index, pain_spawn = one_call(pain, "createUnit")
    if decimal_argument(pain_damage[2]) != Decimal(160) or integer_argument(pain_spawn[1]) != rawcode("n03C"):
        raise ValueError("Well of Pain damage/spawn changed")
    add(
        "h04K",
        "damage-with-on-kill-summon",
        "random-wounded-enemy;fallback-random-alive-enemy",
        "randomWoundedEnemy/randomAliveEnemy helpers;shield-check-passes",
        (rawcode("n03C"),),
        {
            "damage": 160,
            "attack_type": "normal",
            "damage_type": "sonic",
            "attack_flag": True,
            "ranged_flag": False,
            "kill_threshold_life": "<0.405",
            "on_kill_unit_id": rawcode("n03C"),
        },
        ("wellOfPainSpell", "randomWoundedEnemy"),
    )

    # Beacon of the Oasis: three protected generated filters form an exact
    # priority sequence. The chosen unit receives +175 life and +40 mana. Keep
    # the filter symbols intact until the protected-filter audit proves their
    # complete predicates; the old coarse pairing is insufficient here.
    _replenish_start, replenish = body("spellReplenish")
    replenish_life = call_sites(replenish, "__wurst_safe_SetUnitState")
    if len(replenish_life) != 2 or "175." not in {token.text for token in replenish} or "40." not in {token.text for token in replenish}:
        raise ValueError("Beacon of the Oasis replenish amounts changed")
    add(
        "h079",
        "priority-random-replenish",
        "random-from-Y0;fallback-X0;fallback-W0",
        "protected-generated-filter-sequence-Y0/X0/W0",
        (),
        {
            "target_filters": protected_filter_evidence("Y0", "X0", "W0"),
            "selection_order": ["Y0", "X0", "W0"],
            "life_gain": 175,
            "mana_gain": 40,
        },
        ("spellReplenish",),
        evidence_kind="script-direct-with-protected-target-filters",
    )

    # Sacred Peyote: one random unit from protected filter V0, shield-gated,
    # receives the Item Illusions ability. Object data supplies 125% dealt,
    # 75% received and 60-second duration; the script order is the canonical
    # illusion order 852274.
    _peyote_start, peyote = body("spellPeyote")
    _peyote_cast_index, peyote_cast = one_call(peyote, "dummyCastTargetWithVision")
    desert_constants = literal_assignments("rK")
    if desert_constants.get("Z0") != Decimal(rawcode("AM0y")) or "Z0" not in {token.text for token in peyote_cast[1]}:
        raise ValueError("Sacred Peyote illusion ability alias changed")
    add(
        "h07A",
        "dummy-target-illusion",
        "random-from-V0-filter",
        "protected-generated-filter-V0;shield-check-passes",
        (rawcode("AM0y"),),
        {
            "target_filters": protected_filter_evidence("V0"),
            "effect_ability_id": rawcode("AM0y"),
            "order_id": 852274,
            "dummy_lifetime_seconds": "1",
            "effect_parameters_source": "linked-ability-object-data",
        },
        ("spellPeyote",),
        evidence_kind="script-direct-with-protected-target-filter",
    )

    # Temple of Storm: a six-second map-scale Sandstorm carrier combines the
    # miss/slow ability A0G4 with the periodic damage ability A0GC. Weather and
    # building animation are restored by the generated delayed callback.
    storm_registration = registrations_by_building.get(rawcode("n03A"))
    if storm_registration is not None:
        storm_handler_name = str(storm_registration["handler_function"])
        _storm_start, storm = body(storm_handler_name)
        _storm_carrier_index, storm_carrier = one_call(storm, "dummyCarrierWithAbilities")
        _storm_order_index, storm_order = one_call(storm, "unit_issuePointOrderById")
        _storm_delay_index, storm_delay = one_call(storm, "doAfter")
        if (
            integer_argument(storm_carrier[1]) != rawcode("A0G4")
            or integer_argument(storm_carrier[2]) != rawcode("A0GC")
            or decimal_argument(storm_carrier[4]) != Decimal(6)
            or integer_argument(storm_order[1]) != 852592
            or decimal_argument(storm_delay[0]) != Decimal(6)
        ):
            raise ValueError("Temple of Storm carrier/order/duration changed")
        add(
            "n03A",
            "global-weather-ability-carrier",
            "map-wide-via-20000-area-linked-abilities",
            "linked-ability-target-masks",
            (rawcode("A0G4"), rawcode("A0GC")),
            {
                "carrier_ability_ids": [rawcode("A0G4"), rawcode("A0GC")],
                "carrier_position": [0, 3500],
                "order_id": 852592,
                "order_position": [16, 3500],
                "duration_seconds": "6",
                "weather_effect_symbol": "U0",
                "effect_parameters_source": "linked-ability-object-data",
            },
            (storm_handler_name, "CallbackSingle_doAfter_RaceDesertAbilities_call_doAfter_RaceDesertAbilities"),
        )

    # Obelisk of Elements: prefer an eligible ally missing A0F4, then relax that
    # preference. If more than 350 HP are missing it heals 450; otherwise it
    # chooses uniformly among currently absent A0D7/A0F3/A0F1 effects, with the
    # generated index-sensitive A03Q branch preserved exactly, then adds A0F4.
    _holy_start, holy = body("holyShrineSpell")
    _holy_predicate_start, holy_predicate = body("isHolyShrineTarget")
    for required in ("isAliveCombatSapper", "unit_isAllyOf", "unit_getAbilityLevel", "passesRetryTargetGate"):
        if not call_sites(holy_predicate, required):
            raise ValueError(f"Obelisk of Elements target predicate changed: {required}")
    holy_added: list[int] = []
    for _index, args in call_sites(holy, "addProtectedAbility"):
        if len(args) != 2:
            continue
        try:
            holy_added.append(integer_argument(args[1]))
        except ValueError:
            # The chosen A0D7/A0F3/A0F1 candidate is carried through Wpr.
            pass
    expected_holy_added = [rawcode("A03Q"), rawcode("A0F4")]
    if holy_added != expected_holy_added:
        raise ValueError(f"Obelisk of Elements literal ability additions changed: {holy_added}")
    holy_text = {token.text for token in holy}
    for integer_id in (rawcode("A0D7"), rawcode("A0F3"), rawcode("A0F1"), rawcode("A03Q"), rawcode("A0F4")):
        if str(integer_id) not in holy_text:
            raise ValueError("Obelisk of Elements candidate ability table changed")
    add(
        "h060",
        "heal-or-random-missing-buff",
        "random-eligible-ally;fallback-relaxes-A0F4-preference",
        "alive-combat-sapper;ally;missing-Avul;first-pass-missing-A0F4",
        (rawcode("Avul"), rawcode("A0D7"), rawcode("A0F3"), rawcode("A0F1"), rawcode("A03Q"), rawcode("A0F4")),
        {
            "preferred_missing_ability_id": rawcode("A0F4"),
            "heal_if_missing_hp_gt": 350,
            "heal_amount": 450,
            "candidate_ability_ids": [rawcode("A0D7"), rawcode("A0F3"), rawcode("A0F1")],
            "candidate_selection": "uniform-among-currently-absent-candidates",
            "fallback_if_no_candidate": "heal-450",
            "always_after-buff_ability_id": rawcode("A0F4"),
            "generated_selected-index-2_extra_ability_id": rawcode("A03Q"),
            "generated_index_note": "A03Q is keyed to the selected compacted-array index, not normalized ability identity",
        },
        ("holyShrineSpell", "holyPick", "isHolyShrineTarget", "ForGroupCallback_forUnitsInRect_RaceElementalAbilities_callback_forUnitsInRect_RaceElementalAbilities1"),
    )

    # Elemental Rain: one random living ground enemy point, then a 50/50 choice
    # between Blizzard and Monsoon. Each has a normal and -na object selected by
    # the shared artillery-mode switch.
    _lightning_start, lightning = body("lightningSpell")
    _lightning_random_index, lightning_random = one_call(lightning, "GetRandomInt")
    if [integer_argument(argument) for argument in lightning_random] != [0, 1]:
        raise ValueError("Elemental Rain 50/50 selection changed")
    add(
        "h06F",
        "uniform-random-point-ability",
        "random-alive-enemy-ground",
        "randomAliveEnemyGround-helper;linked-ability-target-mask",
        (rawcode("A0D1"), rawcode("A0D2"), rawcode("A0E3"), rawcode("A0E4")),
        {
            "selection": "uniform-GetRandomInt(0,1)",
            "normal_options": [
                {"ability_id": rawcode("A0D1"), "order_id": 852089},
                {"ability_id": rawcode("A0E3"), "order_id": 852591},
            ],
            "alternate_mode_options": [
                {"ability_id": rawcode("A0D2"), "order_id": 852089},
                {"ability_id": rawcode("A0E4"), "order_id": 852591},
            ],
            "dummy_lifetime_seconds": "10",
            "effect_parameters_source": "selected-linked-ability-object-data",
        },
        ("lightningSpell", "setArtilleryModeAbilityIds"),
    )

    # Meteor Shower: explicit visible filter selects a random living enemy
    # flying combat sapper lacking Avul, then point-casts A0D0 for ten seconds.
    _meteor_start, meteor = body("frostSpell")
    meteor_filter_name = "ForGroupCallback_forUnitsInRect_RaceElementalAbilities_callback_forUnitsInRect_RaceElementalAbilities"
    _meteor_filter_start, meteor_filter = body(meteor_filter_name)
    for required in ("isAliveCombatSapper", "unit_isEnemyOf", "unit_isType", "unit_getAbilityLevel"):
        if len(call_sites(meteor_filter, required)) != 1:
            raise ValueError(f"Meteor Shower target predicate changed: {required}")
    _meteor_cast_index, meteor_cast = one_call(meteor, "dummyCastPointFrom")
    if integer_argument(meteor_cast[1]) != rawcode("A0D0") or integer_argument(meteor_cast[2]) != 852238 or decimal_argument(meteor_cast[5]) != Decimal(10):
        raise ValueError("Meteor Shower dummy cast changed")
    add(
        "h06E",
        "dummy-point-ability",
        "random-battlefield-unit",
        "alive-combat-sapper;enemy;flying;missing-Avul",
        (rawcode("A0D0"), rawcode("Avul")),
        {
            "effect_ability_id": rawcode("A0D0"),
            "order_id": 852238,
            "dummy_lifetime_seconds": "10",
            "effect_parameters_source": "linked-ability-object-data",
        },
        ("frostSpell", meteor_filter_name),
    )

    # City of Magic: shield-gated random live enemy Hex. The linked A018 object
    # carries the actual 45-second duration; generated re-engage callbacks are
    # implementation detail and remain present in building-spell-evidence.tsv.
    _hex_start, hex_spell = body("hexSpell")
    _hex_cast_index, hex_cast = one_call(hex_spell, "dummyCastTargetWithVision")
    if integer_argument(hex_cast[1]) != rawcode("A018") or integer_argument(hex_cast[2]) != 852502:
        raise ValueError("City of Magic Hex cast changed")
    add(
        "h00Z",
        "dummy-target-ability",
        "random-alive-enemy",
        "randomAliveEnemy-helper;shield-check-passes",
        (rawcode("A018"),),
        {
            "effect_ability_id": rawcode("A018"),
            "order_id": 852502,
            "dummy_lifetime_seconds": "1",
            "effect_parameters_source": "linked-ability-object-data",
        },
        ("hexSpell",),
    )

    # Blue/Red Shield Generator share the visible randomAllySapper selector.
    # Blue requires an unshielded target and applies A09L level 1. Red prefers
    # an unshielded target, falls back to any eligible ally, then enforces level
    # 2; this upgrades an existing shield on the fallback path.
    mech_filter_name = "ForGroupCallback_forUnitsInRect_RaceMechAbilities_callback_forUnitsInRect_RaceMechAbilities"
    _mech_filter_start, mech_filter = body(mech_filter_name)
    for required in ("isAliveCombatSapper", "unit_isAllyOf", "unit_getAbilityLevel"):
        if not call_sites(mech_filter, required):
            raise ValueError(f"Shield Generator target predicate changed: {required}")
    add(
        "h06J",
        "apply-shield-level-1",
        "randomAllySapper(owner,true)",
        "alive-combat-sapper;ally;missing-Avul;missing-A09L",
        (rawcode("A09L"), rawcode("Avul")),
        {
            "shield_ability_id": rawcode("A09L"),
            "shield_level": 1,
            "building_animation_reset_delay_seconds": "0.3",
        },
        ("blueShieldSpell", "randomAllySapper", mech_filter_name),
    )
    add(
        "h05R",
        "apply-or-upgrade-shield-level-2",
        "randomAllySapper(owner,true);fallback-randomAllySapper(owner,false)",
        "alive-combat-sapper;ally;missing-Avul;prefer-missing-A09L",
        (rawcode("A09L"), rawcode("Avul")),
        {
            "shield_ability_id": rawcode("A09L"),
            "shield_level": 2,
            "fallback_can_upgrade_existing_shield": True,
            "building_animation_reset_delay_seconds": "0.3",
        },
        ("redShieldSpell", "randomAllySapper", mech_filter_name),
    )

    # Naga utility trio: all use the common random enemy helpers and explicit
    # dummy casts. Tidal Guardian is cooldown-driven (15s) rather than mana-
    # cadence-driven; that distinction is represented in building-spells.tsv.
    for building_code, helper_name, selector, predicate, ability_code, order_id, lifetime, cast_helper in (
        ("h00N", "tidalSpell", "random-alive-enemy-ground", "randomAliveEnemyGround-helper", "A00P", 852218, Decimal(20), "dummyCastTargetWithVision"),
        ("h00M", "oracleSpell", "random-alive-enemy", "randomAliveEnemy-helper;shield-check-passes", "A00N", 852581, Decimal(1), "dummyCastTargetWithVision"),
        ("h00I", "pyramidSpell", "random-alive-enemy-ground", "randomAliveEnemyGround-helper", "A00B", 852096, Decimal(1), "dummyCastImmediateFrom1"),
    ):
        _naga_start, naga_spell = body(helper_name)
        _naga_cast_index, naga_cast = one_call(naga_spell, cast_helper)
        if integer_argument(naga_cast[1]) != rawcode(ability_code) or integer_argument(naga_cast[2]) != order_id or decimal_argument(naga_cast[-1]) != lifetime:
            raise ValueError(f"{helper_name} dummy cast changed")
        add(
            building_code,
            "dummy-target-ability" if cast_helper == "dummyCastTargetWithVision" else "dummy-immediate-ability-at-target-point",
            selector,
            predicate,
            (rawcode(ability_code),),
            {
                "effect_ability_id": rawcode(ability_code),
                "order_id": order_id,
                "dummy_lifetime_seconds": _decimal_text(lifetime),
                "effect_parameters_source": "linked-ability-object-data",
            },
            (helper_name,),
        )

    # Ancient of Wonders: six activations at 0.3-second intervals, alternating
    # between two generated preplaced-unit slots for the owner's team. Each unit
    # is transferred to the owner, marked sapper, ordered to attack and receives
    # 42 seconds timed life. The concrete preplaced unit type is map-placement
    # state rather than an authored rawcode literal in this handler.
    wonders_registration = registrations_by_building.get(rawcode("h02D"))
    if wonders_registration is not None:
        wonders_handler_name = str(wonders_registration["handler_function"])
        _wonders_start, wonders_handler = body(wonders_handler_name)
        _periodic_index, periodic_call = one_call(wonders_handler, "doPeriodically")
        if decimal_argument(periodic_call[0]) != Decimal("0.3"):
            raise ValueError("Ancient of Wonders periodic interval changed")
        wonders_callback_name = "CallbackPeriodic_doPeriodically_RaceNatureAbilities_call_doPeriodically_RaceNatureAbilities"
        _wonders_callback_start, wonders_callback = body(wonders_callback_name)
        _wonders_life_index, wonders_life = one_call(wonders_callback, "__wurst_safe_UnitApplyTimedLife")
        if decimal_argument(wonders_life[2]) != Decimal(42):
            raise ValueError("Ancient of Wonders timed life changed")
        add(
            "h02D",
            "periodic-preplaced-unit-release",
            "alternating-generated-preplaced-slots-for-owner-team",
            "slot-source-sA(baseSlot + iteration%2);preplaced-unit-exists",
            (rawcode("Awha"),),
            {
                "period_seconds": "0.3",
                "iterations": 6,
                "base_slot_formula": "2 * lGb[player_getId(owner)]",
                "slot_formula": "baseSlot + (iteration % 2)",
                "unit_source_function": "sA",
                "removed_ability_id": rawcode("Awha"),
                "sets_owner_to_building_owner": True,
                "adds_unit_type": "UNIT_TYPE_SAPPER",
                "issues_attack_order": True,
                "timed_life_seconds": "42",
            },
            (wonders_handler_name, wonders_callback_name, "sA"),
        )

    # Ancient Guardian: shield-gated Banish on one random live enemy.
    _banish_start, banish = body("banishGuardianSpell")
    _banish_cast_index, banish_cast = one_call(banish, "dummyCastTargetWithVision")
    if integer_argument(banish_cast[1]) != rawcode("A0BL") or integer_argument(banish_cast[2]) != 852486:
        raise ValueError("Ancient Guardian Banish cast changed")
    add(
        "h02C",
        "dummy-target-ability",
        "random-alive-enemy",
        "randomAliveEnemy-helper;shield-check-passes",
        (rawcode("A0BL"),),
        {
            "effect_ability_id": rawcode("A0BL"),
            "order_id": 852486,
            "dummy_lifetime_seconds": "1",
            "effect_parameters_source": "linked-ability-object-data",
        },
        ("banishGuardianSpell",),
    )

    # Ancient of Gale: point-cast the tornado ability at one random live enemy.
    # The shared artillery-mode switch substitutes A0BQ for A0BD in -na mode.
    _gale_start, gale = body("galeSpell")
    _gale_cast_index, gale_cast = one_call(gale, "dummyCastPointFrom")
    if integer_argument(gale_cast[2]) != 852597 or decimal_argument(gale_cast[5]) != Decimal(5):
        raise ValueError("Ancient of Gale cast changed")
    add(
        "h02A",
        "dummy-point-ability",
        "random-alive-enemy",
        "randomAliveEnemy-helper;linked-ability-target-mask",
        (rawcode("A0BD"), rawcode("A0BQ")),
        {
            "normal_ability_id": rawcode("A0BD"),
            "alternate_mode_ability_id": rawcode("A0BQ"),
            "order_id": 852597,
            "dummy_lifetime_seconds": "5",
            "effect_parameters_source": "selected-linked-ability-object-data",
        },
        ("galeSpell", "setArtilleryModeAbilityIds"),
    )

    # Obelisk of Wilderness: exact visible helper selects a random allied combat
    # sapper. applyWildernessObeliskBuff conditionally adds spell resistance when
    # none of the four exclusion effects is present, then always adds Hardened
    # Skin and Endurance Aura.
    wilderness_predicate_name = "isWildernessObeliskTarget"
    _wilderness_predicate_start, wilderness_predicate = body(wilderness_predicate_name)
    for required in ("isAliveCombatSapper", "unit_isAllyOf", "unit_getAbilityLevel", "hasWildernessObeliskBuff"):
        if len(call_sites(wilderness_predicate, required)) != 1:
            raise ValueError(f"Obelisk of Wilderness target predicate changed: {required}")
    _wilderness_apply_start, wilderness_apply = body("applyWildernessObeliskBuff")
    wilderness_added = [integer_argument(args[1]) for _index, args in call_sites(wilderness_apply, "addProtectedAbility") if len(args) == 2]
    if wilderness_added != [rawcode("A08C"), rawcode("A08D"), rawcode("A08F")]:
        raise ValueError(f"Obelisk of Wilderness buff bundle changed: {wilderness_added}")
    add(
        "h07H",
        "persistent-buff-bundle",
        "random-from-isWildernessObeliskTarget",
        "alive-combat-sapper;ally;predicate-helper-hasWildernessObeliskBuff/ability-gate",
        (rawcode("A08C"), rawcode("A08D"), rawcode("A08F"), rawcode("A08K"), rawcode("A0AH"), rawcode("A0AI"), rawcode("A0BV")),
        {
            "conditional_spell_resistance_ability_id": rawcode("A08C"),
            "spell_resistance_exclusion_ability_ids": [rawcode("A08K"), rawcode("A0AH"), rawcode("A0AI"), rawcode("A0BV")],
            "always_ability_ids": [rawcode("A08D"), rawcode("A08F")],
            "selection_function": wilderness_predicate_name,
            "effect_parameters_source": "linked-ability-object-data",
        },
        ("wildernessObeliskSpell", wilderness_predicate_name, "hasWildernessObeliskBuff", "applyWildernessObeliskBuff"),
    )

    # Silver Glade: each cast adds one team charge and enables the shared
    # EVENT_PLAYER_UNIT_ATTACKED trigger. A qualifying event consumes one team
    # charge and casts A08A at the event unit. The protected NK call is the exact
    # charge test/decrement function; both team counters reset on the watched
    # round/progress signal.
    silver_registration = registrations_by_building.get(rawcode("h08P"))
    if silver_registration is not None:
        silver_handler_name = str(silver_registration["handler_function"])
        _silver_start, silver_handler = body(silver_handler_name)
        if len(call_sites(silver_handler, "addSilverGladeChargeForTeam")) != 1:
            raise ValueError("Silver Glade charge grant changed")
        _silver_proc_start, silver_proc = body("PK")
        _silver_cast_index, silver_cast = one_call(silver_proc, "dummyCastImmediateFrom1")
        if integer_argument(silver_cast[1]) != rawcode("A08A") or integer_argument(silver_cast[2]) != 852269:
            raise ValueError("Silver Glade proc cast changed")
        add(
            "h08P",
            "team-charge-next-attack-proc",
            "shared-EVENT_PLAYER_UNIT_ATTACKED-trigger",
            "event-unit-is-combat-sapper;missing-A08H;owner-team-has-charge",
            (rawcode("A08A"), rawcode("A08H")),
            {
                "charges_granted_per_cast": 1,
                "charge_counter": "R0[team]",
                "charge_consume_function": "NK",
                "trigger_symbol": "Q0",
                "trigger_event": "EVENT_PLAYER_UNIT_ATTACKED",
                "excluded_ability_id": rawcode("A08H"),
                "effect_ability_id": rawcode("A08A"),
                "order_id": 852269,
                "dummy_lifetime_seconds": "1",
                "counter_reset_function": "resetSilverGladeCounterForTeam",
                "round_end_signal_resets_both_team_counters": True,
            },
            (silver_handler_name, "addSilverGladeChargeForTeam", "OK", "PK", "NK", "resetSilverGladeCounterForTeam", "Action_watch_RaceNelfAbilities_run_watch_RaceNelfAbilities"),
        )

    # Starfall Obelisk: choose one random live enemy and cast the Starfall object
    # at its coordinates. Normal mode uses A07T, -na mode A08T. A temporary 1100
    # vision modifier is created unless NGb is active; visual/fog cleanup occurs
    # after eight seconds while the dummy cast lifetime is eleven seconds.
    _starfall_start, starfall = body("starfallSpell")
    _starfall_cast_index, starfall_cast = one_call(starfall, "dummyCastImmediateFrom1")
    if integer_argument(starfall_cast[2]) != 852183 or decimal_argument(starfall_cast[4]) != Decimal(11):
        raise ValueError("Starfall Obelisk cast changed")
    _starfall_cleanup_index, starfall_cleanup = one_call(starfall, "doAfter")
    if decimal_argument(starfall_cleanup[0]) != Decimal(8):
        raise ValueError("Starfall Obelisk cleanup delay changed")
    add(
        "h07I",
        "dummy-immediate-area-ability",
        "random-alive-enemy-position",
        "randomAliveEnemy-helper;linked-ability-target-mask",
        (rawcode("A07T"), rawcode("A08T")),
        {
            "normal_ability_id": rawcode("A07T"),
            "alternate_mode_ability_id": rawcode("A08T"),
            "order_id": 852183,
            "dummy_lifetime_seconds": "11",
            "temporary_vision_radius": 1100,
            "vision_skipped_when_symbol_true": "NGb",
            "visual_and_fog_cleanup_delay_seconds": "8",
            "effect_parameters_source": "selected-linked-ability-object-data",
        },
        ("starfallSpell", "setArtilleryModeAbilityIds", "CallbackSingle_doAfter_RaceNelfAbilities_call_doAfter_RaceNelfAbilities"),
    )

    # Chilling Mushroom: random eligible flying enemy, shield-gated, delivered
    # through one dummy Storm Bolt ability. Damage/duration are intentionally
    # left to the object-data join (the map tooltip disagrees with DataA).
    mushroom_start, mushroom = body("mushroomSpell")
    _mushroom_cast_index, mushroom_cast = one_call(mushroom, "dummyCastTargetWithVision")
    if len(mushroom_cast) != 6:
        raise ValueError("mushroomSpell dummy cast argument count changed")
    mushroom_ability = integer_argument(mushroom_cast[1])
    mushroom_order = integer_argument(mushroom_cast[2])
    mushroom_dummy_life = decimal_argument(mushroom_cast[5])
    if len(call_sites(mushroom, "randomFlyingEnemySapper")) != 1 or len(call_sites(mushroom, "checkForShield")) != 1:
        raise ValueError("mushroomSpell target/shield structure changed")
    _mushroom_filter_start, mushroom_filter = body(
        "ForGroupCallback_forUnitsInRect_RaceNorthernAbilities_callback_forUnitsInRect_RaceNorthernAbilities"
    )
    for required in ("isAliveCombatSapper", "unit_isEnemyOf", "unit_isType", "unit_getAbilityLevel"):
        if len(call_sites(mushroom_filter, required)) != 1:
            raise ValueError(f"Chilling Mushroom target filter changed: {required}")
    add(
        "h047",
        "dummy-target-ability",
        "random-battlefield-unit",
        "alive-combat-sapper;enemy;flying;missing-invulnerable-ability;shield-check-passes",
        (mushroom_ability, rawcode("Avul")),
        {
            "dummy_ability_id": mushroom_ability,
            "order_id": mushroom_order,
            "dummy_lifetime_seconds": _decimal_text(mushroom_dummy_life),
            "effect_parameters_source": "ability-object-data",
        },
        ("mushroomSpell", "randomFlyingEnemySapper", "ForGroupCallback_forUnitsInRect_RaceNorthernAbilities_callback_forUnitsInRect_RaceNorthernAbilities"),
    )

    # Frost Launchers: random eligible enemy structure; the spawned dummy unit's
    # protected UnitStat weapon and attached Freezing Breath ability define the
    # actual impact damage/freeze profile downstream.
    _frost_start, frost = body("frostLauncherSpell")
    _timed_index, timed_life = one_call(frost, "__wurst_safe_UnitApplyTimedLife")
    if len(timed_life) != 3:
        raise ValueError("frostLauncherSpell timed-life call changed")
    frost_lifetime = decimal_argument(timed_life[2])
    _order_start, order_tokens = body("issueFrostLauncherOrder")
    order_calls = call_sites(order_tokens, "unit_issueTargetOrderById")
    if not order_calls:
        raise ValueError("issueFrostLauncherOrder has no target order")
    order_ids = {integer_argument(args[1]) for _index, args in order_calls if len(args) == 3}
    if order_ids != {851983}:
        raise ValueError(f"unexpected Frost Launcher order IDs: {sorted(order_ids)}")
    _frost_filter_start, frost_filter = body(
        "ForGroupCallback_forUnitsInRect_RaceNorthernAbilities_callback_forUnitsInRect_RaceNorthernAbilities1"
    )
    filter_text = {token.text for token in frost_filter}
    required_filter_tokens = {"UNIT_TYPE_STRUCTURE", "bj_MAX_PLAYERS"}
    if not required_filter_tokens <= filter_text:
        raise ValueError("Frost Launcher structure target filter changed")
    filter_rawcodes = {
        int(token.integer_value)
        for token in frost_filter
        if token.kind == "number" and token.integer_value is not None and int(token.integer_value) > 0xFFFFFF
    }
    expected_filter_rawcodes = {rawcode("hcas"), rawcode("h06M"), rawcode("B01K")}
    if not expected_filter_rawcodes <= filter_rawcodes:
        raise ValueError("Frost Launcher exclusions no longer include both castles and Power Armor")
    for building_code, dummy_code, freeze_ability_code in (("h048", "h04G", "A04F"), ("h03L", "h04H", "A04K")):
        registration = registrations_by_building.get(rawcode(building_code))
        if registration is None:
            continue
        handler_name = str(registration["handler_function"])
        _handler_start, handler_tokens = body(handler_name)
        _call_index, helper_args = one_call(handler_tokens, "frostLauncherSpell")
        if len(helper_args) != 2 or integer_argument(helper_args[1]) != rawcode(dummy_code):
            raise ValueError(f"{building_code} no longer launches expected dummy {dummy_code}")
        add(
            building_code,
            "spawn-attack-dummy",
            "random-battlefield-structure",
            "alive;enemy;structure;normal-player;not-main-castle;not-alt-castle;missing-power-armor",
            (rawcode(dummy_code), rawcode(freeze_ability_code), rawcode("B01K")),
            {
                "dummy_unit_id": rawcode(dummy_code),
                "freeze_ability_id": rawcode(freeze_ability_code),
                "timed_life_seconds": _decimal_text(frost_lifetime),
                "order_id": 851983,
                "excluded_unit_ids": [rawcode("hcas"), rawcode("h06M")],
                "excluded_buff_id": rawcode("B01K"),
                "effect_parameters_source": "dummy-unit-protected-unitstat-and-object-data",
            },
            (handler_name, "frostLauncherSpell", "randomNorthernBuildingTarget", "ForGroupCallback_forUnitsInRect_RaceNorthernAbilities_callback_forUnitsInRect_RaceNorthernAbilities1", "issueFrostLauncherOrder"),
        )

    # World Freezer: three moving orb dummies. The scripted mover proves motion,
    # bounce and target cadence; the linked abilities carry slow/damage/freeze.
    world_registration = registrations_by_building.get(rawcode("h03O"))
    if world_registration is not None:
        world_handler = str(world_registration["handler_function"])
        _world_handler_start, world_handler_tokens = body(world_handler)
        _world_delay_index, world_delay = one_call(world_handler_tokens, "doAfter")
        world_spawn_delay = decimal_argument(world_delay[0])
        _world_callback_start, world_callback = body(
            "CallbackSingle_doAfter_RaceNorthernAbilities_call_doAfter_RaceNorthernAbilities1"
        )
        create_calls = call_sites(world_callback, "createUnit")
        if len(create_calls) != 1 or len(create_calls[0][1]) < 2:
            raise ValueError("World Freezer spawn callback changed")
        orb_unit = integer_argument(create_calls[0][1][1])
        process_assignments = literal_assignments("RK")
        if process_assignments.get("K0") != Decimal(-2000) or process_assignments.get("J0") != Decimal(2000):
            raise ValueError("World Freezer vertical bounce bounds changed")
        _add_orb_start, add_orb = body("addWorldFreezerMissile")
        _timer_index, timer_call = one_call(add_orb, "__wurst_safe_TimerStart")
        timer_period = decimal_argument(timer_call[1])
        _process_start, process = body("processWorldFreezerMissiles")
        process_text = [token.text for token in process]
        if "12." not in process_text or "700." not in process_text or "50" not in process_text:
            raise ValueError("World Freezer movement/target constants changed")
        target_abilities = []
        for _index, args in call_sites(process, "dummyCastTargetWithVision"):
            if len(args) >= 2:
                target_abilities.append(integer_argument(args[1]))
        expected_target_abilities = [rawcode("A082"), rawcode("A083"), rawcode("A04H")]
        if target_abilities != expected_target_abilities:
            raise ValueError(f"World Freezer target ability chain changed: {target_abilities}")
        add(
            "h03O",
            "moving-orb-field",
            "three-lane-projectiles-with-periodic-random-nearby-target",
            "orb-aura:enemy;target-selection:alive-enemy-within-700;special-flying-vs-ground-effects",
            (orb_unit, rawcode("A081"), rawcode("A080"), rawcode("A084"), *target_abilities),
            {
                "spawn_delay_seconds": _decimal_text(world_spawn_delay),
                "orb_count": 3,
                "angle_offsets_degrees": [-45, 0, 45],
                "orb_unit_id": orb_unit,
                "movement_tick_seconds": _decimal_text(timer_period),
                "movement_step_world_units": 12,
                "movement_speed_world_units_per_second": _decimal_text(Decimal(12) / timer_period),
                "vertical_bounds": [-2000, 2000],
                "target_check_after_ticks_gt": 50,
                "target_check_nominal_seconds": _decimal_text(timer_period * Decimal(51)),
                "target_radius": 700,
                "ambient_slow_ability_id": rawcode("A081"),
                "ambient_damage_ability_id": rawcode("A080"),
                "alternate_mode_damage_ability_id": rawcode("A084"),
                "flying_target_ability_id": rawcode("A082"),
                "ground_stun_ability_id": rawcode("A083"),
                "ground_damage_ability_id": rawcode("A04H"),
                "effect_parameters_source": "linked-ability-object-data",
            },
            (world_handler, "CallbackSingle_doAfter_RaceNorthernAbilities_call_doAfter_RaceNorthernAbilities1", "addWorldFreezerMissile", "processWorldFreezerMissiles"),
        )

    # Ceremonial Totem: four persistent base bonuses plus one of three equally
    # likely two-ability option bundles. Target retry semantics are retained
    # exactly instead of paraphrasing the tooltip's "doesn't already have any".
    _ceremonial_start, ceremonial = body("ceremonialTotemSpell")
    random_choices = call_sites(ceremonial, "GetRandomInt")
    if len(random_choices) != 1 or [integer_argument(arg) for arg in random_choices[0][1]] != [1, 3]:
        raise ValueError("Ceremonial Totem random option structure changed")
    added_abilities = [
        integer_argument(args[1])
        for _index, args in call_sites(ceremonial, "addProtectedAbility")
        if len(args) == 2
    ]
    expected_added = [
        rawcode("A03B"), rawcode("A03A"), rawcode("A03D"), rawcode("A08L"),
        rawcode("A05I"), rawcode("A06W"), rawcode("A08M"), rawcode("A06Y"),
        rawcode("A08R"), rawcode("A06X"),
    ]
    if added_abilities != expected_added:
        raise ValueError(f"Ceremonial Totem ability bundle changed: {added_abilities}")
    _ceremonial_predicate_start, ceremonial_predicate = body("isCeremonialTotemTarget")
    predicate_ability_checks = [
        integer_argument(args[1])
        for _index, args in call_sites(ceremonial_predicate, "unit_getAbilityLevel")
        if len(args) == 2
    ]
    if predicate_ability_checks != [rawcode("Avul"), rawcode("A0F1")]:
        raise ValueError("Ceremonial Totem target-gate abilities changed")
    add(
        "h02R",
        "persistent-random-buff-bundle",
        "random-battlefield-ally-with-retry",
        "alive-combat-sapper;ally;missing-invulnerable-ability;first-pass-missing-A0F1;fallback-relaxes-A0F1",
        (*expected_added, rawcode("Avul"), rawcode("A0F1")),
        {
            "always_ability_ids": [rawcode("A03B"), rawcode("A03A"), rawcode("A03D"), rawcode("A08L")],
            "option_selection": "uniform-GetRandomInt(1,3)",
            "option_weights": [1, 1, 1],
            "option_ability_ids": [
                [rawcode("A05I"), rawcode("A06W")],
                [rawcode("A08M"), rawcode("A06Y")],
                [rawcode("A08R"), rawcode("A06X")],
            ],
            "duration_seconds": None,
            "effect_parameters_source": "linked-ability-object-data",
        },
        ("ceremonialTotemSpell", "ceremonialPick", "isCeremonialTotemTarget", "passesRetryTargetGate"),
    )

    # Global Orc totems are direct dummy ability carriers. Their durations are
    # script literals; effect values/radii come from the linked WC3 abilities.
    for building_code, expected_ability_code, helper, mechanic, duration in (
        ("h07X", "Ast9", "dummyCastPointFrom", "global-point-ability", Decimal(1)),
        ("h07Y", "AstB", "dummyCarrierWithAbility", "global-aura-carrier", Decimal(8)),
        ("h07Z", "AstC", "dummyCarrierWithAbility", "global-aura-carrier", Decimal(6)),
    ):
        registration = registrations_by_building.get(rawcode(building_code))
        if registration is None:
            continue
        handler_name = str(registration["handler_function"])
        _handler_start, handler_tokens = body(handler_name)
        _helper_index, helper_args = one_call(handler_tokens, helper)
        if len(helper_args) < 2 or integer_argument(helper_args[1]) != rawcode(expected_ability_code):
            raise ValueError(f"{building_code} linked global effect ability changed")
        actual_duration = decimal_argument(helper_args[-1])
        if actual_duration != duration:
            raise ValueError(f"{building_code} global effect duration changed: {actual_duration}")
        add(
            building_code,
            mechanic,
            "map-wide-via-object-ability",
            "linked-ability-target-mask",
            (rawcode(expected_ability_code),),
            {
                "effect_ability_id": rawcode(expected_ability_code),
                "carrier_or_dummy_lifetime_seconds": _decimal_text(actual_duration),
                "effect_parameters_source": "linked-ability-object-data",
            },
            (handler_name,),
        )

    # Serpent Rock: random live enemy target, delivered as a dummy Acid Bomb.
    _serpent_start, serpent = body("serpentRockShoot")
    _serpent_cast_index, serpent_cast = one_call(serpent, "dummyCastTargetWithVision")
    if len(serpent_cast) != 6:
        raise ValueError("Serpent Rock dummy cast argument count changed")
    serpent_ability = integer_argument(serpent_cast[1])
    if len(call_sites(serpent, "randomAliveEnemy")) != 1:
        raise ValueError("Serpent Rock no longer selects randomAliveEnemy")
    add(
        "h02N",
        "dummy-target-ability",
        "random-alive-enemy",
        "randomAliveEnemy-helper;linked-ability-target-mask",
        (serpent_ability,),
        {
            "dummy_ability_id": serpent_ability,
            "order_id": integer_argument(serpent_cast[2]),
            "dummy_lifetime_seconds": _decimal_text(decimal_argument(serpent_cast[5])),
            "effect_parameters_source": "linked-ability-object-data",
        },
        ("serpentRockShoot",),
    )

    # Death Pit: direct scripted death damage, shield-gated.
    _death_start, death = body("deathPitSpell")
    _death_damage_index, death_damage = one_call(death, "__wurst_safe_UnitDamageTarget")
    if len(death_damage) != 8 or len(call_sites(death, "randomAliveEnemy")) != 1 or len(call_sites(death, "checkForShield")) != 1:
        raise ValueError("Death Pit selection/damage structure changed")
    death_text = [token.text for token in death]
    if "DAMAGE_TYPE_DEATH" not in death_text or "ATTACK_TYPE_NORMAL" not in death_text:
        raise ValueError("Death Pit attack/damage type assignments changed")
    add(
        "h00A",
        "direct-random-target-damage",
        "random-alive-enemy",
        "randomAliveEnemy-helper;shield-check-passes",
        (),
        {
            "damage": _decimal_text(decimal_argument(death_damage[2])),
            "attack_type": "normal",
            "damage_type": "death",
            "attack_flag": True,
            "ranged_flag": False,
        },
        ("deathPitSpell",),
    )

    # Snowveil Fountain: automatic plus-shaped snow placement and team-owned
    # 20% incoming-damage reduction. Its same subsystem also exposes the manual
    # snow detonation constants; the automatic random-target filter SX remains
    # obfuscated and is explicitly retained as unresolved rather than guessed.
    snow_constants = literal_assignments("FL")
    snow_filter = next((
        binding for binding in protected_filter_bindings
        if binding["symbol"] == "SX" and binding["resolution_status"] == "resolved"
    ), None)
    required_snow = {"aW", "bW", "ZV", "YV", "XV", "WV", "VV"}
    if not required_snow <= snow_constants.keys():
        raise ValueError(f"Snowveil constants missing: {sorted(required_snow - snow_constants.keys())}")
    _snow_start, snow = body("createSnowveilSnow")
    if len(call_sites(snow, "group_getRandom")) != 1 or len(call_sites(snow, "vec2_setSnow")) != 1:
        raise ValueError("Snowveil automatic placement structure changed")
    snow_tokens = [token.text for token in snow]
    if "128." not in snow_tokens:
        raise ValueError("Snowveil tile spacing changed")
    _snow_damage_start, snow_damage = body("DamageListener_addListener_SnowveilFountain_onEvent_addListener_SnowveilFountain")
    if "0.8" not in [token.text for token in snow_damage]:
        raise ValueError("Snowveil incoming damage multiplier changed")
    _snow_manual_start, snow_manual = body("damageUnitsOnSnowInArea")
    _manual_damage_index, manual_damage = one_call(snow_manual, "__wurst_safe_UnitDamageTarget")
    if not any(token.kind == "ident" and token.text == "DAMAGE_TYPE_UNIVERSAL" for token in manual_damage[6]):
        raise ValueError("Snowveil detonation damage type changed")
    add(
        "h07W",
        "team-snowfield",
        "random-unit-from-generated-SX-filter",
        (
            snow_filter["predicate"] + ";damage-reduction-applies-when-target-team-owns-snow-tile"
            if snow_filter is not None
            else "SX-filter-unresolved;damage-reduction-applies-when-target-team-owns-snow-tile"
        ),
        (int(snow_constants["bW"]),),
        {
            "tile_spacing_world_units": 128,
            "placement_shape": "center-plus-four-cardinal-neighbors",
            "placement_tile_count": 5,
            "incoming_damage_multiplier": "0.8",
            "incoming_damage_reduction_percent": 20,
            "manual_explosion_ability_id": int(snow_constants["bW"]),
            "manual_explosion_radius": _decimal_text(snow_constants["YV"]),
            "manual_explosion_damage": _decimal_text(snow_constants["XV"]),
            "manual_explosion_damage_type": "universal",
            "manual_explosion_normal_cooldown_seconds": _decimal_text(snow_constants["WV"]),
            "manual_explosion_Pcb_cooldown_seconds": _decimal_text(snow_constants["VV"]),
            "automatic_target_filter_status": "resolved-generated-filter-SX" if snow_filter is not None else "unresolved-generated-filter-SX",
            "automatic_target_filter_function": snow_filter["resolved_function"] if snow_filter is not None else "",
            "automatic_target_filter_predicate": snow_filter["predicate"] if snow_filter is not None else "",
            "round_end_signal_removes_all_snow_without_explosion": True,
        },
        ("createSnowveilSnow", "vec2_setSnow", "DamageListener_addListener_SnowveilFountain_onEvent_addListener_SnowveilFountain", "damageUnitsOnSnowInArea", "explodeSnowInArea", "Action_watch_SnowveilFountain_run_watch_SnowveilFountain", "FL"),
        evidence_kind=(
            "script-direct-with-resolved-generated-target-filter"
            if snow_filter is not None else "script-direct-with-unresolved-target-filter"
        ),
    )

    # Thunderpaw: each cast grants one team charge. The next qualifying melee
    # attack consumes one charge and emits clamped universal AoE damage; flying
    # recipients take half. A07M is an explicit source-side special modifier.
    thunder_registration = registrations_by_building.get(rawcode("h07R"))
    if thunder_registration is not None:
        thunder_handler = str(thunder_registration["handler_function"])
        _thunder_start, thunder = body(thunder_handler)
        if not any(token.kind == "ident" and token.text == "RS" for token in thunder):
            raise ValueError("Thunderpaw cast no longer increments RS charge state")
        listener_name = "DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire"
        _listener_start, listener = body(listener_name)
        listener_text = [token.text for token in listener]
        for expected in ("128.1", "75.", "300.", "0.35", "0.5"):
            if expected not in listener_text:
                raise ValueError(f"Thunderpaw listener constant changed: {expected}")
        if len(call_sites(listener, "__wurst_safe_BlzGetUnitWeaponRealField")) != 2:
            raise ValueError("Thunderpaw melee range check changed")
        _splash_start, splash = body(
            "ForGroupCallback_forUnitsInRange_addListener_doAfter_ThunderpawSpire_callback_forUnitsInRange_addListener_doAfter_ThunderpawSpire"
        )
        _splash_damage_index, splash_damage = one_call(splash, "__wurst_safe_UnitDamageTarget")
        if not any(token.kind == "ident" and token.text == "DAMAGE_TYPE_UNIVERSAL" for token in splash_damage[6]):
            raise ValueError("Thunderpaw splash damage type changed")
        add(
            "h07R",
            "armed-next-melee-attack-splash",
            "next-qualifying-allied-melee-attack",
            "team-charge>0;attack-damage-event;source-damage>10;source-not-invulnerable;weapon0-range>1-and<128.1;weapon1-range<128.1",
            (rawcode("A07M"),),
            {
                "charges_granted_per_cast": 1,
                "charges_consumed_per_proc": 1,
                "damage_multiplier": 2,
                "minimum_damage": 75,
                "maximum_damage": 300,
                "effect_radius": 300,
                "flying_recipient_multiplier": "0.5",
                "special_source_ability_id": rawcode("A07M"),
                "special_source_multiplier": "0.35",
                "damage_type": "universal",
                "attack_type": "normal",
                "round_end_signal_resets_all_charge_slots": True,
            },
            (thunder_handler, listener_name, "ForGroupCallback_forUnitsInRange_addListener_doAfter_ThunderpawSpire_callback_forUnitsInRange_addListener_doAfter_ThunderpawSpire", "Action_watch_doAfter_ThunderpawSpire_run_watch_doAfter_ThunderpawSpire"),
        )

    # Fold the already-validated corpse-dependent building mechanics into this
    # stricter semantic catalog. Direct EventListener registrations are covered
    # by building-spell-evidence until their script-specific normalization is
    # promoted here.
    for corpse in corpse_building_mechanics:
        building_id = int(corpse["building_id"])
        registration = registrations_by_building.get(building_id)
        if registration is None:
            raise ValueError(f"corpse building mechanic lacks registration: {building_id}")
        effect_ids: list[int] = []
        if corpse["auxiliary_ability_id"] is not None:
            effect_ids.append(int(corpse["auxiliary_ability_id"]))
        if corpse["invulnerable_ability_id"] is not None:
            effect_ids.append(int(corpse["invulnerable_ability_id"]))
        effect_ids.extend(int(unit_id) for unit_id, _probability in corpse["summon_outcomes"])
        rows.append({
            "building_id": building_id,
            "ability_id": int(corpse["ability_id"]),
            "mechanic_kind": str(corpse["mechanic_kind"]),
            "target_selector": str(corpse["selection_function"]),
            "target_predicate": str(corpse["selection_predicate"]),
            "effect_rawcode_ids": tuple(dict.fromkeys(effect_ids)),
            "parameters": {
                "corpse_phase": corpse["corpse_phase"],
                "requires_wc3_can_raise": bool(corpse["requires_wc3_can_raise"]),
                "consumption_mode": corpse["consumption_mode"],
                "consume_radius": corpse["consume_radius"],
                "effect_radius": corpse["effect_radius"],
                "damage": corpse["damage"],
                "attack_type": corpse["attack_type"],
                "damage_type": corpse["damage_type"],
                "auxiliary_ability_id": corpse["auxiliary_ability_id"],
                "summon_outcomes": [
                    {"unit_id": int(unit_id), "probability_percent": int(probability)}
                    for unit_id, probability in corpse["summon_outcomes"]
                ],
            },
            "source_functions": tuple(dict.fromkeys((
                str(corpse["handler_function"]), str(corpse["predicate_function"]), str(corpse["effect_function"]),
            ))),
            "evidence_kind": "script-direct",
            "byte_offset": int(corpse["byte_offset"]),
        })

    rows.sort(key=lambda row: (int(row["building_id"]), int(row["ability_id"])))
    if len(building_spell_registrations) >= 10:
        covered = {(int(row["building_id"]), int(row["ability_id"])) for row in rows}
        registered = {(int(row["building_id"]), int(row["ability_id"])) for row in building_spell_registrations}
        if covered != registered:
            missing = sorted(registered - covered)
            extra = sorted(covered - registered)
            raise ValueError(f"building-spell semantic coverage mismatch; missing={missing} extra={extra}")
    return rows


def _decode_lua_short_string_contents(value: bytes) -> bytes:
    """Decode the escape subset used by W3P's generated short string literals."""
    simple_escapes = {
        ord("a"): 7,
        ord("b"): 8,
        ord("f"): 12,
        ord("n"): 10,
        ord("r"): 13,
        ord("t"): 9,
        ord("v"): 11,
        ord("\\"): ord("\\"),
        ord('"'): ord('"'),
        ord("'"): ord("'"),
    }
    result = bytearray()
    index = 0
    while index < len(value):
        current = value[index]
        if current != ord("\\"):
            result.append(current)
            index += 1
            continue
        if index + 1 >= len(value):
            raise ValueError("unterminated Lua short-string escape")
        escape = value[index + 1]
        if escape == ord("x"):
            if index + 3 >= len(value):
                raise ValueError("truncated Lua hex escape")
            try:
                result.append(int(value[index + 2:index + 4].decode("ascii"), 16))
            except ValueError as error:
                raise ValueError("invalid Lua hex escape") from error
            index += 4
            continue
        if escape in simple_escapes:
            result.append(simple_escapes[escape])
            index += 2
            continue
        if ord("0") <= escape <= ord("9"):
            end = index + 2
            while end < min(index + 4, len(value)) and ord("0") <= value[end] <= ord("9"):
                end += 1
            decimal = int(value[index + 1:end].decode("ascii"), 10)
            if decimal > 255:
                raise ValueError("Lua decimal escape exceeds one byte")
            result.append(decimal)
            index = end
            continue
        raise ValueError(f"unsupported Lua short-string escape: {chr(escape)!r}")
    return bytes(result)


def _decode_w3p_hex_escaped(value: bytes) -> bytes:
    """Decode the all-hex escaped form used by protected VM byte payloads."""
    decoded = _decode_lua_short_string_contents(value)
    if len(decoded) * 4 != len(value):
        raise ValueError("unexpected W3P protected hex-string encoding")
    return decoded


def _w3p_vm_static_strings(data: bytes, vm_index: int) -> list[str]:
    marker = f"_fr({vm_index},".encode("ascii")
    start = data.find(marker)
    if start < 0:
        raise ValueError(f"W3P VM block missing: {vm_index}")
    table_start = data.find(b"_s={", start)
    table_end = data.find(b"};_Y=", table_start)
    if table_start < 0 or table_end < 0:
        raise ValueError(f"W3P VM static-string table missing: {vm_index}")
    rows: list[str] = []
    for expression in data[table_start + len(b"_s={"):table_end].split(b";"):
        match = re.fullmatch(rb'"((?:\\.|[^"\\])*)"', expression)
        if match is None:
            raise ValueError(f"W3P VM {vm_index} has non-literal static string entry")
        rows.append(_decode_lua_short_string_contents(match.group(1)).decode("utf-8"))
    return rows


def _w3p_vm_global_expressions(data: bytes, vm_index: int) -> list[bytes]:
    marker = f"_fr({vm_index},".encode("ascii")
    start = data.find(marker)
    if start < 0:
        raise ValueError(f"W3P VM block missing: {vm_index}")
    table_start = data.find(b"_Y={", start)
    table_end = data.find(b"};_v=", table_start)
    if table_start < 0 or table_end < 0:
        raise ValueError(f"W3P VM global-name table missing: {vm_index}")
    return data[table_start + len(b"_Y={"):table_end].split(b";")


def _w3p_global_payload(expression: bytes) -> tuple[bytes, bool]:
    """Return the encrypted payload plus whether W3P's extra _a layer is applied."""
    match = re.search(rb'"((?:\\.|[^"\\])*)"', expression)
    if match is None:
        raise ValueError("W3P global-name expression has no short string literal")
    return _decode_lua_short_string_contents(match.group(1)), expression.startswith(b"_a(")


def _decode_w3p_hr_payload(payload: bytes, lane: int, seed: int) -> list[int]:
    """Mirror the visible top-level _hr transform without executing protected Lua."""
    state = (seed + len(payload) + lane * 17) & 0xFF
    decoded: list[int] = []
    for index, cipher in enumerate(payload, 1):
        key = ((state * state + seed * index) ^ (lane * 31 + index)) & 0xFF
        decoded.append(cipher ^ key)
        state = (cipher + state) & 0xFF
    return decoded


def _w3p_vm_integrity(payloads: Iterable[list[int]]) -> int:
    accumulator = 173
    rolling = 89
    position = 0
    for payload in payloads:
        for value in payload:
            position += 1
            accumulator = (accumulator + value * 11 + position * 17 + rolling % 37) % 4093
            rolling = (rolling ^ ((value * 13 + accumulator + position * 7) & 0xFF)) & 0xFF
            rolling = (rolling + ((accumulator * 3 + value + position) & 0xFF)) & 0xFF
    return accumulator * 257 + rolling


def _decode_w3p_string_payload(payload: bytes, multiplier: int, offset: int) -> bytes:
    """Mirror the visible _T/_L/_r/_j protected-string transform."""
    if len(payload) < 5:
        raise ValueError("W3P protected string payload is too short")
    if payload[0] == 1:
        key = payload[1] * 256 + payload[2]
        cipher = payload[3:]
    else:
        try:
            key = int(payload[1:5].decode("ascii"), 16)
            cipher = bytes.fromhex(payload[5:].decode("ascii"))
        except ValueError as error:
            raise ValueError("W3P protected string has invalid hex key/ciphertext") from error

    modulus = 32749
    key %= modulus
    first = ((key * multiplier + offset + 25) % modulus) + 1
    second = ((key + multiplier * 18) % modulus) + 1
    third = ((first * second + 17) % modulus) + 1
    decoded = bytearray()
    for value in cipher:
        previous_first, previous_second = first, second
        first = second
        second = third
        third = (previous_second * third + previous_first + 17) % modulus
        decoded.append((value - third) & 0xFF)
    return bytes(decoded)


def _decode_w3p_global_name(expression: bytes, multiplier: int, offset: int) -> str:
    payload, extra_shift = _w3p_global_payload(expression)
    decoded = _decode_w3p_string_payload(payload, multiplier, offset)
    if extra_shift:
        decoded = bytes((value - offset) & 0xFF for value in decoded)
    try:
        return decoded.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError("W3P global name did not decode as UTF-8") from error


def _decode_w3p_vm_program(data: bytes, vm_index: int) -> dict[str, object]:
    """Statically decode one W3P VM instruction stream.

    This only reverses the visible byte transforms and instruction framing. It
    does not evaluate the protected VM or invoke any map/runtime functions.
    """
    marker = f"_fr({vm_index},".encode("ascii")
    start = data.find(marker)
    if start < 0:
        raise ValueError(f"W3P VM block missing: {vm_index}")
    wrapper_marker = f"function ".encode("ascii")
    end = data.find(wrapper_marker, start)
    if end < 0:
        raise ValueError(f"W3P VM wrapper missing after block: {vm_index}")
    block = data[start:end]

    payload_matches = re.findall(rb'_hr\("((?:\\x[0-9a-fA-F]{2})+)",([123])\)', block)
    if len(payload_matches) < 3:
        raise ValueError(f"W3P VM block {vm_index} has fewer than three encoded payloads")
    encoded_payloads = [_decode_w3p_hex_escaped(value) for value, _lane in payload_matches[:3]]

    integrity_match = re.search(rb"_N=([0-9]+)", block)
    mode_match = re.search(rb"_J=([0-9]+)", block)
    if integrity_match is None or mode_match is None:
        raise ValueError(f"W3P VM block {vm_index} is missing integrity/mode metadata")
    expected_integrity = int(integrity_match.group(1))
    operand_mode = int(mode_match.group(1))

    candidates: list[tuple[int, list[list[int]]]] = []
    for seed in range(256):
        decoded = [
            _decode_w3p_hr_payload(payload, lane, seed)
            for lane, payload in enumerate(encoded_payloads, 1)
        ]
        if _w3p_vm_integrity(decoded) == expected_integrity:
            candidates.append((seed, decoded))
    if len(candidates) != 1:
        raise ValueError(f"W3P VM block {vm_index} hidden-byte seed is not unique: {len(candidates)} candidates")
    hidden_seed, (opcodes_encrypted, opcode_keys, remap_bytes) = candidates[0]

    interpreter_at = data.find(b"local Er={")
    interpreter_end = data.find(b"}local Hr=", interpreter_at)
    if interpreter_at < 0 or interpreter_end < 0:
        raise ValueError("W3P VM opcode-width table missing")
    widths = [int(value) for value in data[interpreter_at + len(b"local Er={"):interpreter_end].split(b";")]
    handled_opcodes = {
        int(value)
        for value in re.findall(rb"(?:if|elseif)\(_S==([0-9]+)\)", data[interpreter_end:start])
    }

    opcode_map: dict[int, int] = {}
    for offset in range(0, len(remap_bytes), 2):
        if offset + 1 >= len(remap_bytes):
            raise ValueError(f"W3P VM block {vm_index} has odd remap table length")
        opcode_map[remap_bytes[offset]] = (((remap_bytes[offset + 1] * 139) + 96) & 0xFF) + 1

    xor_candidates: list[int] = []
    for xor_byte in range(256):
        cursor = 0
        valid = True
        while cursor < len(opcodes_encrypted):
            encrypted_opcode = opcodes_encrypted[cursor]
            key_byte = opcode_keys[cursor]
            opcode = opcode_map.get(encrypted_opcode ^ key_byte ^ xor_byte)
            if opcode is None or opcode not in handled_opcodes or opcode > len(widths):
                valid = False
                break
            cursor += 1 + widths[opcode - 1]
        if valid and cursor == len(opcodes_encrypted):
            xor_candidates.append(xor_byte)
    if len(xor_candidates) != 1:
        raise ValueError(f"W3P VM block {vm_index} opcode xor byte is not unique: {len(xor_candidates)} candidates")
    xor_byte = xor_candidates[0]

    if operand_mode == 1:
        operand_transform = lambda value: (value - xor_byte) & 0xFF
    elif operand_mode == 2:
        rotate = ((xor_byte << 1) | (xor_byte >> 7)) & 0xFF
        operand_transform = lambda value: value ^ rotate
    elif operand_mode == 3:
        operand_transform = lambda value: (value + xor_byte) & 0xFF
    elif operand_mode == 4:
        rotate = ((xor_byte << 1) | (xor_byte >> 7)) & 0xFF
        operand_transform = lambda value: (value - rotate) & 0xFF
    elif operand_mode == 5:
        operand_transform = lambda value: ((((value ^ xor_byte) << 4) | ((value ^ xor_byte) >> 4)) & 0xFF)
    elif operand_mode == 6:
        inverse = (~xor_byte) & 0xFF
        operand_transform = lambda value: value ^ inverse
    else:
        operand_transform = lambda value: value ^ xor_byte

    raw_jump_last = {240, 10, 124, 131}
    instructions: list[dict[str, object]] = []
    cursor = 0
    while cursor < len(opcodes_encrypted):
        pc = cursor
        opcode = opcode_map[opcodes_encrypted[cursor] ^ opcode_keys[cursor] ^ xor_byte]
        width = widths[opcode - 1]
        cursor += 1
        operands: list[int] = []
        for operand_index in range(width):
            value = opcodes_encrypted[cursor + operand_index]
            if opcode in raw_jump_last and operand_index == width - 1:
                operands.append(value)
            else:
                operands.append(operand_transform(value))
        cursor += width
        instructions.append({"pc": pc + 1, "opcode": opcode, "operands": operands})

    return {
        "vm_index": vm_index,
        "hidden_seed": hidden_seed,
        "opcode_xor_byte": xor_byte,
        "operand_mode": operand_mode,
        "integrity": expected_integrity,
        "instructions": instructions,
    }


def _extract_protected_perk_registry_audit(
    data: bytes,
    functions: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Inventory authored draft perks without guessing protected registry call targets.

    The normal draft runtime calls initPerks__w3p_vmProtect. Its protected VM
    bytecode is decoded statically from the visible W3P transforms, then each
    three-argument addRegisteredPerk call is tied to the exact readable factory
    that produced its first argument and the exact numeric registry slot.
    """
    functions_by_name = {str(row["name"]): row for row in functions}
    if "initPerks__w3p_vmProtect" not in functions_by_name:
        return []

    def source(name: str) -> tuple[int, bytes]:
        row = functions_by_name.get(name)
        if row is None:
            raise ValueError(f"protected-perk audit source function missing: {name}")
        start = int(row["start"])
        end = int(row["end"])
        return start, data[start:end]

    initializer = "initPerks__w3p_vmProtect"
    _initializer_start, initializer_source = source(initializer)
    if initializer_source != b"function initPerks__w3p_vmProtect()return _qr(65)end":
        raise ValueError("protected perk initializer wrapper changed")

    live_callers = [
        "CallbackSingle_doAfter_DraftOrchestrator_call_doAfter_DraftOrchestrator",
        "restartDraftAfterRoundEnd",
        "startDraft__w3p_vmProtect",
    ]
    for caller in live_callers:
        _caller_start, caller_source = source(caller)
        if b"initPerks__w3p_vmProtect()" not in caller_source:
            raise ValueError(f"normal draft runtime no longer calls protected perk initializer: {caller}")

    vm_start = data.find(b"_fr(65,")
    vm_end = data.find(b"function initPerks__w3p_vmProtect", vm_start)
    if vm_start < 0 or vm_end < 0:
        raise ValueError("protected perk initializer VM block missing")
    vm_source = data[vm_start:vm_end]
    slot_sequence = b'"DraftPerkRegistry_perkRegistryRollingToken";' + b";".join(
        f'"{index}"'.encode("ascii") for index in range(19)
    )
    if slot_sequence not in vm_source:
        raise ValueError("protected perk registry slot sequence changed")

    # These two W3P string-transform parameters are recovered from the visible
    # decoder using the exact readable setReminderAbility anchor. Keep them
    # asserted against two independent protected globals so protection drift
    # fails loudly rather than silently mis-decoding VM names.
    string_multiplier = 11351
    string_offset = 1106
    global_expressions = _w3p_vm_global_expressions(data, 65)
    global_names = [
        _decode_w3p_global_name(expression, string_multiplier, string_offset)
        for expression in global_expressions
    ]
    if global_names[14] != "DraftPerk_DraftPerk_setReminderAbility":
        raise ValueError("protected perk VM string decoder no longer resolves setReminderAbility anchor")
    if global_names[16] != "addRegisteredPerk__w3p_vmProtect":
        raise ValueError("protected perk VM string decoder no longer resolves addRegisteredPerk anchor")

    program = list(_decode_w3p_vm_program(data, 65)["instructions"])
    static_strings = _w3p_vm_static_strings(data, 65)
    local_factories: dict[int, tuple[int, str]] = {}
    for current, following in zip(program, program[1:]):
        if (
            int(current["opcode"]) == 42
            and list(current["operands"])[1:] == [1]
            and int(following["opcode"]) == 24
        ):
            global_index = int(list(current["operands"])[0])
            factory_name = global_names[global_index - 1]
            if factory_name.startswith("create") and factory_name.endswith("Perk"):
                local_factories[int(list(following["operands"])[0])] = (global_index, factory_name)

    registrations: dict[str, dict[str, object]] = {}
    for index in range(len(program) - 4):
        load_registry, perk_value, load_slot, load_signature, invoke = program[index:index + 5]
        if not (
            int(load_registry["opcode"]) == 218
            and global_names[int(list(load_registry["operands"])[0]) - 1] == "addRegisteredPerk__w3p_vmProtect"
            and int(load_slot["opcode"]) == 144
            and int(load_signature["opcode"]) == 218
            and int(invoke["opcode"]) == 98
            and list(invoke["operands"]) == [48]
        ):
            continue

        if int(perk_value["opcode"]) == 253:
            local_index = int(list(perk_value["operands"])[0])
            factory = local_factories.get(local_index)
            if factory is None:
                raise ValueError(f"protected perk registry call uses unresolved local factory: {local_index}")
            factory_global_index, factory_name = factory
            factory_evidence = "factory-result-via-local"
        elif int(perk_value["opcode"]) == 42 and list(perk_value["operands"])[1:] == [1]:
            factory_global_index = int(list(perk_value["operands"])[0])
            factory_name = global_names[factory_global_index - 1]
            factory_evidence = "factory-result-direct-on-stack"
            if not (factory_name.startswith("create") and factory_name.endswith("Perk")):
                raise ValueError(f"protected perk direct registry argument is not a perk factory: {factory_name}")
        else:
            continue

        slot_string_index = int(list(load_slot["operands"])[0])
        try:
            registry_slot = int(static_strings[slot_string_index - 1])
        except (IndexError, ValueError) as error:
            raise ValueError(f"protected perk registry slot is not numeric: static index {slot_string_index}") from error
        signature_global_index = int(list(load_signature["operands"])[0])
        signature_global_name = global_names[signature_global_index - 1]
        if factory_name in registrations:
            raise ValueError(f"protected perk factory registered multiple times: {factory_name}")
        registrations[factory_name] = {
            "protected_registry_slot": registry_slot,
            "factory_global_index": factory_global_index,
            "signature_global_index": signature_global_index,
            "signature_global_name": signature_global_name,
            "factory_value_evidence": factory_evidence,
            "registration_pc": int(load_registry["pc"]),
        }

    if len(registrations) != 19:
        raise ValueError(f"protected perk VM registration call count changed: {len(registrations)}")
    if {int(row["protected_registry_slot"]) for row in registrations.values()} != set(range(19)):
        raise ValueError("protected perk VM registry slots are not exactly 0..18")

    damage_listener_names = {
        str(row["name"])
        for row in functions
        if str(row["name"]).startswith("DamageListener_perkListenDamage_")
    }
    factory_names = sorted(
        str(row["name"])
        for row in functions
        if str(row["name"]).startswith("create") and str(row["name"]).endswith("Perk")
    )
    if len(factory_names) != 19:
        raise ValueError(f"authored draft perk factory count changed: {len(factory_names)}")

    rows: list[dict[str, object]] = []
    seen_ids: set[str] = set()
    for factory_name in factory_names:
        factory_start, factory_source = source(factory_name)
        marker = b'DraftPerk_new_DraftPerk("'
        marker_at = factory_source.find(marker)
        if marker_at < 0:
            raise ValueError(f"draft perk factory has no direct DraftPerk constructor: {factory_name}")
        cursor = marker_at + len(marker)
        id_end = factory_source.find(b'"', cursor)
        if id_end < 0:
            raise ValueError(f"draft perk factory id literal is unterminated: {factory_name}")
        perk_id = factory_source[cursor:id_end].decode("utf-8")
        name_marker = b',"'
        name_start = factory_source.find(name_marker, id_end)
        if name_start < 0:
            raise ValueError(f"draft perk factory name literal is missing: {factory_name}")
        name_start += len(name_marker)
        name_end = factory_source.find(b'"', name_start)
        if name_end < 0:
            raise ValueError(f"draft perk factory name literal is unterminated: {factory_name}")
        perk_name = factory_source[name_start:name_end].decode("utf-8")
        if perk_id in seen_ids:
            raise ValueError(f"duplicate authored draft perk id: {perk_id}")
        seen_ids.add(perk_id)

        stem = factory_name[len("create"):-len("Perk")]
        matching_listeners = sorted(name for name in damage_listener_names if f"Perk{stem}" in name)
        if len(matching_listeners) > 1:
            raise ValueError(f"draft perk has multiple damage listeners: {factory_name}: {matching_listeners}")
        registration = registrations.get(factory_name)
        if registration is None:
            raise ValueError(f"readable draft perk factory is absent from protected registry VM: {factory_name}")
        expected_slot = int(perk_id.removeprefix("perk_")) - 1
        if int(registration["protected_registry_slot"]) != expected_slot:
            raise ValueError(
                f"protected perk registry slot disagrees with readable perk id: {perk_id} -> "
                f"{registration['protected_registry_slot']}"
            )
        rows.append({
            "perk_id": perk_id,
            "perk_name": perk_name,
            "factory_function": factory_name,
            "damage_listener_function": matching_listeners[0] if matching_listeners else "",
            "protected_initializer_function": initializer,
            "protected_vm_index": 65,
            "protected_registry_slot_count": 19,
            "protected_registry_slot": int(registration["protected_registry_slot"]),
            "factory_global_index": int(registration["factory_global_index"]),
            "signature_global_index": int(registration["signature_global_index"]),
            "signature_global_name": str(registration["signature_global_name"]),
            "factory_value_evidence": str(registration["factory_value_evidence"]),
            "registration_pc": int(registration["registration_pc"]),
            "normal_draft_initializer_callers": live_callers,
            "runtime_registry_path_status": "reachable-protected-initializer",
            "individual_factory_registration_status": "exact-protected-vm-call",
            "individual_factory_registration_proven": True,
            "evidence_kind": "exact-readable-factory-plus-statically-decoded-protected-vm-registration-call",
            "byte_offset": factory_start,
        })

    expected_ids = {f"perk_{index:02d}" for index in range(1, 20)}
    if seen_ids != expected_ids:
        raise ValueError(f"authored draft perk id sequence changed: {sorted(seen_ids)}")
    rows.sort(key=lambda row: str(row["perk_id"]))
    return rows


def _extract_perk_mechanics(
    data: bytes,
    functions: list[dict[str, object]],
    protected_perk_registry_audit: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Normalize every proven-live draft perk into importer-facing runtime semantics."""
    if not protected_perk_registry_audit:
        return []

    functions_by_name = {str(row["name"]): row for row in functions}
    audit_by_id = {str(row["perk_id"]): row for row in protected_perk_registry_audit}

    def source(name: str, fragments: Iterable[bytes] = ()) -> tuple[int, bytes]:
        row = functions_by_name.get(name)
        if row is None:
            raise ValueError(f"perk mechanic source function missing: {name}")
        start = int(row["start"])
        body = data[start:int(row["end"])]
        for fragment in fragments:
            if fragment not in body:
                raise ValueError(f"perk mechanic source changed: {name}: missing {fragment!r}")
        return start, body

    def require_global_fragment(fragment: bytes) -> None:
        if data.count(fragment) != 1:
            raise ValueError(f"perk mechanic global constant changed or became ambiguous: {fragment!r}")

    for fragment in (
        b"A6=0.5", b"z6=(-0.3)", b"evb=0.12", b"dvb=0.09",
        b"P2=0.8", b"Q2=4", b"R2=1231251027",
        b"M4=__wurst_ensureInt(2016423986)", b"L4=__wurst_ensureInt(1229795377)", b"K4=0.6",
        b"J4=6", b"I4=800.", b"H4=260.", b"G4=750.", b"F4=120.", b"E4=25", b"D4=300.", b"C4=12",
        b"B4[0]=600.", b"B4[1]=450.", b"B4[2]=300.", b"B4[3]=150.", b"B4[4]=120.", b"B4[5]=60.",
        b"U3=__wurst_ensureInt(1229795378)", b"T3=1747990577", b"S3=900", b"R3=45",
        b"N3=0.85", b"M3=1.25",
        b"B5=1.12", b"A5=0.93", b"z5=0.93",
        b"H3=3", b"G3=(-2)", b"J3=__wurst_ensureInt(1095577656)", b"I3=__wurst_ensureInt(1095577657)",
        b"B3=90", b"A3=0.88",
        b"m6=__wurst_ensureInt(1095577697)", b"k6=__wurst_ensureInt(1095577698)", b"e6=__wurst_ensureInt(1095577699)",
        b"V5=1.15", b"U5=1.15", b"T5=1.10", b"S5=0.95", b"R5=0.95",
        b"Q5=__wurst_ensureInt(1095577700)", b"P5=__wurst_ensureInt(1095577701)", b"K5=0.20", b"J5=(-0.05)",
        b"Z2=7.0", b"Y2=0.90",
        b"q6=1.20", b"o6=0.75",
        b"q5=0.75", b"o5=(-50)",
        b"m5=1.20", b"k5=0.90",
        b"L3=1.18", b"K3=0.85",
        b"o3=3.0", b"T2=1.15", b"L2=45.", b"M2=(-15)",
        b"R4=0.85", b"S4=1.18", b"T4=0.85", b"U4=1.25",
        b"V4=0.70", b"W4=0.35", b"Q4=10.",
        b"e5=__wurst_ensureInt(1095577654)", b"Z4=__wurst_ensureInt(1095577655)",
    ):
        require_global_fragment(fragment)

    rows: list[dict[str, object]] = []

    def add(
        perk_id: str,
        mechanic_kind: str,
        trigger: str,
        parameters: dict[str, object],
        source_functions: list[tuple[str, tuple[bytes, ...]]],
        related_rawcode_ids: Iterable[int] = (),
        evidence_kind: str = "exact-proven-perk-registration-plus-readable-runtime-handlers",
    ) -> None:
        audit = audit_by_id.get(perk_id)
        if audit is None or not bool(audit["individual_factory_registration_proven"]):
            raise ValueError(f"perk mechanic lacks proven protected registration: {perk_id}")
        offsets: list[int] = []
        names: list[str] = [str(audit["factory_function"])]
        factory_start, _factory_source = source(str(audit["factory_function"]))
        offsets.append(factory_start)
        for name, fragments in source_functions:
            start, _body = source(name, fragments)
            offsets.append(start)
            names.append(name)
        rows.append({
            "perk_id": perk_id,
            "perk_name": str(audit["perk_name"]),
            "protected_registry_slot": int(audit["protected_registry_slot"]),
            "mechanic_kind": mechanic_kind,
            "trigger": trigger,
            "parameters": parameters,
            "related_rawcode_ids": list(related_rawcode_ids),
            "source_functions": list(dict.fromkeys(names)),
            "evidence_kind": evidence_kind,
            "byte_offset": min(offsets),
        })

    add(
        "perk_01",
        "caster-mana-overcharge-with-special-building-penalty",
        "owned-unit-indexing-plus-owned-construction-finish",
        {
            "unit_scope": "non-structure units with positive max mana",
            "unit_spawn_mana": "set-current-mana-to-max",
            "unit_mana_regen_delta": 0.5,
            "special_building_income_factors": [0.12, 0.09],
            "special_building_mana_regen_delta": -0.3,
            "special_building_penalty_trigger": "owned-construction-finish",
            "cleanup_reversal_present": False,
        },
        [
            ("PerkIndexHandler_perkOnIndex_setCleanup_PerkArcaneOvercharge_call_perkOnIndex_setCleanup_PerkArcaneOvercharge", (
                b"applyArcaneOverchargeTo(abn)",
            )),
            ("applyArcaneOverchargeTo", (
                b"UNIT_TYPE_STRUCTURE", b"UNIT_STATE_MAX_MANA", b"UNIT_STATE_MANA", b"unit_getMaxMana(M4q)",
                b"UNIT_RF_MANA_REGENERATION", b"+A6",
            )),
            ("EventListener_perkListen_setCleanup_PerkArcaneOvercharge_onEvent_perkListen_setCleanup_PerkArcaneOvercharge", (
                b"GetConstructedStructure()", b"applyArcaneOverchargeSpecialPenalty",
            )),
            ("applyArcaneOverchargeSpecialPenalty", (
                b"unit_getOwner(V4q)==U4q", b"CFBuilding_getBuildingById", b"CFBuilding_incomeFactor_field==evb",
                b"CFBuilding_incomeFactor_field==dvb", b"UNIT_RF_MANA_REGENERATION", b"+z6",
            )),
        ],
    )

    add(
        "perk_02",
        "portable-cloud-item-with-siege-base-damage-penalty",
        "perk-activation-round-start-and-owned-unit-indexing",
        {
            "builder_item_id": 1231251027,
            "builder_item_charges": 4,
            "round_start_refills_or_reissues_item": True,
            "item_effect_ability_id": 1097033299,
            "siege_unit_base_damage_factor": 0.8,
            "siege_detection": "spawn-building CFBuilding_isSiege_field",
            "affected_weapon_indices": [0, 1],
            "rounding": "real_toInt(value*factor+0.5)",
            "cleanup_disables_future_application_without_reversing_existing_units": True,
        },
        [
            ("giveCloudStaffToBuilder", (
                b"unit_getItemById(xdr,R2)", b"unit_addProtectedItemById(xdr,R2)", b"SetItemCharges(zdr,Adr)", b"Adr=Q2",
            )),
            ("Action_watch_PerkStormchaser_run_watch_PerkStormchaser", (
                b"O2[lhn]", b"giveCloudStaffToBuilder(V1[lhn])",
            )),
            ("PerkIndexHandler_perkOnIndex_setCleanup_PerkStormchaser_call_perkOnIndex_setCleanup_PerkStormchaser", (
                b"getSpawnBuilding(xhn)", b"unit_getOwner(yhn)==whn.plr", b"CFBuilding_isSiege_field",
                b"applyStormchaserSiegePenalty(xhn)",
            )),
            ("applyStormchaserSiegePenalty", (
                b"unit_getBaseDamage(Edr,0)", b"unit_getBaseDamage(Edr,1)", b"P2", b"BlzSetUnitBaseDamage",
            )),
            ("PerkCleanupFunc_setCleanup_PerkStormchaser_call_setCleanup_PerkStormchaser", (
                b"O2[player_getId(Chn)]=false",
            )),
        ],
        related_rawcode_ids=(1231251027, 1097033299),
    )

    add(
        "perk_03",
        "persistent-builder-bird-with-food-slot-decay-and-time-scaling-damage",
        "perk-activation-round-start-plus-0.6-second-periodic-controller",
        {
            "bird_unit_id": 2016423986,
            "bird_food_item_id": 1229795377,
            "inventory_slots_filled": 6,
            "food_intervals_seconds": [600, 450, 300, 150, 120, 60],
            "food_consumption_times_from_round_start_seconds": [600, 1050, 1350, 1500, 1620, 1680],
            "bird_base_damage_initial": 25,
            "bird_base_damage_gain": 12,
            "bird_damage_step_seconds": 300,
            "bird_base_damage_formula": "25 + floor(elapsed-round-seconds/300)*12",
            "bird_damage_weapon_index": 0,
            "controller_period_seconds": 0.6,
            "target_search_radius_from_builder": 750.0,
            "target_filter": "alive enemy combat sapper and not invulnerable; nearest to builder",
            "builder_anchor_forward_distance": 120.0,
            "follow_reissue_distance": 260.0,
            "teleport_to_anchor_distance": 800.0,
            "bird_locust_removed_on_spawn": True,
            "bird_invulnerable_ability_id": 1098282348,
            "round_start_recreates_bird_and_resets_food_schedule": True,
        },
        [
            ("resetFeatheredFriendRoundStateFor", (
                b"RemoveUnit(d9q)", b"Z3[c9q]=(-1)", b"Y3[c9q]=0", b"X3[c9q]=0.",
            )),
            ("spawnFeatheredFriendBird", (
                b"createUnit(h_q,M4", b"unit_removeAbility(j_q,1097625443)", b"addProtectedAbility(k_q,1098282348)",
                b"unit_makeAbilityPermanent(j_q,1098282348,true)",
            )),
            ("applyFeatheredFriendDamageScaling", (
                b"getElapsedGameTime()-V3", b"real_toInt((h9q/D4))", b"E4+(i9q*C4)", b"BlzSetUnitBaseDamage(k9q,l9q,0)",
            )),
            ("ForGroupCallback_forUnitsInRange_PerkFeatheredFriend_callback_forUnitsInRange_PerkFeatheredFriend", (
                b"unit_isAlive(ven)", b"unit_isEnemyOf(ven,uen.plr)", b"isCombatSapper(ven)", b"not unit_isInvulnerable(ven)",
                b"vec2_distanceToSq", b"bestTarget",
            )),
            ("updateFeatheredFriendBirdFoodPenalty", (
                b"giveFeatheredFriendBirdFood(z_q)", b"removeOneFeatheredFriendBirdFood(z_q)",
                b"nextFeatheredFriendFoodDelay", b"Y3[A_q]=(__wurst_ensureInt(Y3[A_q])+1)",
            )),
            ("controlFeatheredFriendBird", (
                b"D9q>(I4*I4)", b"D9q>(H4*H4)", b"findFeatheredFriendTarget", b"featheredFriendAnchor",
            )),
            ("CallbackPeriodic_perkPeriodic_PerkFeatheredFriend_call_perkPeriodic_PerkFeatheredFriend", (
                b"updateFeatheredFriendBirdFoodPenalty", b"ensureFeatheredFriendBird", b"applyFeatheredFriendDamageScaling",
                b"controlFeatheredFriendBird",
            )),
            ("Action_watch_PerkFeatheredFriend_run_watch_PerkFeatheredFriend", (
                b"V3=getElapsedGameTime()", b"resetFeatheredFriendRoundStateFor", b"nextFeatheredFriendFoodDelay", b"activateFeatheredFriendFor",
            )),
        ],
        related_rawcode_ids=(2016423986, 1229795377, 1097625443, 1098282348),
    )

    add(
        "perk_04",
        "disable-y-tier-and-grant-one-tiny-watch-tower-item",
        "perk-activation-and-round-start",
        {
            "disabled_building_scope": "all CFBuilding entries in BuildingTier_index (Y-tier)",
            "disable_method": "SetPlayerTechMaxAllowed(building-id,0)",
            "builder_item_id": 1229795378,
            "builder_item_granted_if_missing": True,
            "tiny_tower_unit_id": 1747990577,
            "tiny_tower_build_ability_id": 1095577652,
            "initializer_related_multishot_ability_id": 1095577653,
            "initializer_related_multishot_runtime_attachment_proven": False,
            "display_attack_range": 900,
            "display_average_dps": 45,
            "round_start_reenforces_tech_caps_and_reissues_missing_item": True,
        },
        [
            ("LLItrClosure_forEach_PerkFortifiedOutpost_run_forEach_PerkFortifiedOutpost", (
                b"SetPlayerTechMaxAllowed(Men.plr,Nen.CFBuilding_buildingId,0)",
            )),
            ("enforceFortifiedOutpostTechAvailability", (
                b"LinkedList_LinkedList_forEach(btb[Vrb.BuildingTier_index],bar)",
            )),
            ("giveTinyOutpostItemToBuilder", (
                b"unit_getItemById(dar,U3)", b"unit_addProtectedItemById(dar,U3)",
            )),
            ("Action_watch_PerkFortifiedOutpost_run_watch_PerkFortifiedOutpost", (
                b"enforceFortifiedOutpostTechAvailability(Ven)", b"giveTinyOutpostItemToBuilder(Ven)",
            )),
        ],
        related_rawcode_ids=(1229795378, 1747990577, 1095577652, 1095577653),
    )

    add(
        "perk_05",
        "glass-cannon-max-hp-and-primary-base-damage-scaling",
        "owned-unit-indexing",
        {
            "unit_scope": "non-structure units",
            "max_hp_factor": 0.85,
            "max_hp_rounding": "real_toInt(old-max-hp*0.85+0.5)",
            "current_life_after_application": "new-max-hp",
            "primary_weapon_base_damage_factor": 1.25,
            "primary_weapon_rounding": "real_toInt(old-base-damage*1.25+0.5)",
            "secondary_weapon_modified": False,
            "cleanup_reversal_present": False,
        },
        [
            ("PerkIndexHandler_perkOnIndex_setCleanup_PerkGlassCannon_call_perkOnIndex_setCleanup_PerkGlassCannon", (
                b"applyGlassCannonTo(kfn)",
            )),
            ("applyGlassCannonTo", (
                b"UNIT_TYPE_STRUCTURE", b"BlzGetUnitMaxHP", b"lar*N3", b"SetWidgetLife(oar,par)",
                b"unit_getBaseDamage(kar,0)", b"nar*M3", b"BlzSetUnitBaseDamage(qar,rar,0)",
            )),
        ],
    )

    add(
        "perk_10",
        "attack-damage-target-type-tradeoff",
        "positive-attack-damage-event-from-perk-owner-to-enemy",
        {
            "attack_damage_event_type": 0,
            "base_damage_factor": 1.0,
            "structure_damage_factor": 1.20,
            "flying_damage_factor": 0.75,
            "factors_multiply_when_target_matches_both": True,
            "source_requires_perk_owner": True,
            "target_requires_enemy": True,
            "source_structure_is_not_excluded_by_script": True,
            "modifies_current_damage_instance": True,
        },
        [
            ("DamageListener_perkListenDamage_PerkBreachingDoctrine_onEvent_perkListenDamage_PerkBreachingDoctrine", (
                b"DamageEvent_getType()==0", b"DamageEvent_getAmount()>0.", b"UNIT_TYPE_STRUCTURE",
                b"UNIT_TYPE_FLYING", b"q6", b"o6", b"DamageInstance_DamageInstance_setAmount",
            )),
        ],
    )

    add(
        "perk_07",
        "attack-damage-target-type-tradeoff",
        "positive-attack-damage-event-from-perk-owner-to-enemy",
        {
            "attack_damage_event_type": 0,
            "base_damage_factor": 1.0,
            "flying_damage_factor": 1.18,
            "structure_damage_factor": 0.85,
            "factors_multiply_when_target_matches_both": True,
            "source_requires_perk_owner": True,
            "target_requires_enemy": True,
            "source_structure_is_not_excluded_by_script": True,
            "modifies_current_damage_instance": True,
        },
        [
            ("DamageListener_perkListenDamage_setCleanup_PerkGroundControl_onEvent_perkListenDamage_setCleanup_PerkGroundControl", (
                b"DamageEvent_getType()==0", b"DamageEvent_getAmount()>0.", b"UNIT_TYPE_FLYING",
                b"UNIT_TYPE_STRUCTURE", b"L3", b"K3", b"DamageInstance_DamageInstance_setAmount",
            )),
        ],
    )

    add(
        "perk_18",
        "bidirectional-spell-damage-amplification",
        "positive-non-attack-damage-event",
        {
            "attack_damage_event_type_excluded": 0,
            "per_owned_source_factor": 1.15,
            "per_owned_target_factor": 1.15,
            "owned_source_and_owned_target_factor": 1.3225,
            "enemy_relationship_not_required": True,
            "source_and_target_factors_multiply": True,
            "modifies_current_damage_instance": True,
        },
        [
            ("isSpellsEdgeSpellDamage", (b"not(DamageEvent_getType()==0)", b"DamageEvent_getAmount()>0.")),
            ("DamageListener_perkListenDamage_PerkSpellsEdge_onEvent_perkListenDamage_PerkSpellsEdge", (
                b"isSpellsEdgeSpellDamage()", b"unit_getOwner(ehn)==dhn.plr", b"unit_getOwner(fhn)==dhn.plr",
                b"T2", b"DamageInstance_DamageInstance_setAmount",
            )),
        ],
    )

    add(
        "perk_19",
        "cage-conditional-spell-damage-tradeoff",
        "positive-non-attack-damage-event-from-perk-owner-to-enemy",
        {
            "attack_damage_event_type_excluded": 0,
            "caged_target_damage_factor": 1.20,
            "uncaged_target_damage_factor": 0.90,
            "cage_test_owner": "damage-target-owner",
            "cage_test": "vec2_isCagedAt(target-position,target-owner)",
            "source_requires_perk_owner": True,
            "target_requires_enemy": True,
            "modifies_current_damage_instance": True,
        },
        [
            ("getContainmentFocusSpellDamageFactor", (b"if g8q then h8q=m5 else h8q=k5 end",)),
            ("isContainmentFocusSpellDamage", (b"not(DamageEvent_getType()==0)", b"DamageEvent_getAmount()>0.")),
            ("isTargetCagedForContainmentFocus", (b"vec2_isCagedAt(unit_getPos(i8q),unit_getOwner(i8q))",)),
            ("DamageListener_perkListenDamage_PerkContainmentFocus_onEvent_perkListenDamage_PerkContainmentFocus", (
                b"isContainmentFocusSpellDamage()", b"unit_getOwner(Ndn)==Mdn.plr", b"unit_isEnemyOf(Odn,Mdn.plr)",
                b"getContainmentFocusSpellDamageFactor", b"DamageInstance_DamageInstance_setAmount",
            )),
        ],
    )

    add(
        "perk_16",
        "mana-shield-damage-absorption",
        "positive-damage-event-targeting-owned-eligible-unit",
        {
            "damage_per_mana": 3.0,
            "absorb_formula": "min(current_mana*3,damage_amount)",
            "mana_spent_formula": "absorbed_damage/3",
            "remaining_damage_formula": "damage_amount-absorbed_damage",
            "eligible_requires_combat_sapper": True,
            "eligible_excludes_peon": True,
            "eligible_excludes_invulnerable": True,
            "eligible_requires_positive_max_mana": True,
            "requires_positive_current_mana": True,
            "applies_to_attack_and_non_attack_damage": True,
            "enemy_relationship_not_required": True,
            "modifies_current_damage_instance": True,
        },
        [
            ("isManaShieldEligible", (
                b"isCombatSapper(hcr)", b"UNIT_TYPE_PEON", b"BlzIsUnitInvulnerable",
                b"UNIT_STATE_MAX_MANA", b">0.",
            )),
            ("applyManaShield", (
                b"DamageEvent_getAmount()", b"unit_getMana(icr)", b"min1((kcr*o3),jcr)",
                b"(-(lcr/o3))", b"UNIT_STATE_MANA", b"DamageInstance_DamageInstance_setAmount",
            )),
            ("DamageListener_perkListenDamage_PerkManaLeak_onEvent_perkListenDamage_PerkManaLeak", (
                b"DamageEvent_getTarget()", b"unit_getOwner(Agn)==zgn.plr", b"applyManaShield(Agn)",
            )),
        ],
    )

    add(
        "perk_08",
        "caged-spell-resistance-and-base-damage-penalty",
        "owned-unit-indexing-plus-positive-non-attack-damage-event",
        {
            "spell_damage_factor_while_currently_caged": 0.75,
            "spell_damage_reduction_percent": 25,
            "base_damage_delta_on_qualifying_index": -50,
            "base_damage_floor": 0,
            "base_damage_weapon_indices": [0, 1],
            "indexed_unit_qualifies_if": "(has-cage-data-at-position and currently-caged-by-perk-owner) or spawn-building-is-cage",
            "damage_listener_cage_test": "vec2_isCagedAt(target-position,perk-owner)",
            "damage_listener_excludes_attack_damage_event_type": 0,
            "damage_listener_requires_positive_damage": True,
            "modifies_current_damage_instance": True,
        },
        [
            ("PerkIndexHandler_perkOnIndex_setCleanup_PerkCagedAegis_call_perkOnIndex_setCleanup_PerkCagedAegis", (
                b"isCagedUnitForPerk", b"getSpawnBuilding", b"applyCagedDamagePenalty",
            )),
            ("applyCagedDamagePenalty", (
                b"unit_getBaseDamage", b"P7q+o5", b"Q7q+o5", b"BlzSetUnitBaseDamage",
            )),
            ("isCagedUnitForPerk", (
                b"hasCageDataAt(V7q,Y7q)", b"vec2_isCagedAt(Y7q,V7q)", b"unit_isCageBuilding(W7q)",
            )),
            ("DamageListener_perkListenDamage_setCleanup_PerkCagedAegis_onEvent_perkListenDamage_setCleanup_PerkCagedAegis", (
                b"not(DamageEvent_getType()==0)", b"DamageEvent_getAmount()>0.", b"vec2_isCagedAt",
                b"getCagedAegisSpellDamageFactor()", b"DamageInstance_DamageInstance_setAmount",
            )),
            ("getCagedAegisSpellDamageFactor", (b"return q5",)),
        ],
    )

    add(
        "perk_06",
        "flat-armor-class-attack-bonus-with-base-damage-penalty",
        "owned-unit-indexing-plus-positive-attack-damage-event",
        {
            "base_damage_delta_on_non_structure_index": -15,
            "base_damage_floor": 0,
            "base_damage_weapon_indices": [0, 1],
            "flat_triggering_damage_bonus": 45,
            "bonus_target_defense_types": ["none", "hero", "divine"],
            "damage_source_requires_non_structure": True,
            "damage_source_requires_perk_owner": True,
            "damage_target_requires_enemy": True,
            "damage_event_type": 0,
            "damage_listener_requires_positive_damage": True,
            "modifies_current_damage_instance_additively": True,
        },
        [
            ("PerkIndexHandler_perkOnIndex_PerkTranscendentBlades_call_perkOnIndex_PerkTranscendentBlades", (
                b"not unit_isType(Rhn,UNIT_TYPE_STRUCTURE)", b"applyTranscendentBladesTo(Rhn)",
            )),
            ("applyTranscendentBladesTo", (
                b"unit_getBaseDamage", b"Qdr+M2", b"Rdr+M2", b"BlzSetUnitBaseDamage",
            )),
            ("isTranscendentBladesArmor", (
                b"DEFENSE_TYPE_NONE", b"DEFENSE_TYPE_HERO", b"DEFENSE_TYPE_DIVINE",
            )),
            ("DamageListener_perkListenDamage_PerkTranscendentBlades_onEvent_perkListenDamage_PerkTranscendentBlades", (
                b"unit_getOwner(Uhn)==Thn.plr", b"unit_isEnemyOf(Vhn,Thn.plr)", b"not unit_isType(Uhn,UNIT_TYPE_STRUCTURE)",
                b"DamageEvent_getType()==0", b"DamageEvent_getAmount()>0.", b"DamageEvent_getAmount()+L2",
                b"DamageInstance_DamageInstance_setAmount",
            )),
        ],
    )

    add(
        "perk_09",
        "toggleable-health-band-attack-damage-stance",
        "positive-attack-damage-event-plus-builder-toggle-spell",
        {
            "initial_mode": "execute",
            "modes": {
                "execute": {
                    "target_hp_ratio_below_0_35_factor": 1.25,
                    "target_hp_ratio_above_0_70_factor": 0.85,
                    "middle_band_factor": 1.0,
                },
                "open-fire": {
                    "target_hp_ratio_below_0_35_factor": 0.85,
                    "target_hp_ratio_above_0_70_factor": 1.18,
                    "middle_band_factor": 1.0,
                },
            },
            "low_hp_threshold_exclusive": 0.35,
            "high_hp_threshold_exclusive": 0.70,
            "attack_damage_event_type": 0,
            "source_requires_perk_owner": True,
            "target_requires_enemy": True,
            "toggle_cooldown_seconds": 10.0,
            "execute_mode_ability_id": 1095577654,
            "open_fire_mode_ability_id": 1095577655,
            "round_start_resets_to_execute": True,
            "cleanup_removes_both_toggle_abilities": True,
            "modifies_current_damage_instance": True,
        },
        [
            ("registerExecutionOrderPlayer", (b"O4[L8q]=true", b"P4[L8q]=0", b"ensureExecutionOrderBuilderAbility",)),
            ("Action_watch_PerkExecutionOrder_run_watch_PerkExecutionOrder", (b"P4[Tdn]=0", b"ensureExecutionOrderBuilderAbility",)),
            ("executionOrderDamageFactor", (
                b"P4[o8q])==1", b"p8q>V4", b"return S4", b"p8q<W4", b"return R4",
                b"return U4", b"return T4",
            )),
            ("toggleExecutionOrderMode", (b"P4[A8q]=1", b"P4[A8q]=0", b"Q4", b"BlzStartUnitAbilityCooldown",)),
            ("EventListener_perkListen_setCleanup_PerkExecutionOrder_onEvent_perkListen_setCleanup_PerkExecutionOrder", (
                b"GetSpellAbilityId()", b"fen==e5", b"fen==Z4", b"toggleExecutionOrderMode",
            )),
            ("DamageListener_perkListenDamage_setCleanup_PerkExecutionOrder_onEvent_perkListenDamage_setCleanup_PerkExecutionOrder", (
                b"DamageEvent_getType()==0", b"DamageEvent_getAmount()>0.", b"BlzGetUnitMaxHP",
                b"widget_getLife(jen)/ken", b"executionOrderDamageFactor", b"DamageInstance_DamageInstance_setAmount",
            )),
            ("PerkCleanupFunc_setCleanup_PerkExecutionOrder_call_setCleanup_PerkExecutionOrder", (
                b"O4[ren]=false", b"P4[ren]=0", b"unit_removeAbility(sen,e5)", b"unit_removeAbility(sen,Z4)",
            )),
        ],
        related_rawcode_ids=(1095577654, 1095577655),
    )

    add(
        "perk_11",
        "bulwark-hp-for-mobility-and-attack-speed-tradeoff",
        "perk-activation-current-units-plus-owned-unit-indexing",
        {
            "unit_scope": "combat sappers",
            "max_hp_factor": 1.12,
            "max_hp_rounding": "real_toInt(old-max-hp*1.12+0.5)",
            "current_life_factor": 1.12,
            "move_speed_factor": 0.93,
            "attack_speed_factor": 0.93,
            "attack_cooldown_formula": "old-cooldown/0.93",
            "affected_weapon_indices": [0, 1],
            "cleanup_reversal_present": False,
        },
        [
            ("applyBulwarkMarchToCurrentUnits", (
                b"GroupEnumUnitsOfPlayer", b"isCombatSapper(F7q)", b"applyBulwarkMarchTo(F7q)",
            )),
            ("PerkIndexHandler_perkOnIndex_setCleanup_PerkBulwarkMarch_call_perkOnIndex_setCleanup_PerkBulwarkMarch", (
                b"isCombatSapper(fdn)", b"applyBulwarkMarchTo(fdn)",
            )),
            ("applyBulwarkMarchTo", (
                b"BlzGetUnitMaxHP", b"x7q*B5", b"widget_getLife(w7q)*B5", b"GetUnitMoveSpeed(w7q)*A5",
                b"BlzGetUnitAttackCooldown(w7q,0)", b"z7q/z5", b"BlzGetUnitAttackCooldown(w7q,1)", b"A7q/z5",
            )),
        ],
    )

    add(
        "perk_12",
        "castle-boundary-armor-stance",
        "perk-activation-unit-indexing-and-castle-enter-leave-events",
        {
            "unit_scope": "combat sappers",
            "outside_own_castle_armor_bonus": 3,
            "inside_own_castle_armor_bonus": -2,
            "outside_armor_ability_id": 1095577656,
            "inside_armor_ability_id": 1095577657,
            "inside_test": "isInsideOwnCastleRect(unit-owner,unit)",
            "castle_event_hooks": "enter and leave for both castle rectangles",
            "mutually_exclusive_armor_abilities": True,
            "cleanup_removes_both_armor_abilities_and_tracking": True,
        },
        [
            ("registerCurrentLastWallUnits", (
                b"GroupEnumUnitsOfPlayer", b"isLastWallEligibleUnit(jbr)", b"trackLastWallUnit(gbr,jbr)",
            )),
            ("PerkIndexHandler_perkOnIndex_setCleanup_PerkLastWallProtocol_call_perkOnIndex_setCleanup_PerkLastWallProtocol", (
                b"isLastWallEligibleUnit(Qfn)", b"trackLastWallUnit(Pfn.plr,Qfn)",
            )),
            ("refreshLastWallArmor", (
                b"isInsideOwnCastleRect(Par,Oar)", b"unit_getAbilityLevel(Oar,J3)", b"unit_getAbilityLevel(Oar,I3)",
                b"unit_removeAbility(Oar,J3)", b"addProtectedAbility(Var,War)", b"unit_removeAbility(Oar,I3)", b"addProtectedAbility(Xar,Yar)",
            )),
            ("installLastWallCastleHooksOnce", (
                b"TriggerRegisterEnterRectSimple(rbr,NFb)", b"TriggerRegisterLeaveRectSimple(sbr,NFb)",
                b"TriggerRegisterEnterRectSimple(tbr,MFb)", b"TriggerRegisterLeaveRectSimple(ubr,MFb)",
            )),
            ("EventListener_perkListen_setCleanup_PerkLastWallProtocol_onEvent_perkListen_setCleanup_PerkLastWallProtocol", (
                b"GetTriggerUnit()", b"unit_getOwner(Tfn)==Sfn.plr", b"untrackLastWallUnit(Tfn)",
            )),
            ("untrackLastWallUnit", (
                b"group_remove", b"removeLastWallArmorAbilities(dbr)", b"E3[ebr]=false", b"D3[ebr]=(-1)",
            )),
            ("PerkCleanupFunc_setCleanup_PerkLastWallProtocol_call_setCleanup_PerkLastWallProtocol", (
                b"removeLastWallArmorAbilities(Zfn)", b"E3[agn]=false", b"D3[agn]=(-1)", b"GroupClear(cgn)",
            )),
        ],
        related_rawcode_ids=(1095577656, 1095577657),
    )

    add(
        "perk_13",
        "ranged-ground-range-and-attack-speed-tradeoff",
        "perk-activation-current-units-plus-owned-unit-indexing",
        {
            "unit_scope": "combat sapper and ground and ranged attacker",
            "advertised_attack_range_delta": 90,
            "script_range_reference_weapon_index": 0,
            "script_range_write_weapon_index": 1,
            "script_range_write_formula": "weapon1 + 90",
            "primary_weapon_range_is_not_directly_written": True,
            "acquisition_range_update_condition": "current-acquisition < weapon0 + 90",
            "acquisition_range_on_update": "weapon0 + 140",
            "acquisition_range_otherwise": "unchanged",
            "attack_speed_factor": 0.88,
            "attack_cooldown_formula": "old-cooldown/0.88",
            "attack_cooldown_weapon_indices": [0, 1],
            "script_range_quirk_preserved": True,
            "cleanup_reversal_present": False,
        },
        [
            ("isRangedGroundCombatUnit", (
                b"isCombatSapper(Kbr)", b"UNIT_TYPE_GROUND", b"UNIT_TYPE_RANGED_ATTACKER",
            )),
            ("setUnitAttackRangeFixed", (
                b"UNIT_WEAPON_RF_ATTACK_RANGE,0", b"UNIT_WEAPON_RF_ATTACK_RANGE,1", b"((Mbr-Nbr)+Obr)",
            )),
            ("applyLonglineFormationTo", (
                b"Qbr+B3", b"setUnitAttackRangeFixed(Pbr,Rbr)", b"Rbr+50.",
                b"BlzGetUnitAttackCooldown(Pbr,0)", b"Sbr/A3", b"BlzGetUnitAttackCooldown(Pbr,1)", b"Tbr/A3",
            )),
            ("applyLonglineFormationToCurrentUnits", (
                b"GroupEnumUnitsOfPlayer", b"isRangedGroundCombatUnit(Ybr)", b"applyLonglineFormationTo(Ybr)",
            )),
            ("PerkIndexHandler_perkOnIndex_setCleanup_PerkLonglineFormation_call_perkOnIndex_setCleanup_PerkLonglineFormation", (
                b"isRangedGroundCombatUnit(ogn)", b"applyLonglineFormationTo(ogn)",
            )),
        ],
    )

    add(
        "perk_14",
        "production-building-enchantment-with-global-trained-unit-penalty",
        "perk-activation-unit-indexing-builder-target-spell-and-target-death",
        {
            "builder_target_ability_id": 1095577697,
            "selected_building_marker_ability_id": 1095577698,
            "enchanted_spawn_marker_ability_id": 1095577699,
            "target_requirement": "allied production building",
            "all_owned_trained_combat_sapper_max_hp_factor": 0.95,
            "all_owned_trained_combat_sapper_base_damage_factor": 0.95,
            "selected_building_spawn_max_hp_factor": 1.15,
            "selected_building_spawn_base_damage_factor": 1.15,
            "selected_building_spawn_model_scale_factor": 1.10,
            "selected_building_spawn_nominal_net_hp_factor_after_global_penalty": 1.0925,
            "selected_building_spawn_nominal_net_base_damage_factor_after_global_penalty": 1.0925,
            "selected_building_spawn_scaling_order": ["global-0.95", "selected-1.15"],
            "selected_bonus_deferred_via_nested_zero_timers": 2,
            "each_scaling_step_rounds_independently": True,
            "max_hp_current_life_when_increasing": "old-life + (new-max-old-max)",
            "max_hp_current_life_when_decreasing": "min(old-life,new-max)",
            "base_damage_weapon_indices": [0, 1],
            "scale_rounding": "real_toInt(value*factor+0.5)",
            "selected_bonus_scans_all_active_players_and_stops_at_first_matching_target": True,
            "selected_bonus_stacks_multiple_active_targets": False,
            "round_start_clears_selected_target_but_keeps_perk_active": True,
            "cleanup_disables_perk_removes_builder_ability_and_spawn_markers": True,
        },
        [
            ("scaleUnitBaseDamage", (
                b"unit_getBaseDamage(z5q,0)", b"B5q*A5q", b"unit_getBaseDamage(z5q,1)", b"C5q*A5q",
                b"BlzSetUnitBaseDamage",
            )),
            ("scaleUnitMaxHp", (
                b"BlzGetUnitMaxHP(H5q)", b"int_toReal(J5q)*I5q", b"if(K5q>J5q)then",
                b"widget_getLife(H5q)+int_toReal((K5q-J5q))", b"min1(widget_getLife(H5q),int_toReal(K5q))",
            )),
            ("scaleUnitModelSize", (
                b"UNIT_RF_SCALING_VALUE", b"R5q*Q5q", b"SetUnitScale",
            )),
            ("applyProductionSpawnModifiers", (
                b"isCombatSapper(V5q)", b"getSpawnBuilding(V5q)", b"unit_getOwner(W5q)==U5q",
                b"scaleUnitMaxHp(X5q,R5)", b"scaleUnitBaseDamage(X5q,S5)",
            )),
            ("applyProductionEnchantBonusFromAnyActiveTarget", (
                b"isCombatSapper(Y5q)", b"getSpawnBuilding(Y5q)", b"ProductionEnchantmentState_active",
                b"Z5q==b6q.ProductionEnchantmentState_target", b"scaleUnitMaxHp(c6q,U5)", b"scaleUnitBaseDamage(c6q,V5)",
                b"scaleUnitModelSize(c6q,T5)", b"addAbilityIfMissing(c6q,e6)", b"return",
            )),
            ("setProductionEnchantedBuilding", (
                b"unit_isAllyOf(g6q,f6q)", b"isProductionBuilding(g6q)", b"addAbilityIfMissing(g6q,k6)",
            )),
            ("registerProductionEnchantmentPlayer", (
                b"ProductionEnchantmentState_active=true", b"ensureProductionBuilderAbility(Z6q)", b"code__onUnitIndex_PerkBuildingEnchantment",
            )),
            ("code__onUnitIndex_PerkBuildingEnchantment", (
                b"getIndexingUnit()", b"Pr:create715()", b"f7q.indexed=e7q", b"nullTimer(f7q)",
            )),
            ("CallbackSingle_nullTimer_onUnitIndex_PerkBuildingEnchantment_call_nullTimer_onUnitIndex_PerkBuildingEnchantment", (
                b"Rr:create716()", b"zbn.indexed=ybn.indexed", b"nullTimer(zbn)",
            )),
            ("CallbackSingle_nullTimer_nullTimer_onUnitIndex_PerkBuildingEnchantment_call_nullTimer_nullTimer_onUnitIndex_PerkBuildingEnchantment", (
                b"applyProductionEnchantBonusFromAnyActiveTarget(Bbn.indexed)",
            )),
            ("PerkIndexHandler_perkOnIndex_setCleanup_PerkBuildingEnchantment_call_perkOnIndex_setCleanup_PerkBuildingEnchantment", (
                b"applyProductionSpawnModifiers(Ubn.plr,Vbn)",
            )),
            ("EventListener_perkListen_setCleanup_PerkBuildingEnchantment_onEvent_perkListen_setCleanup_PerkBuildingEnchantment", (
                b"GetSpellAbilityId()==m6", b"setProductionEnchantedBuilding(Xbn.plr,Zbn)",
            )),
            ("EventListener_perkListen_setCleanup_PerkBuildingEnchantment_onEvent_perkListen_setCleanup_PerkBuildingEnchantment1", (
                b"GetTriggerUnit()", b"ProductionEnchantmentState_target", b"clearProductionEnchantedBuildingFor(bcn.plr)",
            )),
            ("clearProductionEnchantedBuildingFor", (
                b"removeAbilityIfPresent(e6q.ProductionEnchantmentState_target,k6)", b"ProductionEnchantmentState_target=nil",
                b"destroyEnchantmentEffect", b"ProductionEnchantmentState_fx=nil",
            )),
            ("Action_watch_PerkBuildingEnchantment_run_watch_PerkBuildingEnchantment", (
                b"ensureProductionBuilderAbility(V1[wbn])", b"clearProductionEnchantedBuildingFor(V1[wbn])",
            )),
            ("PerkCleanupFunc_setCleanup_PerkBuildingEnchantment_call_setCleanup_PerkBuildingEnchantment", (
                b"ProductionEnchantmentState_active=false", b"removeAbilityIfPresent(hcn,k6)", b"removeAbilityIfPresent(hcn,e6)",
                b"unit_removeAbility(icn,m6)",
            )),
        ],
        related_rawcode_ids=(1095577697, 1095577698, 1095577699),
    )

    add(
        "perk_15",
        "flat-hp-regeneration-with-base-damage-penalty",
        "perk-activation-current-units-plus-owned-unit-indexing",
        {
            "unit_scope": "combat sappers",
            "hp_regeneration_delta_per_second": 7.0,
            "base_damage_factor": 0.90,
            "base_damage_weapon_indices": [0, 1],
            "base_damage_rounding": "real_toInt(old-base-damage*0.90+0.5)",
            "cleanup_reversal_present": False,
        },
        [
            ("applyRampantGrowthTo", (
                b"UNIT_RF_HIT_POINTS_REGENERATION_RATE", b"+Z2", b"unit_getBaseDamage(Ccr,0)", b"int_toReal(Dcr)*Y2",
                b"unit_getBaseDamage(Ccr,1)", b"int_toReal(Ecr)*Y2", b"BlzSetUnitBaseDamage",
            )),
            ("applyRampantGrowthToCurrentUnits", (
                b"GroupEnumUnitsOfPlayer", b"isCombatSapper", b"applyRampantGrowthTo",
            )),
            ("PerkIndexHandler_perkOnIndex_setCleanup_PerkRampantGrowth_call_perkOnIndex_setCleanup_PerkRampantGrowth", (
                b"isCombatSapper(Ogn)", b"applyRampantGrowthTo(Ogn)",
            )),
        ],
    )

    add(
        "perk_17",
        "spell-building-mana-regeneration-enchantment",
        "perk-activation-construction-finish-builder-target-spell-and-target-death",
        {
            "builder_target_ability_id": 1095577700,
            "selected_building_marker_ability_id": 1095577701,
            "target_requirement": "allied spell building",
            "selected_spell_building_mana_regen_delta": 0.20,
            "other_owned_spell_building_mana_regen_delta": -0.05,
            "selected_allied_building_may_be_owned_by_another_player": True,
            "mana_regen_delta_tracking_is_idempotent": True,
            "construction_finish_refreshes_owned_spell_buildings": True,
            "target_death_clears_selection": True,
            "unit_deindex_clears_stored_delta_for_reused_index": True,
            "round_start_clears_selected_target_but_keeps_perk_active": True,
            "cleanup_resets_selected_delta_and_removes_builder_ability": True,
        },
        [
            ("desiredSpellBuildingManaDelta", (
                b"isSpellBuilding(B6q)", b"SpellEnchantmentState_active", b"B6q==C6q.SpellEnchantmentState_target",
                b"return K5", b"return J5",
            )),
            ("setSpellBuildingManaDelta", (
                b"UNIT_RF_MANA_REGENERATION", b"+x6q)-z6q", b"G5[y6q]=x6q",
            )),
            ("refreshSpellBuildingEnchantments", (
                b"GroupEnumUnitsOfPlayer", b"desiredSpellBuildingManaDelta(H6q,K6q)", b"setSpellBuildingManaDelta(K6q,L6q)",
                b"K6q==I6q.SpellEnchantmentState_target", b"addAbilityIfMissing(K6q,P5)",
            )),
            ("setSpellEnchantedBuilding", (
                b"unit_isAllyOf(T6q,S6q)", b"isSpellBuilding(T6q)", b"SpellEnchantmentState_target=T6q",
                b"refreshSpellBuildingEnchantments(S6q)",
            )),
            ("registerSpellEnchantmentPlayer", (
                b"SpellEnchantmentState_active=true", b"ensureSpellBuilderAbility(g7q)", b"refreshSpellBuildingEnchantments(g7q)",
                b"code__onUnitDeindex_PerkBuildingEnchantment",
            )),
            ("code__onUnitDeindex_PerkBuildingEnchantment", (
                b"unit_getIndex(getIndexingUnit())", b"G5[l7q]=0.",
            )),
            ("EventListener_perkListen_setCleanup_PerkBuildingEnchantment_onEvent_perkListen_setCleanup_PerkBuildingEnchantment2", (
                b"GetConstructedStructure()", b"unit_getOwner(xcn)==wcn.plr", b"isSpellBuilding(xcn)", b"refreshSpellBuildingEnchantments(wcn.plr)",
            )),
            ("EventListener_perkListen_setCleanup_PerkBuildingEnchantment_onEvent_perkListen_setCleanup_PerkBuildingEnchantment3", (
                b"GetSpellAbilityId()==Q5", b"setSpellEnchantedBuilding(zcn.plr,Bcn)",
            )),
            ("EventListener_perkListen_setCleanup_PerkBuildingEnchantment_onEvent_perkListen_setCleanup_PerkBuildingEnchantment4", (
                b"GetTriggerUnit()", b"SpellEnchantmentState_target", b"clearSpellEnchantedBuildingFor(Dcn.plr)",
            )),
            ("clearSpellEnchantedBuildingFor", (
                b"removeAbilityIfPresent(R6q.SpellEnchantmentState_target,P5)", b"setSpellBuildingManaDelta(R6q.SpellEnchantmentState_target,0.)",
                b"SpellEnchantmentState_target=nil", b"refreshSpellBuildingEnchantments(Q6q)",
            )),
            ("Action_watch_PerkBuildingEnchantment_run_watch_PerkBuildingEnchantment1", (
                b"ensureSpellBuilderAbility(V1[Fbn])", b"clearSpellEnchantedBuildingFor(V1[Fbn])",
            )),
            ("PerkCleanupFunc_setCleanup_PerkBuildingEnchantment_call_setCleanup_PerkBuildingEnchantment1", (
                b"SpellEnchantmentState_active=false", b"clearSpellEnchantedBuildingFor(Hcn)", b"unit_removeAbility(Jcn,Q5)",
            )),
        ],
        related_rawcode_ids=(1095577700, 1095577701),
    )

    rows.sort(key=lambda row: int(row["protected_registry_slot"]))
    if len(rows) != 19:
        raise ValueError(f"proven-live perk semantic row count changed: {len(rows)}")
    if {str(row["perk_id"]) for row in rows} != set(audit_by_id):
        raise ValueError("not every proven protected perk registration has normalized semantics")
    return rows


def _extract_runtime_ai_mechanics(
    data: bytes,
    functions: list[dict[str, object]],
    protected_filter_bindings: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Normalize live AI-only damage observers/controllers without treating them as combat rewrites."""
    functions_by_name = {str(row["name"]): row for row in functions}
    engagement_listener = "DamageListener_addListener_AiEngagement_onEvent_addListener_AiEngagement"
    custom_ai_listener = "DamageListener_addListener_CustomAI_onEvent_addListener_CustomAI"
    if engagement_listener not in functions_by_name or custom_ai_listener not in functions_by_name:
        return []

    def source(name: str, fragments: Iterable[bytes] = ()) -> tuple[int, bytes]:
        row = functions_by_name.get(name)
        if row is None:
            raise ValueError(f"runtime AI mechanic source function missing: {name}")
        start = int(row["start"])
        body = data[start:int(row["end"])]
        for fragment in fragments:
            if fragment not in body:
                raise ValueError(f"runtime AI mechanic source changed: {name}: missing {fragment!r}")
        return start, body

    def require_global_fragment(fragment: bytes) -> None:
        if data.count(fragment) != 1:
            raise ValueError(f"runtime AI global constant changed or became ambiguous: {fragment!r}")

    for fragment in (
        b"oDb=2.0", b"nDb=0.8",
        b"Ghb=28.0", b"Fhb=75.0", b"Ehb=810000.0", b"Dhb=900.0", b"Chb=700.0",
        b"zhb=16", b"yhb=0.25",
    ):
        require_global_fragment(fragment)

    sx_rows = [row for row in protected_filter_bindings if str(row["symbol"]) == "SX"]
    if len(sx_rows) != 1:
        raise ValueError(f"runtime AI Rescue Strike filter SX resolution changed: {len(sx_rows)} rows")
    sx = sx_rows[0]
    if str(sx["resolution_status"]) != "resolved" or str(sx["predicate"]) != "alive-combat-sapper;enemy-of-mIb":
        raise ValueError(f"runtime AI Rescue Strike filter SX changed: {sx}")

    rows: list[dict[str, object]] = []

    def add(
        system_id: str,
        mechanic_kind: str,
        trigger: str,
        parameters: dict[str, object],
        source_functions: list[tuple[str, tuple[bytes, ...]]],
        *,
        related_rawcode_ids: Iterable[int] = (),
    ) -> None:
        offsets: list[int] = []
        names: list[str] = []
        for name, fragments in source_functions:
            start, _body = source(name, fragments)
            offsets.append(start)
            names.append(name)
        rows.append({
            "system_id": system_id,
            "mechanic_kind": mechanic_kind,
            "trigger": trigger,
            "parameters": parameters,
            "related_rawcode_ids": list(related_rawcode_ids),
            "source_functions": list(dict.fromkeys(names)),
            "evidence_kind": "exact-readable-ai-runtime-control-flow",
            "byte_offset": min(offsets),
        })

    add(
        "ai-engagement-damage-signals",
        "decayed-damage-weighted-engagement-and-structure-pressure-signals",
        "damage-event-plus-periodic-decay-and-round-reset",
        {
            "rewrites_damage": False,
            "requires_positive_damage": True,
            "source_structure_ignored": True,
            "source_currently_caged_ignored": True,
            "structure_target_behavior": "accumulate damage into source-team structure-pressure bucket and stop",
            "structure_target_contributes_to_engagement_centroid": False,
            "non_structure_caged_target_ignored": True,
            "engagement_sample_position": "midpoint(source-position,target-position)",
            "engagement_sample_weight": "damage-amount",
            "centroid_accumulators": "sum(midpoint.x*damage), sum(midpoint.y*damage), sum(damage)",
            "decay_period_seconds": 2.0,
            "decay_factor_per_period": 0.8,
            "decayed_values": ["engagement-x-weighted-sum", "engagement-y-weighted-sum", "engagement-damage-weight", "team-0-structure-damage", "team-1-structure-damage"],
            "round_start_resets_all_accumulators": True,
            "engagement_axis_formula": "clamp(dot(weighted-centroid-castle0,castle1-castle0)/length_sq(castle1-castle0),0,1)",
            "engagement_axis_invalid_value": -1.0,
            "engagement_dominance_formula": "team0=(axis-0.5)*2; other-team=negative(team0)",
            "structure_push_dominance_formula": "clamp((team-damage-other-team-damage)/(team-damage+other-team-damage),-1,1)",
        },
        [
            (engagement_listener, (
                b"DamageEvent_getSource()", b"DamageEvent_getTarget()", b"DamageEvent_getAmount()", b"jyk<=0.0",
                b"UNIT_TYPE_STRUCTURE", b"vec2_isCagedAt(unit_getPos(hyk),unit_getOwner(hyk))",
                b"jDb[kyk]=(__wurst_ensureReal(jDb[kyk])+jyk)", b"vec2_isCagedAt(unit_getPos(iyk),unit_getOwner(iyk))",
                b"vec2_op_mult(vec2_op_plus(unit_getPos(hyk),unit_getPos(iyk)),0.5)",
                b"mDb=(mDb+(lyk[1]*jyk))", b"lDb=(lDb+(lyk[2]*jyk))", b"kDb=(kDb+jyk)",
            )),
            ("RD", (b"oDb=2.0", b"nDb=0.8", b"DamageEvent_addListener", b"doPeriodically(ZSo,YSo)")),
            ("CallbackPeriodic_doPeriodically_AiEngagement_call_doPeriodically_AiEngagement", (
                b"mDb=(mDb*nDb)", b"lDb=(lDb*nDb)", b"kDb=(kDb*nDb)",
                b"jDb[0]=(__wurst_ensureReal(jDb[0])*nDb)", b"jDb[1]=(__wurst_ensureReal(jDb[1])*nDb)",
            )),
            ("Action_batch_AiRoundState_run_batch_AiRoundState", (
                b"mDb=0.0", b"lDb=0.0", b"kDb=0.0", b"jDb[0]=0.0", b"jDb[1]=0.0",
            )),
            ("getEngagementAxisPos", (
                b"mDb/kDb", b"lDb/kDb", b"unit_getPos(cX[0])", b"unit_getPos(cX[1])", b"real_clamp", b"return(-1.0)",
            )),
            ("getEngagementDominance", (b"getEngagementAxisPos()", b"(iTo-0.5)*2.0", b"kTo=(-jTo)")),
            ("getStructurePushDominance", (b"jDb[lTo]", b"jDb[oTo]", b"(mTo-nTo)/pTo", b"real_clamp")),
        ],
    )

    add(
        "ai-strategic-aura-purchase-observer",
        "team-strategic-aura-purchase-state-and-buyer-coordination",
        "player-unit-sell-item-event",
        {
            "rewrites_gameplay_event": False,
            "observed_event": "EVENT_PLAYER_UNIT_SELL_ITEM",
            "buyer_source": "owner of GetBuyingUnit()",
            "item_source": "GetSoldItem() type id",
            "tracked_items": [
                {"item_id": 1227894849, "rawcode": "I00A", "strategic_index": 0},
                {"item_id": 1227894835, "rawcode": "I003", "strategic_index": 1},
            ],
            "team_index_range": [0, 1],
            "state_effect": "mark strategic aura bought for buyer team, clear that team's pending/reserved strategic aura slots, and record buyer player id",
            "human_and_ai_purchases_are_observed": True,
            "already_bought_item_is_removed_from_future_strategic_candidates": True,
            "state_resets_with_ai_round_state": True,
        },
        [
            ("UD", (b"EVENT_PLAYER_UNIT_SELL_ITEM", b"EventListener_add(aWo,ZVo)")),
            ("EventListener_add_AiItemBuying_onEvent_add_AiItemBuying", (
                b"GetBuyingUnit()", b"GetSoldItem()", b"noteStrategicItemBoughtByPlayer(unit_getOwner(Nyk),item_getTypeId(Oyk))",
            )),
            ("getStrategicAuraItemIndex", (b"rWo==1227894849", b"return 0", b"rWo==1227894835", b"return 1")),
            ("markStrategicItemBoughtForTeam", (
                b"getStrategicAuraItemIndex(mXo)", b"uCb[((lXo*16)+nXo)]=true", b"pCb[lXo]=(-1)", b"oCb[lXo]=(-1)",
            )),
            ("noteStrategicItemBoughtByPlayer", (
                b"markStrategicItemBoughtForTeam(__wurst_ensureInt(lGb[qXo]),pXo)",
                b"qCb[__wurst_ensureInt(lGb[qXo])]=qXo",
            )),
            ("hasTeamBoughtStrategicItem", (b"getStrategicAuraItemIndex(eXo.CFItem_itemId)", b"uCb[((dXo*16)+fXo)]")),
            ("isStrategicAuraCandidateForTeam", (
                b"rXo.CFItem_isStrategicAura", b"rXo.CFItem_requiresTarget", b"not hasTeamBoughtStrategicItem(sXo,rXo)",
            )),
            ("Action_batch_AiRoundState_run_batch_AiRoundState", (b"resetStrategicItemBuyingState()",)),
            ("resetStrategicItemBuyingState", (b"uCb[((eWo*16)+fWo)]=false", b"qCb[eWo]=(-1)", b"pCb[eWo]=(-1)", b"oCb[eWo]=(-1)")),
        ],
        related_rawcode_ids=(1227894849, 1227894835),
    )

    add(
        "ai-executor-periodic-fsm",
        "staggered-one-second-ai-fsm-controller-tick",
        "ai-executor-construction-delayed-periodic-update",
        {
            "fsm_update_period_seconds": 1.0,
            "startup_delay_formula_seconds": "0.02 + ((player_id*7) mod 12)*0.075",
            "startup_delay_min_seconds": 0.02,
            "startup_delay_max_seconds": 0.845,
            "startup_generation_must_still_match": True,
            "executor_must_still_be_registered_for_player": True,
            "does_not_start_second_periodic_callback_if_one_exists": True,
            "periodic_action": "FSM_FSM_update(executor.fsm,1.0)",
        },
        [
            ("AiExecutor_construct_AiExecutor", (
                b"b8l=(0.02+(int_toReal(__wurst_modInt((player_getId(Z7l)*7),12))*0.075))",
                b"d8l.tick=1.0", b"doAfter(b8l,d8l)",
            )),
            ("CallbackSingle_doAfter_AiExecutor_CustomAI_call_doAfter_AiExecutor_CustomAI1", (
                b"AiExecutor_executorGeneration==b9l.startupGeneration", b"Nhb[player_getId(b9l.plr)]==b9l.this",
                b"b9l.this.AiExecutor_update==nil", b"b9l.this.AiExecutor_update=doPeriodically(d9l,c9l)",
            )),
            ("CallbackPeriodic_doPeriodically_doAfter_AiExecutor_CustomAI_call_doPeriodically_doAfter_AiExecutor_CustomAI", (
                b"FSM_FSM_update(f9l.this.AiExecutor_fsm,f9l.tick)",
            )),
        ],
    )

    add(
        "ai-rescue-strike-controller",
        "damage-triggered-ai-rescue-strike-targeting-and-throttling",
        "damage-event-on-low-hp-structure",
        {
            "rewrites_damage": False,
            "rescue_strike_ability_id": 1093677109,
            "damaged_unit_source": "EventData trigger unit",
            "attacker_source": "DamageEvent source",
            "target_requires_structure": True,
            "target_hp_ratio_below_exclusive": 0.65,
            "target_must_not_be_in_construction": True,
            "team_index_range": [0, 1],
            "evaluation_debounce_seconds_per_team": 0.25,
            "global_commit_lock_seconds": 3.0,
            "requires_positive_team_rescue_strike_count": True,
            "non_castle_disabled_mode_restricts_target_to_team_castle": True,
            "caster_requirement": "active AI executor with builder carrying A005 and not build-locked",
            "target_filter": str(sx["predicate"]),
            "target_filter_symbol": "SX",
            "target_filter_owner_context": "mIb = damaged-structure owner",
            "effect_radius": 700.0,
            "candidate_search_radius_around_damaged_structure": 900.0,
            "maximum_candidate_units_scored": 16,
            "initial_candidate_point": "attacker position",
            "candidate_score": "number of filtered units within 700 radius",
            "best_candidate_tie_behavior": "keep earlier candidate; replace only on strictly greater score",
            "tower_target_vs_siege_attacker_score_delta": -4,
            "required_score_hp_ratio_clamp": [0.20, 0.65],
            "required_score_formula": "2 + 14*((clamp(hp_ratio,0.20,0.65)-0.20)/0.45)",
            "commit_condition": "adjusted-score > 16 OR adjusted-score > required-score",
            "comparison_is_strict": True,
            "non_castle_recent_commit_throttle_seconds": 28.0,
            "non_castle_recent_commit_throttle_applies_when_score_lte": 16,
            "same_area_distance_squared_threshold": 810000.0,
            "same_area_distance_threshold": 900.0,
            "same_area_repeat_throttle_seconds": 75.0,
            "same_area_repeat_score_limit_formula": "real_toInt(required-score+5)",
            "castle_targets_bypass_repeat_throttles": True,
            "commit_selects_first_eligible_ai_player_in_team_force": True,
            "commit_updates_last_time_and_target_position": True,
        },
        [
            (custom_ai_listener, (b"onAttackStrikeCheck()",)),
            ("resetAiRescueStrikeCoordination", (b"Lhb=false", b"Khb[amq]=0.", b"Jhb[amq]=0.", b"Ihb[amq]=0.", b"Hhb[amq]=0.")),
            ("requiredCount", (b"clamp(emq,0.20,0.65)", b"2.+(14*((fmq-0.20)/0.45))")),
            ("shouldRescueStrike", (b"hmq>16", b"hmq>requiredCount(gmq)")),
            ("countRescueStrikeHitsAt", (b"mmq=Chb", b"nmq=SX", b"GroupEnumUnitsInRange", b"group_size(imq)")),
            ("findBestRescueStrikeTarget", (
                b"qmq=tupleCopy1(unit_getPos(pmq))", b"countRescueStrikeHitsAt(Ahb,qmq)", b"Dhb", b"Cmq=SX",
                b"if(smq<zhb)then", b"countRescueStrikeHitsAt(Ahb,vmq)", b"if(wmq>rmq)then",
            )),
            ("hasAvailableAiRescueStrikeCaster", (
                b"aIb[player_getId(Imq)]", b"AiExecutor_builder", b"unit_hasAbility(Jmq.AiExecutor_builder,1093677109)",
                b"not AiExecutor_AiExecutor_isBuildLocked(Jmq)",
            )),
            ("shouldThrottleAiRescueStrike", (
                b"if(Qmq or(Smq<=0.0))then return false", b"Zmq<Ghb", b"Ymq<=16", b"<=Ehb", b"Zmq<Fhb",
                b"Ymq<=real_toInt((requiredCount(Xmq)+5.0))",
            )),
            ("onAttackStrikeCheck", (
                b"DamageEvent_getSource()", b"EventData_getTriggerUnit()", b"getElapsedGameTime()-__wurst_ensureReal(Khb[gnq]))>=yhb",
                b"UNIT_TYPE_STRUCTURE", b"unit_getHPRatio(enq)<0.65", b"not unit_isInConstruction(enq)",
                b"isRsNonCastleDisabled(gnq)", b"getRescueStrikesForTeam(gnq)", b"hasAvailableAiRescueStrikeCaster(inq)",
                b"mIb=unit_getOwner(enq)", b"findBestRescueStrikeTarget(enq,dnq)", b"Chb", b"SX", b"group_size(nIb)",
                b"CFBuilding_isTower", b"CFBuilding_isSiege_field", b"qnq=(qnq-4)", b"shouldRescueStrike(mnq,qnq)",
                b"shouldThrottleAiRescueStrike", b"unit_hasAbility(tnq.AiExecutor_builder,1093677109)", b"FSM_FSM_changeState",
                b"Jhb[gnq]=getElapsedGameTime()", b"Ihb[gnq]=knq[1]", b"Hhb[gnq]=knq[2]", b"Lhb=true", b"doAfter(3.,unq)",
            )),
            ("CallbackSingle_doAfter_CustomAI_call_doAfter_CustomAI1", (b"Lhb=false",)),
            ("getRescueStrikesForTeam", (b"S1q==0", b"Computed_Computed_get(T7)", b"S1q==1", b"Computed_Computed_get(S7)")),
        ],
        related_rawcode_ids=(1093677109,),
    )

    return rows


def _extract_runtime_mode_mechanics(
    data: bytes,
    functions: list[dict[str, object]],
    function_aliases: list[dict[str, object]],
    call_edges: Counter[tuple[str, str]],
) -> list[dict[str, object]]:
    """Recover the complete readable 9.27 mode registry and chat-entry controller."""
    functions_by_name = {str(row["name"]): row for row in functions}
    initializer_name = "ModeParser_initialize__w3p_vmProtect"
    listener_name = "EventListener_add_ModeParser_onEvent_add_ModeParser"
    parse_name = "ModeParser_parseModeAppend__w3p_vmProtect"
    required = {
        initializer_name, listener_name, parse_name,
        "StartResourceMode_new_StartResourceMode", "StartResourceMode_StartResourceMode_execute",
        "StartResourceMode_StartResourceMode_isValidChoice", "StartResourceMode_StartResourceMode_minForChoice",
        "StartResourceMode_StartResourceMode_applyChoice",
        "Action_watch_ModeRaceRuntime_run_watch_ModeRaceRuntime",
        "Action_watch_UltimateRoll_run_watch_UltimateRoll",
        "startNextRoundViaModeRuntime", "player_allowAllBuildings", "clearUltiTexttags",
    }
    if not required.issubset(functions_by_name):
        return []

    def body(name: str) -> tuple[int, bytes]:
        row = functions_by_name[name]
        return int(row["start"]), data[int(row["start"]):int(row["end"])]

    initializer_start, initializer = body(initializer_name)
    listener_start, listener = body(listener_name)
    parse_start, parse_source = body(parse_name)
    if b"EventData_getTriggerPlayer()==V1[0]" not in listener or b"string_startsWith(rVm,\"-\")" not in listener:
        raise ValueError("mode parser host-player/chat-prefix gate changed")
    if b"not Ocb" not in listener or b"ModeParser_parseModeAppend__w3p_vmProtect(rVm)" not in listener:
        raise ValueError("mode parser selection-active/append dispatch changed")
    if b"string_split(CUq,\"-\")" not in parse_source or b"ModeParser_parseSingleMode__w3p_vmProtect(GUq)" not in parse_source:
        raise ValueError("mode parser append splitting/dispatch changed")
    if b"ModeParser_rejectUltimateDraftConflict()" not in parse_source:
        raise ValueError("mode parser Ultimate Draft conflict gate changed")

    callback_var_to_class = {
        match.group(1).decode("ascii"): match.group(2).decode("ascii")
        for match in re.finditer(rb"([A-Za-z][A-Za-z0-9_]*)=([A-Za-z][A-Za-z0-9_]*):create\d+\(\)", initializer)
    }
    alias_targets = {str(row["alias"]): str(row["target_function"]) for row in function_aliases}
    callees_by_caller: dict[str, set[str]] = defaultdict(set)
    for (caller, callee), count in call_edges.items():
        if count > 0:
            callees_by_caller[str(caller)].add(str(callee))

    records: list[tuple[int, dict[str, object]]] = []
    patterns = [
        (
            "choice-value",
            re.compile(rb'ChoiceValueMode_new_ChoiceValueMode\("([^\"]+)",\"([^\"]+)\",\"([^\"]+)\",([A-Za-z0-9_]+),(\d+),(\d+)\)'),
        ),
        (
            "integer-value",
            re.compile(rb'ValueMode_new_ValueMode\("([^\"]+)",\"([^\"]+)\",\"([^\"]+)\",([A-Za-z0-9_]+),(\d+),(\d+)\)'),
        ),
        (
            "flag",
            re.compile(rb'FlagMode_new_FlagMode\("([^\"]+)",\"([^\"]+)\",\"([^\"]+)\",([A-Za-z0-9_]+)\)'),
        ),
    ]
    expected_counts = {"choice-value": 4, "integer-value": 15, "flag": 24}
    actual_counts: Counter[str] = Counter()
    callback_functions: list[str] = []
    for kind, pattern in patterns:
        for match in pattern.finditer(initializer):
            actual_counts[kind] += 1
            mode_id = match.group(1).decode("utf-8")
            name = match.group(2).decode("utf-8")
            description = match.group(3).decode("utf-8")
            callback_var = match.group(4).decode("ascii")
            class_symbol = callback_var_to_class.get(callback_var)
            if class_symbol is None:
                raise ValueError(f"mode callback variable has no generated closure class: {mode_id}: {callback_var}")
            callback_function = alias_targets.get(f"{class_symbol}.call")
            if callback_function is None or callback_function not in functions_by_name:
                raise ValueError(f"mode callback class has no exact .call alias: {mode_id}: {class_symbol}")
            callback_functions.append(callback_function)
            callback_start, callback_source = body(callback_function)
            simple_assignments = [
                {"symbol": symbol.decode("ascii"), "value": value.decode("utf-8")}
                for symbol, value in re.findall(
                    rb'\b([A-Za-z][A-Za-z0-9_]*)=(true|false|\(-?\d+\)|-?\d+(?:\.\d+)?|"[^"\\]*")',
                    callback_source,
                )
            ]
            row: dict[str, object] = {
                "mode_id": mode_id,
                "name": name,
                "description": description,
                "kind": kind,
                "callback_variable": callback_var,
                "callback_class_symbol": class_symbol,
                "callback_function": callback_function,
                "callback_direct_calls": sorted(callees_by_caller.get(callback_function, ())),
                "callback_simple_assignments": simple_assignments,
                "callback_byte_offset": callback_start,
            }
            if kind != "flag":
                row["minimum_value"] = int(match.group(5))
                row["maximum_value"] = int(match.group(6))
            if kind == "choice-value":
                local_tail = initializer[match.end():match.end() + 32]
                if b',"r"),"g")' not in local_tail:
                    raise ValueError(f"choice-value mode choices changed: {mode_id}")
                row["choices"] = ["r", "g"]
            records.append((match.start(), row))
    if actual_counts != Counter(expected_counts):
        raise ValueError(f"mode registry constructor counts changed: {dict(sorted(actual_counts.items()))}")

    start_resource_match = re.search(rb"([A-Za-z][A-Za-z0-9_]*)=StartResourceMode_new_StartResourceMode\(\)", initializer)
    if start_resource_match is None:
        raise ValueError("Start Resource mode registration missing")
    start_resource_start, start_resource_source = body("StartResourceMode_new_StartResourceMode")
    if b'GameMode_id="sr"' not in start_resource_source or b'GameMode_name="Set Start Resource"' not in start_resource_source:
        raise ValueError("Start Resource mode id/name changed")
    _execute_start, execute_source = body("StartResourceMode_StartResourceMode_execute")
    if b"Valid: g, l, u" not in execute_source or b"HYm>100000" not in execute_source:
        raise ValueError("Start Resource validation changed")
    _choice_start, choice_source = body("StartResourceMode_StartResourceMode_isValidChoice")
    _minimum_start, minimum_source = body("StartResourceMode_StartResourceMode_minForChoice")
    _apply_start, apply_source = body("StartResourceMode_StartResourceMode_applyChoice")
    if b'PYm=="g"' not in choice_source or b'PYm=="l"' not in choice_source or b'PYm=="u"' not in choice_source:
        raise ValueError("Start Resource choices changed")
    if b'if(RYm=="u")then return 0 end return 100' not in minimum_source:
        raise ValueError("Start Resource choice minimums changed")
    if b'if(TYm=="g")then MX=UYm' not in apply_source or b'elseif(TYm=="l")' not in apply_source or b'elseif(TYm=="u")then KX=UYm' not in apply_source:
        raise ValueError("Start Resource apply semantics changed")
    records.append((start_resource_match.start(), {
        "mode_id": "sr",
        "name": "Set Start Resource",
        "description": "Set starting gold, lumber, or unit limit",
        "kind": "resource-choice-value",
        "choices": ["g", "l", "u"],
        "choice_minimum_values": {"g": 100, "l": 100, "u": 0},
        "maximum_value": 100000,
        "callback_function": "StartResourceMode_StartResourceMode_applyChoice",
        "callback_direct_calls": sorted(callees_by_caller.get("StartResourceMode_StartResourceMode_applyChoice", ())),
        "callback_simple_assignments": [],
        "callback_byte_offset": start_resource_start,
    }))

    records.sort(key=lambda item: item[0])
    modes = [row for _offset, row in records]
    if len(modes) != 44 or len({str(row["mode_id"]) for row in modes}) != 44:
        raise ValueError(f"mode registry size/IDs changed: {len(modes)}")
    expected_ids = [
        "r", "p", "m", "d", "cr", "sr", "um", "ud", "na", "ntb", "nb", "ns", "ni", "la", "nrs",
        "ur", "norb", "desync", "du", "nch", "co", "cc", "dom", "ult", "nca", "noai", "nfow", "it", "lt",
        "glw", "gld", "mp", "emp", "ll", "ban", "rban", "bal", "fow", "fill", "nt", "ht", "mt", "skip", "w3c",
    ]
    if [str(row["mode_id"]) for row in modes] != expected_ids:
        raise ValueError("mode registry order/IDs changed")

    mode_round_watch = "Action_watch_ModeRaceRuntime_run_watch_ModeRaceRuntime"
    ultimate_round_watch = "Action_watch_UltimateRoll_run_watch_UltimateRoll"
    _mode_round_start, mode_round_source = body(mode_round_watch)
    if b"Signal_Signal_get(bX)" not in mode_round_source or b"startNextRoundViaModeRuntime()" not in mode_round_source:
        raise ValueError("mode round-end next-round watcher changed")
    _ultimate_round_start, ultimate_round_source = body(ultimate_round_watch)
    if (
        b"Signal_Signal_get(ZW)" not in ultimate_round_source
        or b"clearUltiTexttags()" not in ultimate_round_source
        or b"if(C8n>11)then break" not in ultimate_round_source
        or b"player_allowAllBuildings(V1[C8n])" not in ultimate_round_source
    ):
        raise ValueError("ultimate round-end building-availability reset changed")

    periodic_mode_sources = {
        "startLumberLimitClamp": (b"doPeriodically(0.25,IQq)",),
        "CallbackPeriodic_doPeriodically_ModeAppliers_call_doPeriodically_ModeAppliers": (
            b"if(BGb<0)then stopLumberLimitClamp()else enforceLumberLimitNow()end",
        ),
        "advanceManualRaceBan": (
            b"if __wurst_ensureBool(aIb[UVq])then WVq=1. else WVq=15. end",
            b"doPeriodically(.1,XVq)",
        ),
        "CallbackPeriodic_doPeriodically_ModeRaceRuntime_call_doPeriodically_ModeRaceRuntime": (
            b"TimerGetRemaining(L9)<=.05", b"if(AZm<=0)then AZm=MN(OW)end", b"rememberRaceBan__w3p_vmProtect(AZm)",
            b"G9=(G9-1)", b"advanceManualRaceBan()",
        ),
        "advanceDraftRaceSelection": (b"TimerStart(E9,15.", b"doPeriodically(.1,pXq)"),
        "CallbackPeriodic_doPeriodically_ModeRaceRuntime_call_doPeriodically_ModeRaceRuntime1": (
            b"TimerGetRemaining(E9)<=.05", b"if(GZm==0)then GZm=(-1)end", b"tGb[IZm]=JZm",
            b"applyRaceSelectionSideEffects", b"advanceDraftRaceSelection()",
        ),
        "runPickRaceSelection": (b"TimerStart(n9,15.", b"doPeriodically(.1,GXq)"),
        "CallbackPeriodic_doPeriodically_ModeRaceRuntime_call_doPeriodically_ModeRaceRuntime2": (
            b"if((LW<=0)or(__wurst_safe_TimerGetRemaining(n9)<=.05))then finishPickRaceSelection()end",
        ),
    }
    for periodic_name, fragments in periodic_mode_sources.items():
        _start, periodic_body = body(periodic_name)
        for fragment in fragments:
            if fragment not in periodic_body:
                raise ValueError(f"mode periodic runtime changed: {periodic_name}: missing {fragment!r}")

    return [{
        "system_id": "mode-selection-controller-and-registry",
        "mechanic_kind": "host-chat-mode-parser-with-exact-registered-mode-catalog",
        "trigger": "host-player-chat-message-during-mode-selection",
        "parameters": {
            "host_player_id": 0,
            "requires_leading_dash": True,
            "requires_selection_not_finalized": True,
            "append_parser_strips_one_leading_dash": True,
            "multiple_modes_separator": "-",
            "ultimate_draft_conflict_is_rejected_before_application": True,
            "skip_command_is_reserved_before_append_parser": True,
            "e2e_prefix_is_reserved_before_append_parser": "-e2e ",
            "registered_mode_count": 44,
            "registered_modes": modes,
            "round_end_signal_starts_next_round_via_mode_runtime": True,
            "ultimate_round_end_restores_all_building_availability_for_player_ids": [0, 11],
            "ultimate_round_end_clears_roll_texttags": True,
            "periodic_runtime": {
                "lumber_limit_enforcement_period_seconds": 0.25,
                "lumber_limit_negative_value_stops_clamp": True,
                "manual_race_ban_poll_period_seconds": 0.1,
                "manual_race_ban_timer_seconds_ai": 1,
                "manual_race_ban_timer_seconds_human": 15,
                "manual_race_ban_timer_expiry_threshold_seconds": 0.05,
                "manual_race_ban_missing_choice_uses_random_available_race": True,
                "draft_race_selection_poll_period_seconds": 0.1,
                "draft_race_selection_timer_seconds": 15,
                "draft_race_zero_choice_becomes_skip_sentinel": -1,
                "pick_race_poll_period_seconds": 0.1,
                "pick_race_timer_seconds": 15,
                "pick_race_finishes_when_no_human_choices_remain_or_timer_expires": True,
            },
        },
        "related_rawcode_ids": [],
        "source_functions": [
            listener_name, parse_name, initializer_name,
            "StartResourceMode_new_StartResourceMode", "StartResourceMode_StartResourceMode_execute",
            "StartResourceMode_StartResourceMode_isValidChoice", "StartResourceMode_StartResourceMode_minForChoice",
            "StartResourceMode_StartResourceMode_applyChoice", mode_round_watch, ultimate_round_watch,
            "startNextRoundViaModeRuntime", "player_allowAllBuildings", "clearUltiTexttags",
            *periodic_mode_sources.keys(), *callback_functions,
        ],
        "evidence_kind": "exact-readable-mode-registry-generated-closure-aliases-and-chat-parser-control-flow",
        "byte_offset": min(initializer_start, listener_start, parse_start),
    }]


def _extract_runtime_session_mechanics(
    data: bytes,
    functions: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Normalize live player-session state that changes control/ownership or match flow."""
    functions_by_name = {str(row["name"]): row for row in functions}
    required = {
        "applyNoAfkMode", "beginRoundStartModeSection", "completeRoundStart",
        "startIdleDetectionIfEnabled", "stopIdleDetection", "checkPlayerIdle", "recordPlayerAction",
        "togglePlayerAway", "campaignBlocksAwayControl",
        "CallbackPeriodic_doPeriodically_IdleDetectionRuntime_call_doPeriodically_IdleDetectionRuntime",
        "Action_watch_IdleDetectionRuntime_run_watch_IdleDetectionRuntime",
        "EventListener_add_IdleDetectionRuntime_onEvent_add_IdleDetectionRuntime",
        "EventListener_add_IdleDetectionRuntime_onEvent_add_IdleDetectionRuntime1",
        "applyAutobalanceMode", "applyLeaveAutobalance", "redistributeAllLeaverUnits",
        "shareDependentSlotsWithRemainingTeam", "enableAiForLeaverIfNeeded",
        "clearAwayStateOnLeave", "cleanupPlayerAfkOnLeave", "hasRemainingPlayingTeamMember",
        "EventListener_add_PlayerLeave_onEvent_add_PlayerLeave",
        "EventListener_add_doAfter_MMDData_onEvent_add_doAfter_MMDData", "handlePlayerLeave",
        "CallbackSingle_doAfter_MMDData_call_doAfter_MMDData2",
        "Action_watch_RoundEndRuntime_run_watch_RoundEndRuntime", "onAllVotedDraw",
        "beginNextRoundReview", "CallbackPeriodic_doPeriodically_RoundEndRuntime_call_doPeriodically_RoundEndRuntime",
        "queueNextRound",
    }
    if not required.issubset(functions_by_name):
        return []

    def source(name: str, fragments: Iterable[bytes]) -> tuple[int, bytes]:
        row = functions_by_name[name]
        body = data[int(row["start"]):int(row["end"])]
        for fragment in fragments:
            if fragment not in body:
                raise ValueError(f"runtime session mechanic source changed: {name}: missing {fragment!r}")
        return int(row["start"]), body

    rows: list[dict[str, object]] = []
    if data.count(b"KX=1 HX=1 EX=(-1)") != 1:
        raise ValueError("runtime session default autobalance mode initializer changed")

    away_sources = [
        ("applyNoAfkMode", (b"rX=MQq", b"No AFK")),
        ("beginRoundStartModeSection", (b"if rX then Lab=true end",)),
        ("completeRoundStart", (b"startIdleDetectionIfEnabled()",)),
        ("startIdleDetectionIfEnabled", (b"if(not Lab)then return", b"Mab=doPeriodically(1.,AEq)")),
        ("stopIdleDetection", (b"CallbackPeriodic_destroyCallbackPeriodic",)),
        ("Action_watch_IdleDetectionRuntime_run_watch_IdleDetectionRuntime", (b"Signal_Signal_get(ZW)", b"stopIdleDetection()")),
        ("CallbackPeriodic_doPeriodically_IdleDetectionRuntime_call_doPeriodically_IdleDetectionRuntime", (b"if(XCm>11)then break", b"checkPlayerIdle(XCm)")),
        ("checkPlayerIdle", (b"zEq==20", b"zEq==30", b"togglePlayerAway(xEq)", b"zEq==60", b"zEq==120", b"(VGb==0)and(UGb<40)")),
        ("EventListener_add_IdleDetectionRuntime_onEvent_add_IdleDetectionRuntime", (b"if __wurst_ensureBool(dGb[bDm])then togglePlayerAway(bDm)end", b"recordPlayerAction(aDm)")),
        ("EventListener_add_IdleDetectionRuntime_onEvent_add_IdleDetectionRuntime1", (b"recordPlayerAction(GetTriggerPlayer())",)),
        ("recordPlayerAction", (b"iY[player_getId(wEq)]=getRoundTimeSeconds()",)),
        ("togglePlayerAway", (b"dGb[vvo]=true", b"dGb[vvo]=false", b"ALLIANCE_SHARED_CONTROL", b"bj_ALLIANCE_ALLIED_ADVUNITS", b"updatePlayerAlliances()")),
        ("campaignBlocksAwayControl", (b"AFK and away control are disabled during campaign.", b"return true")),
    ]
    away_offsets = [source(name, fragments)[0] for name, fragments in away_sources]
    rows.append({
        "system_id": "away-control-and-idle-detection",
        "mechanic_kind": "player-inactivity-driven-allied-control-sharing",
        "trigger": "round-start-periodic-idle-check-plus-player-activity/manual-away-toggle",
        "parameters": {
            "automatic_idle_detection_enabled_by_no_afk_mode": True,
            "idle_check_interval_seconds": 1,
            "player_slots_checked": [0, 11],
            "round_start_fast_window_seconds": 40,
            "round_start_warning_idle_seconds": 20,
            "round_start_auto_away_idle_seconds": 30,
            "general_warning_idle_seconds": 60,
            "general_auto_away_idle_seconds": 120,
            "player_activity_clears_away_immediately": True,
            "manual_away_blocked_during_campaign": True,
            "away_grants_allied_advanced_unit_control": True,
            "away_restores_prior_shared_control_state_when_cleared": True,
            "afk_state_blocks_manual_away_toggle": True,
            "automatic_detection_starts_each_round_when_enabled": True,
            "round_end_signal_stops_automatic_detection": True,
        },
        "related_rawcode_ids": [],
        "source_functions": [name for name, _fragments in away_sources],
        "evidence_kind": "exact-round-start-periodic-idle-thresholds-and-control-sharing-state",
        "byte_offset": min(away_offsets),
    })

    leave_sources = [
        ("applyAutobalanceMode", (b"if((eRq<0)or(eRq>2))", b"HX=eRq", b"eRq==0", b"eRq==1", b"AI will take control")),
        ("EventListener_add_PlayerLeave_onEvent_add_PlayerLeave", (b"voteForDraw(rin)", b"voteForNuke(rin)", b"clearAwayStateOnLeave(rin)", b"cleanupPlayerAfkOnLeave(rin)", b"hasRemainingPlayingTeamMember(qin)", b"if dY then", b"if eY then enableAiForLeaverIfNeeded(rin)else applyLeaveAutobalance",)),
        ("clearAwayStateOnLeave", (b"togglePlayerAway(Ser)", b"dGb[Ser]=false")),
        ("cleanupPlayerAfkOnLeave", (b"fGb[mvo]=false", b"eGb[mvo]=false", b"clearPendingVote", b"SetPlayerName")),
        ("hasRemainingPlayingTeamMember", (b"countRemainingPlayingTeamMembers",)),
        ("applyLeaveAutobalance", (b"if(HX==0)then redistributeAllLeaverUnits", b"elseif(HX==1)then shareDependentSlotsWithRemainingTeam", b"else enableAiForLeaverIfNeeded")),
        ("redistributeAllLeaverUnits", (b"redistributePlayerAssetsToTeam", b"ForceClear", b"DestroyForce")),
        ("shareDependentSlotsWithRemainingTeam", (b"markBuilderSharedControl", b"updatePlayerAlliances()", b"DestroyForce")),
        ("enableAiForLeaverIfNeeded", (b"TriggerExecute",)),
        ("EventListener_add_doAfter_MMDData_onEvent_add_doAfter_MMDData", (b"if(getElapsedGameTime()<300.0)then oUm=3 else oUm=1 end", b"doAfter(0.25,pUm)", b"handlePlayerLeave(mUm,0)", b"handlePlayerLeave(mUm,1)")),
        ("handlePlayerLeave", (b"doAfter(1.,ZMq)",)),
        ("CallbackSingle_doAfter_MMDData_call_doAfter_MMDData2", (b"countPlayingPlayersInForce", b"if(BTm==0)then", b"setMatchWinnerTeamIndex", b"recordMatchResultNow", b"cleanupRoundUnits()")),
    ]
    leave_offsets = [source(name, fragments)[0] for name, fragments in leave_sources]
    rows.append({
        "system_id": "player-leave-autobalance-and-team-empty-resolution",
        "mechanic_kind": "leave-cleanup-autobalance-and-match-termination",
        "trigger": "player-leave-event",
        "parameters": {
            "autobalance_modes": {
                "0": "redistribute all leaver-controlled player assets across remaining team",
                "1": "share dependent player slots among remaining team members",
                "2": "enable AI control for the leaver",
            },
            "default_mode_value_observed_in_runtime_initializer": 1,
            "leave_clears_away_state": True,
            "leave_cleans_afk_state_and_pending_vote": True,
            "leave_votes_are_removed_from_draw_and_nuke_votes": True,
            "autobalance_skipped_if_no_remaining_playing_teammate": True,
            "autobalance_skipped_after_match_end": True,
            "special_eY_path_enables_ai_directly": True,
            "mmd_leave_flag_before_300_seconds": 3,
            "mmd_leave_flag_at_or_after_300_seconds": 1,
            "mmd_leave_record_delay_seconds": 0.25,
            "team_empty_check_delay_seconds": 1,
            "team_empty_sets_opponent_match_winner": True,
            "team_empty_stops_round_and_cleans_round_units": True,
        },
        "related_rawcode_ids": [],
        "source_functions": [name for name, _fragments in leave_sources],
        "evidence_kind": "exact-leave-listener-autobalance-mode-dispatch-and-delayed-team-empty-resolution",
        "byte_offset": min(leave_offsets),
    })

    draw_sources = [
        ("Action_watch_RoundEndRuntime_run_watch_RoundEndRuntime", (
            b"Signal_Signal_get(XW)", b"if(DCn<=0)then return", b"onAllVotedDraw()",
        )),
        ("onAllVotedDraw", (
            b"if(dY or(not isRoundStarted()))then return", b"bY=false", b"Signal_Signal_set(ZW",
            b"cleanupAppliedPerks()", b"showRoundEndStats()", b"cleanupRoundUnits()", b"doAfter(1.,OEr)",
        )),
    ]
    draw_offsets = [source(name, fragments)[0] for name, fragments in draw_sources]
    rows.append({
        "system_id": "unanimous-draw-round-restart",
        "mechanic_kind": "round-end-signal-draw-restart-and-round-cleanup",
        "trigger": "all-players-voted-draw-signal",
        "parameters": {
            "ignored_after_match_end": True,
            "ignored_when_round_not_started": True,
            "marks_round_not_started": True,
            "publishes_round_end_signal": True,
            "cleans_applied_perks": True,
            "stops_round_runtime_and_timers": True,
            "shows_round_end_stats": True,
            "cleans_round_units": True,
            "restart_delay_seconds": 1,
            "does_not_set_match_winner": True,
        },
        "related_rawcode_ids": [],
        "source_functions": [name for name, _fragments in draw_sources],
        "evidence_kind": "exact-round-end-watch-and-readable-unanimous-draw-cleanup-control-flow",
        "byte_offset": min(draw_offsets),
    })

    if data.count(b"AY=15 zY=5 yY=90") != 1:
        raise ValueError("round-end review duration initializer changed")
    review_sources = [
        ("beginNextRoundReview", (
            b"Signal_Signal_set(lY,AY)", b"b0=doPeriodically(1.,LEr)",
        )),
        ("CallbackPeriodic_doPeriodically_RoundEndRuntime_call_doPeriodically_RoundEndRuntime", (
            b"if(hasMatchWinner()or dY)then", b"Signal_Signal_set(lY,0)",
            b"yCn=(__wurst_ensureInt(Signal_Signal_peek(lY))-1)", b"Signal_Signal_set(lY,max1(0,yCn))",
            b"if(yCn<=0)then", b"queueNextRound()",
        )),
        ("queueNextRound", (
            b"if(hasMatchWinner()or dY)then return", b"lockRoundStatsBoard()",
            b"if PGb then Signal_Signal_set(YW", b"else Signal_Signal_set(bX",
        )),
    ]
    review_offsets = [source(name, fragments)[0] for name, fragments in review_sources]
    rows.append({
        "system_id": "round-end-review-countdown",
        "mechanic_kind": "fifteen-second-one-second-tick-next-round-review-gate",
        "trigger": "round-cleanup-complete-without-match-winner",
        "parameters": {
            "duration_seconds": 15,
            "period_seconds": 1,
            "countdown_signal_symbol": "lY",
            "stops_and_clears_countdown_if_match_winner_exists": True,
            "stops_and_clears_countdown_if_match_end_flag_set": True,
            "countdown_clamped_minimum": 0,
            "at_zero_locks_round_stats_board": True,
            "next_round_signal_when_PGb": "YW",
            "next_round_signal_otherwise": "bX",
        },
        "related_rawcode_ids": [],
        "source_functions": [name for name, _fragments in review_sources],
        "evidence_kind": "exact-readable-round-review-periodic-countdown-control-flow",
        "byte_offset": min(review_offsets),
    })
    return rows


def _extract_runtime_campaign_mechanics(
    data: bytes,
    functions: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Normalize campaign challenge mutations reachable from authored event listeners."""
    functions_by_name = {str(row["name"]): row for row in functions}
    required = {
        "EventListener_add_CampaignChallenges_onEvent_add_CampaignChallenges",
        "recordCampaignTrackedBuildingLost",
        "EventListener_add_ShopAnnouncements_onEvent_add_ShopAnnouncements",
        "recordCampaignOwnerItemPurchase__w3p_vmProtect",
        "isActiveRestriction",
        "failCampaignChallenge",
    }
    if not required.issubset(functions_by_name):
        return []

    def source(name: str, fragments: Iterable[bytes]) -> tuple[int, bytes]:
        row = functions_by_name[name]
        body = data[int(row["start"]):int(row["end"])]
        for fragment in fragments:
            if fragment not in body:
                raise ValueError(f"runtime campaign mechanic source changed: {name}: missing {fragment!r}")
        return int(row["start"]), body

    def decode_restriction_global(symbol: bytes) -> str:
        match = re.search(
            rb"(?<![A-Za-z0-9_])" + symbol
            + rb'=\(_d\[\d+\]or _y\(\d+,(_T\("(?:\\.|[^"\\])*"\))\)\)',
            data,
        )
        if match is None:
            raise ValueError(f"campaign restriction global assignment changed: {symbol.decode('ascii')}")
        return _decode_w3p_global_name(match.group(1), 11351, 1106)

    no_buildings_id = decode_restriction_global(b"arb")
    no_items_id = decode_restriction_global(b"crb")
    if no_buildings_id != "challenge_no_buildings_lost":
        raise ValueError(f"campaign no-buildings restriction id changed: {no_buildings_id!r}")
    if no_items_id != "challenge_no_items":
        raise ValueError(f"campaign no-items restriction id changed: {no_items_id!r}")

    tracked_listener = "EventListener_add_CampaignChallenges_onEvent_add_CampaignChallenges"
    shop_listener = "EventListener_add_ShopAnnouncements_onEvent_add_ShopAnnouncements"
    sources = [
        (tracked_listener, (
            b"GetTriggerUnit()", b"IsUnitInGroup", b"GroupRemoveUnit", b"recordCampaignTrackedBuildingLost()",
        )),
        ("recordCampaignTrackedBuildingLost", (
            b"isActiveRestriction(arb)", b"failCampaignChallenge(arb", b"a player-built building was destroyed.", b"true",
        )),
        (shop_listener, (
            b"GetBuyingUnit()", b"GetSoldItem()", b"recordCampaignOwnerItemPurchase__w3p_vmProtect(qLn)",
        )),
        ("recordCampaignOwnerItemPurchase__w3p_vmProtect", (
            b"isChallengeBoundPlayer(unit_getOwner(fNp))", b"isActiveRestriction(crb)",
            b"failCampaignChallenge(crb", b"a player bought an item.", b"true",
        )),
        ("isActiveRestriction", (
            b"CampaignMission_secondStar", b"CampaignMission_thirdStar", b"starRestrictionId",
        )),
        ("failCampaignChallenge", (
            b"CampaignMission_secondStar", b"CampaignMission_thirdStar", b"Signal_Signal_set",
        )),
    ]
    offsets = [source(name, fragments)[0] for name, fragments in sources]

    rows = [{
        "system_id": "campaign-star-restriction-failure-hooks",
        "mechanic_kind": "campaign-active-star-restriction-event-failure",
        "trigger": "tracked-building-death-or-challenge-bound-player-item-purchase",
        "parameters": {
            "active_restriction_scope": ["second-star", "third-star"],
            "tracked_building_loss": {
                "restriction_id": no_buildings_id,
                "listener_function": tracked_listener,
                "requires_unit_in_campaign_tracked_building_group": True,
                "removes_lost_unit_from_tracking_group": True,
                "failure_reason": "a player-built building was destroyed.",
                "sets_hard_failure_flag": True,
            },
            "challenge_bound_item_purchase": {
                "restriction_id": no_items_id,
                "listener_function": shop_listener,
                "requires_buying_unit_owner_is_challenge_bound_player": True,
                "failure_reason": "a player bought an item.",
                "sets_hard_failure_flag": True,
            },
        },
        "related_rawcode_ids": [],
        "source_functions": [name for name, _fragments in sources],
        "evidence_kind": "exact-readable-campaign-listeners-plus-statically-decoded-protected-restriction-ids",
        "byte_offset": min(offsets),
    }]

    challenge_wrapper = "startCampaignCastleHealthChallenge__w3p_vmProtect"
    challenge_callback = "CallbackPeriodic_doPeriodically_CampaignChallenges_call_doPeriodically_CampaignChallenges"
    challenge_sources = [
        ("Action_watch_CampaignRuntime_run_watch_CampaignRuntime", (
            b"Signal_Signal_get(aX)", b"finishPendingCampaignMissionStart__w3p_vmProtect()",
        )),
        (challenge_wrapper, (b"return _qr(23)",)),
        (challenge_callback, (
            b"recordCampaignChallengeElapsedSeconds((getElapsedGameTime()-tqb))",
            b"if(not iYk.tracksCastleHealth)then return",
            b"if(not(kYk==zqb))then zqb=kYk yqb=false end",
            b"if((lYk>0.)and(unit_getMaxHP(kYk)>0.))then yqb=true end",
            b"recordCampaignCastleHealthPercent(real_toInt((lYk*100.)))",
        )),
        ("recordCampaignChallengeElapsedSeconds", (
            b"starTracksFastWin(PMp.CampaignMission_secondStar)", b"OMp>int_toReal",
            b"starTracksFastWin(PMp.CampaignMission_thirdStar)", b"the completion time limit expired.",
        )),
        ("recordCampaignCastleHealthPercent", (
            b"RMp<SMp.CampaignMission_secondStar.CampaignStarChallenge_minimumCastleHpPercent",
            b"RMp<SMp.CampaignMission_thirdStar.CampaignStarChallenge_minimumCastleHpPercent",
            b"castle health dropped below the required minimum.",
        )),
        ("finishPendingCampaignMissionStart__w3p_vmProtect", (
            b"startCampaignCastleHealthChallenge__w3p_vmProtect()", b"startCampaignSurvivalTimer(mTp,lTp)",
        )),
    ]
    challenge_offsets = [source(name, fragments)[0] for name, fragments in challenge_sources]
    challenge_static = _w3p_vm_static_strings(data, 23)
    challenge_globals = [
        _decode_w3p_global_name(expression, 11351, 1106)
        for expression in _w3p_vm_global_expressions(data, 23)
    ]
    if len(challenge_static) < 15 or challenge_static[10] != "0.25" or challenge_static[6] != "create165":
        raise ValueError(f"campaign challenge VM static setup changed: {challenge_static}")
    if len(challenge_globals) < 15 or challenge_globals[13] != "df" or challenge_globals[14] != "doPeriodically":
        raise ValueError(f"campaign challenge VM callback globals changed: {challenge_globals}")
    challenge_program = list(_decode_w3p_vm_program(data, 23)["instructions"])
    if not any(
        int(left["opcode"]) == 144 and list(left["operands"]) == [11]
        and int(right["opcode"]) == 42 and list(right["operands"]) == [15, 33]
        for index, left in enumerate(challenge_program)
        for right in challenge_program[index + 1:index + 4]
    ):
        raise ValueError("campaign challenge VM no longer feeds static 0.25 into doPeriodically")
    rows.append({
        "system_id": "campaign-star-periodic-objectives",
        "mechanic_kind": "quarter-second-fast-win-and-castle-health-star-tracking",
        "trigger": "campaign-mission-start-protected-periodic-callback",
        "parameters": {
            "period_seconds": 0.25,
            "protected_setup_vm_index": 23,
            "elapsed_time_source": "getElapsedGameTime()-campaignChallengeStartedAt",
            "fast_win_failure_comparison": "elapsed_seconds > configured_fast_win_seconds",
            "fast_win_comparison_is_strict": True,
            "castle_health_percent_source": "real_toInt(unit_hp_ratio*100)",
            "castle_health_failure_comparison": "health_percent < configured_minimum_castle_hp_percent",
            "castle_health_comparison_is_strict": True,
            "castle_health_baseline_waits_for_positive_current_and_max_hp": True,
            "castle_health_baseline_resets_when_tracked_castle_reference_changes": True,
            "callback_self_stops_if_campaign_mission_changes_or_challenge_is_locked": True,
            "active_star_slots": ["second-star", "third-star"],
        },
        "related_rawcode_ids": [],
        "source_functions": [name for name, _fragments in challenge_sources],
        "evidence_kind": "readable-periodic-objective-control-flow-plus-statically-decoded-w3p-vm23-period",
        "byte_offset": min(challenge_offsets),
    })

    survival_sources = [
        ("Action_watch_CampaignRuntime_run_watch_CampaignRuntime", (
            b"Signal_Signal_get(aX)", b"finishPendingCampaignMissionStart__w3p_vmProtect()",
        )),
        ("startCampaignSurvivalTimer", (
            b"CampaignMission_survivalSeconds<=0", b"Dpb=GSp.CampaignMission_survivalSeconds",
            b"Epb=doPeriodically(1.,KSp)",
        )),
        ("CallbackPeriodic_doPeriodically_CampaignRuntime_call_doPeriodically_CampaignRuntime", (
            b"if(((not bqb)or(not(Zpb==wYk.mission)))or dY)then stopCampaignSurvivalTimer()return",
            b"Dpb=(Dpb-1)", b"updateCampaignSurvivalTimer(zYk,Dpb)", b"if(Dpb<=0)then",
            b"noteRoundVictoryCondition(LY)", b"KillUnit",
        )),
        ("finishPendingCampaignMissionStart__w3p_vmProtect", (
            b"startCampaignCastleHealthChallenge__w3p_vmProtect()", b"startCampaignSurvivalTimer(mTp,lTp)",
        )),
    ]
    survival_offsets = [source(name, fragments)[0] for name, fragments in survival_sources]
    rows.append({
        "system_id": "campaign-survival-countdown",
        "mechanic_kind": "one-second-survival-objective-countdown-and-castle-kill-victory",
        "trigger": "campaign-mission-start-with-positive-survival-seconds",
        "parameters": {
            "period_seconds": 1,
            "initial_seconds": "CampaignMission_survivalSeconds",
            "updates_visible_timer_for_campaign_player_list_each_tick": True,
            "stops_if_campaign_flow_inactive": True,
            "stops_if_active_mission_changes": True,
            "stops_if_match_has_ended": True,
            "completion_condition": "remaining_seconds <= 0",
            "completion_sets_round_victory_condition": "LY",
            "completion_kills_owner_team_castle": True,
            "castle_team_selection": "western owner force -> team 0; otherwise team 1",
        },
        "related_rawcode_ids": [],
        "source_functions": [name for name, _fragments in survival_sources],
        "evidence_kind": "exact-readable-campaign-survival-periodic-control-flow",
        "byte_offset": min(survival_offsets),
    })
    return rows


def _extract_runtime_draft_mechanics(
    data: bytes,
    functions: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Normalize readable draft lifecycle callbacks outside individual perk semantics."""
    functions_by_name = {str(row["name"]): row for row in functions}
    required = {
        "Action_watch_DraftOrchestrator_run_watch_DraftOrchestrator", "restartDraftAfterRoundEnd",
        "Action_watch_DraftPerkRegistry_run_watch_DraftPerkRegistry", "ensureAppliedList", "addPerkReminderToBuilder",
        "initPerks__w3p_vmProtect", "initializeDefaultDraftTiers__w3p_vmProtect",
        "setupDefaultRoundOrder__w3p_vmProtect", "syncDraftPlayersFromForces",
    }
    if not required.issubset(functions_by_name):
        return []

    def source(name: str, fragments: Iterable[bytes]) -> tuple[int, bytes]:
        row = functions_by_name[name]
        body = data[int(row["start"]):int(row["end"])]
        for fragment in fragments:
            if fragment not in body:
                raise ValueError(f"runtime draft mechanic source changed: {name}: missing {fragment!r}")
        return int(row["start"]), body

    restart_sources = [
        ("Action_watch_DraftOrchestrator_run_watch_DraftOrchestrator", (b"Signal_Signal_get(YW)", b"restartDraftAfterRoundEnd()")),
        ("restartDraftAfterRoundEnd", (
            b"if(Sfb==nil)then return", b"Mcb=true", b"resetPool()", b"initializeDefaultDraftTiers__w3p_vmProtect()",
            b"initPerks__w3p_vmProtect()", b"assertDraftPerkIdHash__w3p_vmProtect()", b"setupDefaultRoundOrder__w3p_vmProtect()",
            b"syncDraftPlayersFromForces()", b"DraftController_DraftController_reset(Sfb)",
            b"IterableMap_IterableMap_forEach(Vdb,xtq)", b"DraftController_DraftController_startWithWarmup(Sfb)",
        )),
        ("initPerks__w3p_vmProtect", (b"return _qr(65)",)),
    ]
    restart_offsets = [source(name, fragments)[0] for name, fragments in restart_sources]

    reminder_sources = [
        ("Action_watch_DraftPerkRegistry_run_watch_DraftPerkRegistry", (
            b"Signal_Signal_get(aX)", b"if(Khm>11)then break", b"ensureAppliedList(Khm)",
            b"LinkedList_LinkedList_iterator(Oeb[Khm])", b"addPerkReminderToBuilder(Lhm,Nhm)",
        )),
        ("ensureAppliedList", (b"if(Oeb[Itq]==nil)then Oeb[Itq]=LinkedList_new_LinkedList()end",)),
        ("addPerkReminderToBuilder", (
            b"DraftPerk_reminderAbilityId==0", b"jX[player_getId(fuq)]", b"addProtectedAbility",
            b"unit_makeAbilityPermanent", b"ABILITY_RLF_CHANCE_TO_CRITICAL_STRIKE", b"BlzSetAbilityRealLevelField",
        )),
    ]
    reminder_offsets = [source(name, fragments)[0] for name, fragments in reminder_sources]

    timer_sources = [
        ("DraftController_DraftController_init", (
            b"DraftController_rerollsPerPlayer=5", b"DraftController_totalRounds=5", b"DraftController_secondsPerRound=20",
            b"if Pcb then Rdm=7 else Rdm=0 end", b"DraftController_postDraftDelaySeconds=Rdm",
            b"if Pcb then Sdm=60 else Sdm=10 end", b"DraftController_warmupSecondsDefault=Sdm",
        )),
        ("DraftController_new_DraftController", (
            b"DraftController_DraftController_init(Xdm)", b"DraftController_rerollsPerPlayer=Tdm",
            b"DraftController_totalRounds=Udm", b"DraftController_secondsPerRound=Vdm",
        )),
        ("DraftController_DraftController_startWithWarmup", (
            b"DraftController_warmupSecondsDefault<=0", b"DraftController_DraftController_beginRound(lbm)",
            b"DraftController_DraftController_beginWarmup(lbm,lbm.DraftController_warmupSecondsDefault",
        )),
        ("DraftController_DraftController_beginWarmup", (
            b"DraftController_warmupLeft=pbm", b"DraftEventBus_fireWarmupStart", b"doPeriodically(1.0,vbm)",
        )),
        ("CallbackPeriodic_doPeriodically_DraftController_DraftData_call_doPeriodically_DraftController_DraftData", (
            b"DraftController_warmupLeft=(cem.this.DraftController_warmupLeft-1)", b"DraftEventBus_fireWarmupTick",
            b"DraftController_DraftController_stopWarmup", b"DraftEventBus_fireWarmupEnd", b"MMD_flagPlayer(gem,0)",
            b"RemovePlayerPreserveUnitsBJ",
        )),
        ("DraftController_DraftController_startCountdown", (
            b"DraftController_timeLeft=ecm", b"DraftEventBus_fireTimer", b"doPeriodically(1.0,fcm)",
        )),
        ("CallbackPeriodic_doPeriodically_DraftController_DraftData_call_doPeriodically_DraftController_DraftData1", (
            b"DraftController_DraftController_autoPickAiAndAway", b"DraftController_timeLeft=(Eem.this.DraftController_timeLeft-1)",
            b"DraftController_DraftController_allPicked", b"DraftController_DraftController_autoPickRemaining",
            b"DraftController_DraftController_endRound",
        )),
        ("DraftController_DraftController_startPostDraftCountdown", (
            b"DraftController_timeLeft=Idm.DraftController_postDraftDelaySeconds", b"doPeriodically(1.0,Jdm)",
        )),
        ("CallbackPeriodic_doPeriodically_DraftController_DraftData_call_doPeriodically_DraftController_DraftData2", (
            b"DraftController_timeLeft=(Hem.this.DraftController_timeLeft-1)",
            b"DraftController_DraftController_stopPostDraftCountdown", b"DraftController_DraftController_finalizeDraft",
        )),
    ]
    timer_offsets = [source(name, fragments)[0] for name, fragments in timer_sources]

    return [
        {
            "system_id": "draft-round-restart-lifecycle",
            "mechanic_kind": "round-end-draft-controller-reset-and-warmup-restart",
            "trigger": "draft-round-end-signal",
            "parameters": {
                "requires_existing_draft_controller": True,
                "resets_unit_pool": True,
                "reinitializes_default_draft_tiers": True,
                "reinitializes_proven_19_perk_registry": True,
                "asserts_perk_id_hash": True,
                "rebuilds_default_round_order": True,
                "resyncs_draft_players_from_team_forces": True,
                "resets_existing_draft_controller": True,
                "reapplies_registered_draft_state_over_registry_map": True,
                "restarts_controller_with_warmup": True,
            },
            "related_rawcode_ids": [],
            "source_functions": [name for name, _fragments in restart_sources],
            "evidence_kind": "exact-readable-draft-restart-control-flow-plus-proven-protected-perk-registry-entrypoint",
            "byte_offset": min(restart_offsets),
        },
        {
            "system_id": "draft-controller-periodic-timers",
            "mechanic_kind": "one-second-warmup-pick-and-post-draft-countdowns",
            "trigger": "draft-controller-warmup-round-and-post-draft-phases",
            "parameters": {
                "period_seconds": 1,
                "initializer_defaults": {
                    "rerolls_per_player": 5,
                    "total_rounds": 5,
                    "seconds_per_round": 20,
                    "post_draft_delay_seconds_when_Pcb": 7,
                    "post_draft_delay_seconds_otherwise": 0,
                    "warmup_seconds_when_Pcb": 60,
                    "warmup_seconds_otherwise": 10,
                },
                "constructor_overrides_initializer_fields": ["rerolls_per_player", "total_rounds", "seconds_per_round"],
                "warmup_behavior": "decrement warmupLeft; emit tick; at zero stop warmup, emit warmup-end, MMD-flag active team players, then neutral-result-remove players",
                "pick_countdown_behavior": "auto-pick AI/AWAY each tick; decrement timer; if expired or all picked, auto-pick remaining and end draft round",
                "post_draft_behavior": "decrement timer; at zero stop post-draft countdown and finalize draft",
                "start_with_warmup_skips_warmup_when_nonpositive": True,
            },
            "related_rawcode_ids": [],
            "source_functions": [name for name, _fragments in timer_sources],
            "evidence_kind": "exact-readable-draft-controller-periodic-timer-control-flow",
            "byte_offset": min(timer_offsets),
        },
        {
            "system_id": "draft-perk-reminder-reapply",
            "mechanic_kind": "round-start-reapply-applied-perk-reminder-abilities-to-current-builders",
            "trigger": "round-start-signal",
            "parameters": {
                "player_ids_scanned": [0, 11],
                "applied_perk_list_symbol": "Oeb[player_id]",
                "current_builder_symbol": "jX[player_id]",
                "skips_perks_with_zero_reminder_ability_id": True,
                "adds_missing_reminder_ability_only": True,
                "makes_reminder_ability_permanent": True,
                "sets_ability_real_level_field": "ABILITY_RLF_CHANCE_TO_CRITICAL_STRIKE",
                "sets_field_level_index": 0,
                "sets_field_value": 0.0,
            },
            "related_rawcode_ids": [],
            "source_functions": [name for name, _fragments in reminder_sources],
            "evidence_kind": "exact-readable-applied-perk-list-builder-reminder-reapply-control-flow",
            "byte_offset": min(reminder_offsets),
        },
    ]


def _extract_damage_listener_coverage(
    functions: list[dict[str, object]],
    production_unit_special_mechanics: list[dict[str, object]],
    runtime_system_mechanics: list[dict[str, object]],
    building_spell_mechanics: list[dict[str, object]],
    perk_mechanics: list[dict[str, object]],
    runtime_ai_mechanics: list[dict[str, object]],
    protected_perk_registry_audit: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Strict closure audit for authored DamageListener callbacks.

    Every authored damage listener must either be referenced by normalized
    gameplay/AI-runtime semantics or be explicitly classified as telemetry/E2E.
    The fallback classifications remain fail-loud guards for future map drift.
    """
    if not protected_perk_registry_audit:
        return []

    normalized_sources: dict[str, set[str]] = defaultdict(set)

    def add_sources(domain: str, rows: list[dict[str, object]], identity_key: str) -> None:
        for row in rows:
            identity = str(row[identity_key])
            for function_name in row.get("source_functions", []):
                normalized_sources[str(function_name)].add(f"{domain}:{identity}")

    add_sources("production-unit-special-mechanics", production_unit_special_mechanics, "mechanic_kind")
    add_sources("runtime-system-mechanics", runtime_system_mechanics, "system_id")
    add_sources("building-spell-mechanics", building_spell_mechanics, "mechanic_kind")
    add_sources("perk-mechanics", perk_mechanics, "perk_id")
    add_sources("runtime-ai-mechanics", runtime_ai_mechanics, "system_id")

    perk_by_listener = {
        str(row["damage_listener_function"]): row
        for row in protected_perk_registry_audit
        if row["damage_listener_function"]
    }
    function_offsets = {str(row["name"]): int(row["start"]) for row in functions}
    listener_names = sorted(
        name for name in function_offsets
        if name.startswith("DamageListener_addListener_") or name.startswith("DamageListener_perkListenDamage_")
    )
    if len(listener_names) != 20:
        raise ValueError(f"damage-listener callback inventory changed: {len(listener_names)}")

    e2e_only = {
        "DamageListener_addListener_BuildingCatalogE2E_onEvent_addListener_BuildingCatalogE2E",
    }
    telemetry_only = {
        "DamageListener_addListener_RoundStatsTracking_onEvent_addListener_RoundStatsTracking",
    }
    ai_runtime = {
        "DamageListener_addListener_AiEngagement_onEvent_addListener_AiEngagement",
        "DamageListener_addListener_CustomAI_onEvent_addListener_CustomAI",
    }

    rows: list[dict[str, object]] = []
    for listener_name in listener_names:
        normalized = sorted(normalized_sources.get(listener_name, ()))
        perk_registration = perk_by_listener.get(listener_name)
        candidate_factory = str(perk_registration["factory_function"]) if perk_registration is not None else ""
        if normalized and all(source.startswith("runtime-ai-mechanics:") for source in normalized):
            status = "normalized-ai-runtime-semantics"
            note = "listener is referenced by importer-facing normalized AI-runtime semantics and does not rewrite damage"
        elif normalized:
            status = "normalized-gameplay-semantics"
            note = "listener is referenced by one or more importer-facing normalized gameplay mechanic rows"
        elif candidate_factory and bool(perk_registration["individual_factory_registration_proven"]):
            status = "registered-perk-semantics-unmodeled"
            note = (
                "listener belongs to an exactly proven protected-VM perk registration, but its gameplay semantics have not "
                "yet been promoted into importer-facing normalized mechanics"
            )
        elif candidate_factory:
            status = "protected-perk-registration-unresolved"
            note = (
                "listener semantics and readable perk factory are present, but the individual factory-to-protected-registry "
                "call target is not structurally proven"
            )
        elif listener_name in e2e_only:
            status = "e2e-only"
            note = "generated building-catalog end-to-end verification listener; not production gameplay semantics"
        elif listener_name in telemetry_only:
            status = "telemetry-only"
            note = "round statistics observer records damage and does not define a gameplay damage rewrite"
        elif listener_name in ai_runtime:
            status = "runtime-ai-subsystem-unmodeled"
            note = "live/runtime AI observation or controller hook; tracked separately from combat mechanic normalization"
        else:
            raise ValueError(f"unclassified damage-listener callback: {listener_name}")
        rows.append({
            "listener_function": listener_name,
            "coverage_status": status,
            "candidate_perk_factory": candidate_factory,
            "normalized_sources": normalized,
            "evidence_note": note,
            "byte_offset": function_offsets[listener_name],
        })
    return rows


def _extract_action_watch_coverage(
    functions: list[dict[str, object]],
    production_unit_special_mechanics: list[dict[str, object]],
    runtime_system_mechanics: list[dict[str, object]],
    building_spell_mechanics: list[dict[str, object]],
    perk_mechanics: list[dict[str, object]],
    runtime_ai_mechanics: list[dict[str, object]],
    runtime_session_mechanics: list[dict[str, object]],
    runtime_mode_mechanics: list[dict[str, object]],
    runtime_campaign_mechanics: list[dict[str, object]],
    runtime_draft_mechanics: list[dict[str, object]],
    protected_perk_registry_audit: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Strict closure audit over generated Action_watch callbacks."""
    if not protected_perk_registry_audit:
        return []

    function_offsets = {str(row["name"]): int(row["start"]) for row in functions}
    callback_names = sorted(name for name in function_offsets if name.startswith("Action_watch_"))
    if len(callback_names) != 43:
        raise ValueError(f"Action_watch callback inventory changed: {len(callback_names)}")

    normalized_sources: dict[str, set[str]] = defaultdict(set)

    def add_sources(domain: str, rows: list[dict[str, object]], identity_key: str) -> None:
        for row in rows:
            identity = str(row[identity_key])
            for function_name in row.get("source_functions", []):
                normalized_sources[str(function_name)].add(f"{domain}:{identity}")

    add_sources("production-unit-special-mechanics", production_unit_special_mechanics, "mechanic_kind")
    add_sources("runtime-system-mechanics", runtime_system_mechanics, "system_id")
    add_sources("building-spell-mechanics", building_spell_mechanics, "mechanic_kind")
    add_sources("perk-mechanics", perk_mechanics, "perk_id")
    add_sources("runtime-ai-mechanics", runtime_ai_mechanics, "system_id")
    add_sources("runtime-session-mechanics", runtime_session_mechanics, "system_id")
    add_sources("runtime-mode-mechanics", runtime_mode_mechanics, "system_id")
    add_sources("runtime-campaign-mechanics", runtime_campaign_mechanics, "system_id")
    add_sources("runtime-draft-mechanics", runtime_draft_mechanics, "system_id")

    e2e_only = {
        "Action_watch_AiFullGameE2E_run_watch_AiFullGameE2E",
        "Action_watch_AiFullGameE2E_run_watch_AiFullGameE2E1",
        "Action_watch_ModeRuntimeE2E_run_watch_ModeRuntimeE2E",
        "Action_watch_ModeRuntimeE2E_run_watch_ModeRuntimeE2E1",
    }
    presentation_only = {
        "Action_watch_CampaignMissionInfoOverlay_CampaignUI_run_watch_CampaignMissionInfoOverlay_CampaignUI",
        "Action_watch_CampaignUI_CampaignUI_run_watch_CampaignUI_CampaignUI",
        "Action_watch_CampaignUI_CampaignUI_run_watch_CampaignUI_CampaignUI1",
        "Action_watch_CampaignUI_CampaignUI_run_watch_CampaignUI_CampaignUI2",
        "Action_watch_CampaignUI_CampaignUI_run_watch_CampaignUI_CampaignUI3",
        "Action_watch_Campaign_run_watch_Campaign",
        "Action_watch_DraftUIScreen_DraftUI_run_watch_DraftUIScreen_DraftUI",
        "Action_watch_DraftUIScreen_DraftUI_run_watch_DraftUIScreen_DraftUI1",
        "Action_watch_DraftWindow_DraftWindow_run_watch_DraftWindow_DraftWindow",
        "Action_watch_IncomeUI_run_watch_IncomeUI",
        "Action_watch_InfoWindow_InfoWindow_run_watch_InfoWindow_InfoWindow",
        "Action_watch_MultiboardEventHooks_run_watch_MultiboardEventHooks",
        "Action_watch_MultiboardInit_run_watch_MultiboardInit",
        "Action_watch_ReactivePlayerMultiboard_PlayerMultiboard_run_watch_ReactivePlayerMultiboard_PlayerMultiboard",
        "Action_watch_doAfter_CameraMovementDetection_run_watch_doAfter_CameraMovementDetection",
        "Action_watch_doAfter_IncomeUI_run_watch_doAfter_IncomeUI",
        "Action_watch_doAfter_IncomeUI_run_watch_doAfter_IncomeUI1",
        "Action_watch_doAfter_RoundStatsBoard_run_watch_doAfter_RoundStatsBoard",
    }
    telemetry_only = {
        "Action_watch_RoundStatsTracking_run_watch_RoundStatsTracking",
        "Action_watch_doAfter_MMDData_run_watch_doAfter_MMDData",
        "Action_watch_doAfter_MMDData_run_watch_doAfter_MMDData1",
    }
    gameplay_framework = {
        "Action_watch_OnUnitDeathHandler_run_watch_OnUnitDeathHandler",
    }

    rows: list[dict[str, object]] = []
    for callback_name in callback_names:
        normalized = sorted(normalized_sources.get(callback_name, ()))
        if normalized:
            if all(source.startswith("runtime-session-mechanics:") for source in normalized):
                status = "normalized-session-runtime-semantics"
                note = "callback is direct lifecycle evidence for normalized player-session runtime semantics"
            elif all(source.startswith("runtime-mode-mechanics:") for source in normalized):
                status = "normalized-mode-runtime-semantics"
                note = "callback is direct lifecycle evidence for normalized mode runtime semantics"
            elif all(source.startswith("runtime-campaign-mechanics:") for source in normalized):
                status = "normalized-campaign-runtime-semantics"
                note = "callback is direct lifecycle evidence for normalized campaign runtime semantics"
            elif all(source.startswith("runtime-draft-mechanics:") for source in normalized):
                status = "normalized-draft-runtime-semantics"
                note = "callback is direct lifecycle evidence for normalized draft runtime semantics"
            elif all(source.startswith("runtime-ai-mechanics:") for source in normalized):
                status = "normalized-ai-runtime-semantics"
                note = "callback is direct lifecycle evidence for normalized AI runtime semantics"
            else:
                status = "normalized-gameplay-semantics"
                note = "callback is direct lifecycle evidence for importer-facing normalized gameplay semantics"
        elif callback_name in e2e_only:
            status = "e2e-only"
            note = "generated AI/mode end-to-end verification callback; not production gameplay semantics"
        elif callback_name in presentation_only:
            status = "presentation-only"
            note = "reactive UI/camera/multiboard callback; no authoritative gameplay mechanic mutation"
        elif callback_name in telemetry_only:
            status = "telemetry-only"
            note = "statistics/MMD timeline observer rather than authoritative gameplay semantics"
        elif callback_name in gameplay_framework:
            status = "gameplay-framework-infrastructure"
            note = "round-lifecycle cache invalidation used by normalized building-count mechanics; not an independent rule"
        else:
            raise ValueError(f"unclassified Action_watch callback: {callback_name}")
        rows.append({
            "callback_function": callback_name,
            "coverage_status": status,
            "normalized_sources": normalized,
            "evidence_note": note,
            "byte_offset": function_offsets[callback_name],
        })

    status_counts = Counter(str(row["coverage_status"]) for row in rows)
    expected_status_counts = Counter({
        "normalized-gameplay-semantics": 10,
        "normalized-session-runtime-semantics": 2,
        "normalized-mode-runtime-semantics": 2,
        "presentation-only": 18,
        "e2e-only": 4,
        "telemetry-only": 3,
        "normalized-draft-runtime-semantics": 2,
        "normalized-campaign-runtime-semantics": 1,
        "gameplay-framework-infrastructure": 1,
    })
    if status_counts != expected_status_counts:
        raise ValueError(f"Action_watch coverage classification changed: {dict(sorted(status_counts.items()))}")
    return rows


def _extract_callback_periodic_coverage(
    functions: list[dict[str, object]],
    production_unit_special_mechanics: list[dict[str, object]],
    runtime_system_mechanics: list[dict[str, object]],
    building_spell_mechanics: list[dict[str, object]],
    perk_mechanics: list[dict[str, object]],
    runtime_ai_mechanics: list[dict[str, object]],
    runtime_session_mechanics: list[dict[str, object]],
    runtime_mode_mechanics: list[dict[str, object]],
    runtime_campaign_mechanics: list[dict[str, object]],
    runtime_draft_mechanics: list[dict[str, object]],
    unit_spell_mechanics: list[dict[str, object]],
    protected_perk_registry_audit: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Strict closure audit over all generated CallbackPeriodic functions."""
    if not protected_perk_registry_audit:
        return []

    function_offsets = {str(row["name"]): int(row["start"]) for row in functions}
    callback_names = sorted(name for name in function_offsets if name.startswith("CallbackPeriodic_"))
    if len(callback_names) != 29:
        raise ValueError(f"CallbackPeriodic callback inventory changed: {len(callback_names)}")

    normalized_sources: dict[str, set[str]] = defaultdict(set)

    def add_sources(domain: str, rows: list[dict[str, object]], identity_key: str) -> None:
        for row in rows:
            identity = str(row[identity_key])
            for function_name in row.get("source_functions", []):
                normalized_sources[str(function_name)].add(f"{domain}:{identity}")

    add_sources("production-unit-special-mechanics", production_unit_special_mechanics, "mechanic_kind")
    add_sources("runtime-system-mechanics", runtime_system_mechanics, "system_id")
    add_sources("building-spell-mechanics", building_spell_mechanics, "mechanic_kind")
    add_sources("perk-mechanics", perk_mechanics, "perk_id")
    add_sources("runtime-ai-mechanics", runtime_ai_mechanics, "system_id")
    add_sources("runtime-session-mechanics", runtime_session_mechanics, "system_id")
    add_sources("runtime-mode-mechanics", runtime_mode_mechanics, "system_id")
    add_sources("runtime-campaign-mechanics", runtime_campaign_mechanics, "system_id")
    add_sources("runtime-draft-mechanics", runtime_draft_mechanics, "system_id")
    for row in unit_spell_mechanics:
        identity = f"{row['unit_id']}:{row['ability_id']}"
        for field in ("delayed_callback_functions", "dynamic_callback_functions"):
            for function_name in row.get(field, []):
                normalized_sources[str(function_name)].add(f"unit-spell-mechanics:{identity}")

    callback_framework = {
        "CallbackPeriodic_CallbackPeriodic_start",
        "CallbackPeriodic_destroyCallbackPeriodic",
        "CallbackPeriodic_staticCallback",
    }
    gameplay_framework = {
        "CallbackPeriodic_doPeriodically_doAfter_CfCastlePathing_call_doPeriodically_doAfter_CfCastlePathing",
    }
    e2e_only = {
        "CallbackPeriodic_doPeriodically_E2E_E2E_call_doPeriodically_E2E_E2E",
        "CallbackPeriodic_doPeriodically_doAfter_doAfter_AiFullGameE2E_call_doPeriodically_doAfter_doAfter_AiFullGameE2E",
    }
    telemetry_only = {
        "CallbackPeriodic_doPeriodically_MMD_call_doPeriodically_MMD",
        "CallbackPeriodic_doPeriodically_doAfter_MMDData_call_doPeriodically_doAfter_MMDData",
    }
    presentation_only = {
        "CallbackPeriodic_doPeriodically_MultiboardAttach_call_doPeriodically_MultiboardAttach",
        "CallbackPeriodic_doPeriodically_PSA_call_doPeriodically_PSA",
        "CallbackPeriodic_doPeriodically_addCommand_Commands_call_doPeriodically_addCommand_Commands",
        "CallbackPeriodic_doPeriodically_addCommand_Commands_call_doPeriodically_addCommand_Commands1",
        "CallbackPeriodic_doPeriodically_watch_doAfter_IncomeUI_call_doPeriodically_watch_doAfter_IncomeUI",
    }

    rows: list[dict[str, object]] = []
    for callback_name in callback_names:
        normalized = sorted(normalized_sources.get(callback_name, ()))
        if normalized:
            domains = {source.split(":", 1)[0] for source in normalized}
            if domains == {"runtime-ai-mechanics"}:
                status = "normalized-ai-runtime-semantics"
            elif domains == {"runtime-session-mechanics"}:
                status = "normalized-session-runtime-semantics"
            elif domains == {"runtime-mode-mechanics"}:
                status = "normalized-mode-runtime-semantics"
            elif domains == {"runtime-campaign-mechanics"}:
                status = "normalized-campaign-runtime-semantics"
            elif domains == {"runtime-draft-mechanics"}:
                status = "normalized-draft-runtime-semantics"
            elif domains == {"unit-spell-mechanics"}:
                status = "normalized-unit-spell-semantics"
            else:
                status = "normalized-gameplay-semantics"
            note = "callback is direct evidence for importer-facing normalized semantics"
        elif callback_name in callback_framework:
            status = "callback-framework-infrastructure"
            note = "generic CallbackPeriodic timer lifecycle/virtual dispatch infrastructure"
        elif callback_name in gameplay_framework:
            status = "gameplay-framework-infrastructure"
            note = "castle pathing job-queue ticker; authoritative infrastructure but not an independent unit/ability mechanic"
        elif callback_name in e2e_only:
            status = "e2e-only"
            note = "end-to-end harness periodic callback"
        elif callback_name in telemetry_only:
            status = "telemetry-only"
            note = "MMD/timeline telemetry periodic callback"
        elif callback_name in presentation_only:
            status = "presentation-only"
            note = "UI/message/PSA refresh callback without authoritative gameplay mutation"
        else:
            raise ValueError(f"unclassified CallbackPeriodic callback: {callback_name}")
        rows.append({
            "callback_function": callback_name,
            "coverage_status": status,
            "normalized_sources": normalized,
            "evidence_note": note,
            "byte_offset": function_offsets[callback_name],
        })

    status_counts = Counter(str(row["coverage_status"]) for row in rows)
    expected_status_counts = Counter({
        "normalized-gameplay-semantics": 2,
        "normalized-unit-spell-semantics": 1,
        "normalized-ai-runtime-semantics": 2,
        "normalized-session-runtime-semantics": 2,
        "normalized-mode-runtime-semantics": 4,
        "normalized-campaign-runtime-semantics": 2,
        "normalized-draft-runtime-semantics": 3,
        "callback-framework-infrastructure": 3,
        "gameplay-framework-infrastructure": 1,
        "presentation-only": 5,
        "telemetry-only": 2,
        "e2e-only": 2,
    })
    if status_counts != expected_status_counts:
        raise ValueError(f"CallbackPeriodic coverage classification changed: {dict(sorted(status_counts.items()))}")
    return rows


def _extract_event_listener_coverage(
    functions: list[dict[str, object]],
    call_edges: Counter[tuple[str, str]],
    production_unit_special_mechanics: list[dict[str, object]],
    runtime_system_mechanics: list[dict[str, object]],
    building_spell_mechanics: list[dict[str, object]],
    perk_mechanics: list[dict[str, object]],
    runtime_ai_mechanics: list[dict[str, object]],
    runtime_session_mechanics: list[dict[str, object]],
    runtime_mode_mechanics: list[dict[str, object]],
    runtime_campaign_mechanics: list[dict[str, object]],
    protected_perk_registry_audit: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Strict closure audit over generated EventListener callbacks.

    A listener is importer-covered either when it is direct semantic evidence or
    when a short, exact named-call path reaches already-normalized semantics.
    Remaining callbacks are classified by subsystem so presentation, E2E,
    campaign/session, command/mode, telemetry and generic dispatch code cannot
    be mistaken for silently omitted unit/spell gameplay.
    """
    # This is deliberately a full-map closure audit. Tiny parser fixtures used
    # throughout the unit tests do not contain the protected perk registry and
    # must remain valid inputs to analyze_lua without synthesizing 70 listeners.
    if not protected_perk_registry_audit:
        return []

    function_offsets = {str(row["name"]): int(row["start"]) for row in functions}
    listener_names = sorted(
        name for name in function_offsets
        if name.startswith("EventListener_") and "_onEvent_" in name
    )
    if len(listener_names) != 70:
        raise ValueError(f"event-listener callback inventory changed: {len(listener_names)}")

    normalized_sources: dict[str, set[str]] = defaultdict(set)

    def add_sources(domain: str, rows: list[dict[str, object]], identity_key: str) -> None:
        for row in rows:
            identity = str(row[identity_key])
            for function_name in row.get("source_functions", []):
                normalized_sources[str(function_name)].add(f"{domain}:{identity}")

    add_sources("production-unit-special-mechanics", production_unit_special_mechanics, "mechanic_kind")
    add_sources("runtime-system-mechanics", runtime_system_mechanics, "system_id")
    add_sources("building-spell-mechanics", building_spell_mechanics, "mechanic_kind")
    add_sources("perk-mechanics", perk_mechanics, "perk_id")
    add_sources("runtime-ai-mechanics", runtime_ai_mechanics, "system_id")
    add_sources("runtime-session-mechanics", runtime_session_mechanics, "system_id")
    add_sources("runtime-mode-mechanics", runtime_mode_mechanics, "system_id")
    add_sources("runtime-campaign-mechanics", runtime_campaign_mechanics, "system_id")

    callees_by_caller: dict[str, set[str]] = defaultdict(set)
    for (caller, callee), count in call_edges.items():
        if count > 0:
            callees_by_caller[str(caller)].add(str(callee))

    def path_to_normalized(listener_name: str) -> tuple[list[str], list[str]]:
        frontier: list[tuple[str, list[str]]] = [(listener_name, [listener_name])]
        seen = {listener_name}
        for _depth in range(3):
            next_frontier: list[tuple[str, list[str]]] = []
            for current, path in sorted(frontier):
                for callee in sorted(callees_by_caller.get(current, ())):
                    candidate_path = [*path, callee]
                    if callee in normalized_sources:
                        return candidate_path, sorted(normalized_sources[callee])
                    if callee not in seen:
                        seen.add(callee)
                        next_frontier.append((callee, candidate_path))
            frontier = next_frontier
        return [], []

    building_e2e_base = "EventListener_add_BuildingCatalogE2E_onEvent_add_BuildingCatalogE2E"
    e2e_only = {
        building_e2e_base,
        *(f"{building_e2e_base}{index}" for index in range(1, 12)),
        "EventListener_add_CampaignSmokeTest_onEvent_add_CampaignSmokeTest",
    }
    presentation_only = {
        "EventListener_add_AttachedEffects_onEvent_add_AttachedEffects",
        "EventListener_add_BuildingAttachments_onEvent_add_BuildingAttachments",
        "EventListener_add_BuildingAttachments_onEvent_add_BuildingAttachments1",
        "EventListener_add_BuildingAttachments_onEvent_add_BuildingAttachments2",
        "EventListener_add_BuildingAttachments_onEvent_add_BuildingAttachments3",
        "EventListener_add_DraftUIScreen_DraftUI_onEvent_add_DraftUIScreen_DraftUI",
        "EventListener_add_MultiboardEventHooks_onEvent_add_MultiboardEventHooks",
        "EventListener_add_ShopAnnouncements_onEvent_add_ShopAnnouncements1",
        "EventListener_add_ShopAnnouncements_onEvent_add_ShopAnnouncements2",
        "EventListener_add_TrainProgressRuntime_onEvent_add_TrainProgressRuntime",
        "EventListener_add_UnitTrainingRuntime_onEvent_add_UnitTrainingRuntime1",
        "EventListener_add_Campaign_onEvent_add_Campaign",
        "EventListener_add_Commands_onEvent_add_Commands",
    }
    gameplay_framework = {
        "EventListener_add_BuildingSpells_onEvent_add_BuildingSpells",
        "EventListener_add_DamageEvent_onEvent_add_DamageEvent",
        "EventListener_add_DamageEvent_onEvent_add_DamageEvent1",
    }
    player_session_runtime = {
        "EventListener_add_IdleDetectionRuntime_onEvent_add_IdleDetectionRuntime",
        "EventListener_add_IdleDetectionRuntime_onEvent_add_IdleDetectionRuntime1",
        "EventListener_add_doAfter_MMDData_onEvent_add_doAfter_MMDData",
        "EventListener_add_PlayerLeave_onEvent_add_PlayerLeave",
    }
    command_framework = {
        "EventListener_add_WurstCommand_onEvent_add_WurstCommand",
    }
    mode_runtime = {
        "EventListener_add_ModeParser_onEvent_add_ModeParser",
    }
    telemetry_only = {
        "EventListener_add_RoundStatsTracking_onEvent_add_RoundStatsTracking",
    }

    rows: list[dict[str, object]] = []
    for listener_name in listener_names:
        direct = sorted(normalized_sources.get(listener_name, ()))
        dispatch_path: list[str] = []
        normalized = direct
        if direct:
            if all(source.startswith("runtime-ai-mechanics:") for source in direct):
                status = "normalized-ai-runtime-semantics"
                note = "listener is direct evidence for normalized AI-runtime semantics"
            elif all(source.startswith("runtime-session-mechanics:") for source in direct):
                status = "normalized-session-runtime-semantics"
                note = "listener is direct evidence for normalized player-session runtime semantics"
            elif all(source.startswith("runtime-mode-mechanics:") for source in direct):
                status = "normalized-mode-runtime-semantics"
                note = "listener is direct evidence for normalized mode-selection runtime semantics"
            elif all(source.startswith("runtime-campaign-mechanics:") for source in direct):
                status = "normalized-campaign-runtime-semantics"
                note = "listener is direct evidence for normalized campaign challenge runtime semantics"
            else:
                status = "normalized-gameplay-semantics"
                note = "listener is direct evidence for importer-facing normalized gameplay semantics"
        else:
            dispatch_path, normalized = path_to_normalized(listener_name)
            if dispatch_path:
                if all(source.startswith("runtime-ai-mechanics:") for source in normalized):
                    status = "normalized-ai-runtime-dispatch"
                    note = "listener reaches normalized AI-runtime semantics through an exact named-call path of at most three edges"
                elif all(source.startswith("runtime-session-mechanics:") for source in normalized):
                    status = "normalized-session-runtime-dispatch"
                    note = "listener reaches normalized player-session runtime semantics through an exact named-call path of at most three edges"
                elif all(source.startswith("runtime-mode-mechanics:") for source in normalized):
                    status = "normalized-mode-runtime-dispatch"
                    note = "listener reaches normalized mode-selection runtime semantics through an exact named-call path of at most three edges"
                elif all(source.startswith("runtime-campaign-mechanics:") for source in normalized):
                    status = "normalized-campaign-runtime-dispatch"
                    note = "listener reaches normalized campaign runtime semantics through an exact named-call path of at most three edges"
                else:
                    status = "normalized-gameplay-dispatch"
                    note = "listener reaches normalized gameplay semantics through an exact named-call path of at most three edges"
            elif listener_name in e2e_only:
                status = "e2e-only"
                note = "generated end-to-end/smoke-test listener; not production gameplay semantics"
            elif listener_name in presentation_only:
                status = "presentation-only"
                note = "visual/UI presentation lifecycle only; does not define authoritative gameplay state"
            elif listener_name in gameplay_framework:
                status = "gameplay-framework-infrastructure"
                note = "generic event-dispatch infrastructure; concrete gameplay semantics are normalized at registered handlers/listeners"
            elif listener_name in player_session_runtime:
                status = "player-session-runtime-unmodeled"
                note = "live AFK/leave/autobalance/session control path; explicitly tracked outside unit/spell mechanics"
            elif listener_name in command_framework:
                status = "command-framework-infrastructure"
                note = "generic chat-command tokenization/dispatch infrastructure; concrete command handlers own any gameplay semantics"
            elif listener_name in mode_runtime:
                status = "mode-selection-runtime-unmodeled"
                note = "live map-mode parser/control path; mode semantics are not yet normalized"
            elif listener_name in telemetry_only:
                status = "telemetry-only"
                note = "round-stat observer used for statistics/telemetry rather than authoritative mechanic mutation"
            else:
                raise ValueError(f"unclassified event-listener callback: {listener_name}")
        rows.append({
            "listener_function": listener_name,
            "coverage_status": status,
            "normalized_sources": normalized,
            "dispatch_path": dispatch_path,
            "evidence_note": note,
            "byte_offset": function_offsets[listener_name],
        })

    status_counts = Counter(str(row["coverage_status"]) for row in rows)
    expected_status_counts = Counter({
        "normalized-gameplay-semantics": 21,
        "normalized-ai-runtime-semantics": 1,
        "normalized-session-runtime-semantics": 4,
        "normalized-mode-runtime-semantics": 1,
        "normalized-gameplay-dispatch": 10,
        "presentation-only": 13,
        "e2e-only": 13,
        "gameplay-framework-infrastructure": 3,
        "normalized-campaign-runtime-semantics": 2,
        "command-framework-infrastructure": 1,
        "telemetry-only": 1,
    })
    if status_counts != expected_status_counts:
        raise ValueError(
            f"event-listener coverage classification changed: {dict(sorted(status_counts.items()))}"
        )
    return rows


def _extract_castle_item_mechanics(
    data: bytes,
    functions: list[dict[str, object]],
    function_aliases: list[dict[str, object]],
) -> dict[str, object]:
    """Recover Castle shop inventory plus item-specific scripted runtime behavior."""
    def body(name: str) -> tuple[int, list[LuaToken]] | None:
        return _function_body_tokens(data, functions, name)

    shop_slots: list[dict[str, object]] = []
    slot_body = body("castleItemIdForSlot")
    if slot_body is not None:
        _start, tokens = slot_body
        for index in range(len(tokens) - 6):
            if tokens[index].text != "==" or tokens[index + 1].kind != "number":
                continue
            if tokens[index + 1].integer_value is None:
                continue
            slot = int(tokens[index + 1].integer_value)
            for cursor in range(index + 2, min(index + 12, len(tokens) - 1)):
                if tokens[cursor].text == "return" and tokens[cursor + 1].kind == "number":
                    item_id = _integer_literal_value([tokens[cursor + 1]])
                    shop_slots.append({
                        "slot": slot,
                        "item_id": item_id,
                        "function": "castleItemIdForSlot",
                        "byte_offset": _start + tokens[index].start,
                    })
                    break
        shop_slots.sort(key=lambda row: int(row["slot"]))

    # The item subsystem stores its constants in the trigger initializer TH,
    # then refers to those globals from UH/ZH. Resolve that small generated
    # constant environment first rather than requiring literals at each use.
    constants: dict[str, int | Decimal] = {}
    setup_body = body("TH")
    if setup_body is not None:
        _start, tokens = setup_body
        for index in range(len(tokens) - 2):
            if tokens[index].kind != "ident" or tokens[index + 1].text != "=":
                continue
            rhs = tokens[index + 2]
            if rhs.kind != "number":
                continue
            if rhs.integer_value is not None:
                constants[tokens[index].text] = int(rhs.integer_value)
                continue
            try:
                constants[tokens[index].text] = Decimal(rhs.text)
            except InvalidOperation:
                continue

    def resolved_integer(token: LuaToken) -> int | None:
        if token.kind == "number" and token.integer_value is not None:
            return int(token.integer_value)
        if token.kind == "ident":
            value = constants.get(token.text)
            if isinstance(value, int):
                return value
            if isinstance(value, Decimal) and value == value.to_integral_value():
                return int(value)
        return None

    item_values: dict[int, int] = {}
    value_body = body("UH")
    if value_body is not None:
        _start, tokens = value_body
        for index in range(len(tokens) - 4):
            if tokens[index].text != "==":
                continue
            item_id = resolved_integer(tokens[index + 1])
            if item_id is None:
                continue
            for cursor in range(index + 2, min(index + 12, len(tokens) - 1)):
                if tokens[cursor].text != "return":
                    continue
                value = resolved_integer(tokens[cursor + 1])
                if value is not None:
                    item_values[item_id] = value
                break

    pickup: dict[str, object] = {
        "gold_item_rawcode_integer": None,
        "cheese_item_rawcode_integer": None,
        "blast_staff_rawcode_integer": None,
        "multi_blast_staff_rawcode_integer": None,
        "gold_amount_formula": "floor(((qGb[player_id] + 4) * 0.25) * (8 + pIb))",
        "gold_item_consumed": True,
        "blast_staff_recipe_required_count": 4,
        "blast_staff_recipe_removes_rawcode": None,
        "blast_staff_recipe_adds_rawcode": None,
        "cheese_no_cheese_mode_flag": "HGb",
        "cheese_legendary_mode_enabled_flag": "GGb",
        "cheese_food_cap_delta": 1,
        "cheese_refund_amount": None,
        "damage_aura_carrier_rawcode_integer": 1697656888,
        "double_damage_aura_ability_rawcode_integer": 1093678660,
        "quad_damage_aura_ability_rawcode_integer": 1093678659,
        "damage_aura_lifetime_seconds": 29,
    }
    pickup_body = body("ZH")
    if pickup_body is not None:
        pickup["gold_item_rawcode_integer"] = constants.get("tab")
        pickup["cheese_item_rawcode_integer"] = constants.get("sab")
        pickup["blast_staff_rawcode_integer"] = constants.get("rab")
        pickup["multi_blast_staff_rawcode_integer"] = constants.get("qab")
        pickup["blast_staff_recipe_removes_rawcode"] = constants.get("rab")
        pickup["blast_staff_recipe_adds_rawcode"] = constants.get("qab")
        pickup["cheese_refund_amount"] = constants.get("pab")
        pickup["damage_aura_lifetime_seconds"] = constants.get("oab")
        pickup["blast_staff_recipe_required_count"] = constants.get("nab")
        pickup["item_inventory_slot_max_index"] = constants.get("mab")

    # Resolve the generated OnCast listener classes back to their concrete
    # handlers, then recover the Castle item's three script-routed spell paths.
    listener_handler_by_class: dict[str, str] = {}
    for alias in function_aliases:
        alias_name = str(alias["alias"])
        suffix = ".OnCastListener_fireEx"
        if alias_name.endswith(suffix):
            listener_handler_by_class[alias_name[: -len(suffix)]] = str(alias["target_function"])

    # Global scalar constants such as Z6/V6 are initialized in small generated
    # setup functions. Keep only variables with one unambiguous positive value.
    constant_candidates: dict[str, set[int | Decimal]] = defaultdict(set)
    for function in functions:
        function_name = str(function["name"])
        function_body = body(function_name)
        if function_body is None:
            continue
        _function_start, tokens = function_body
        for index in range(len(tokens) - 2):
            if tokens[index].kind != "ident" or tokens[index + 1].text != "=":
                continue
            rhs = tokens[index + 2]
            if rhs.kind != "number":
                continue
            if rhs.integer_value is not None:
                value: int | Decimal = int(rhs.integer_value)
            else:
                try:
                    value = Decimal(rhs.text)
                except InvalidOperation:
                    continue
            if value != 0:
                constant_candidates[tokens[index].text].add(value)
    global_constants = {
        name: next(iter(values))
        for name, values in constant_candidates.items()
        if len(values) == 1
    }

    registrations: list[dict[str, object]] = []
    for function in functions:
        function_name = str(function["name"])
        function_body = body(function_name)
        if function_body is None:
            continue
        function_start, tokens = function_body
        integer_variables: dict[str, int] = {}
        variable_classes: dict[str, str] = {}
        for index, token in enumerate(tokens):
            if token.kind == "ident" and index + 2 < len(tokens) and tokens[index + 1].text == "=":
                rhs = tokens[index + 2]
                if rhs.kind == "number" and rhs.integer_value is not None:
                    integer_variables[token.text] = int(rhs.integer_value)
                elif rhs.kind == "ident" and rhs.text in integer_variables:
                    integer_variables[token.text] = integer_variables[rhs.text]
                if (
                    rhs.kind == "ident"
                    and rhs.text in listener_handler_by_class
                    and index + 5 < len(tokens)
                    and tokens[index + 3].text == ":"
                    and tokens[index + 4].kind == "ident"
                    and tokens[index + 4].text.startswith("create")
                    and tokens[index + 5].text == "("
                ):
                    variable_classes[token.text] = rhs.text
            if token.kind != "ident" or token.text != "EventListener_addSpellInternal":
                continue
            try:
                args, _next = _call_arguments(tokens, index)
            except ValueError:
                continue
            if len(args) != 3 or len(args[2]) != 1 or args[2][0].kind != "ident":
                continue
            closure_variable = args[2][0].text
            closure_class = variable_classes.get(closure_variable)
            if closure_class is None:
                continue
            ability_id: int | None = None
            if len(args[1]) == 1:
                ability_token = args[1][0]
                if ability_token.kind == "number" and ability_token.integer_value is not None:
                    ability_id = int(ability_token.integer_value)
                elif ability_token.kind == "ident":
                    ability_id = integer_variables.get(ability_token.text)
            if ability_id is None:
                continue
            registrations.append({
                "trigger_ability_id": ability_id,
                "handler_function": listener_handler_by_class[closure_class],
                "registration_function": function_name,
                "byte_offset": function_start + token.start,
            })

    item_spell_mechanics: list[dict[str, object]] = []
    for registration in registrations:
        handler_name = str(registration["handler_function"])
        handler_body = body(handler_name)
        if handler_body is None:
            continue
        _handler_start, tokens = handler_body

        def calls(callee: str) -> list[list[list[LuaToken]]]:
            result: list[list[list[LuaToken]]] = []
            for index, token in enumerate(tokens):
                if token.kind == "ident" and token.text == callee:
                    try:
                        args, _next = _call_arguments(tokens, index)
                    except ValueError:
                        continue
                    result.append(args)
            return result

        # Scroll of Stone/Speed: point cast -> e008 dummy -> hidden effect
        # ability -> immediate order -> one-second timed life.
        create_calls = calls("createUnit")
        ability_calls = calls("addProtectedAbility")
        order_calls = calls("unit_issueImmediateOrderById")
        life_calls = calls("__wurst_safe_UnitApplyTimedLife")
        if create_calls and ability_calls and order_calls and life_calls:
            carrier_id = resolved_integer(create_calls[0][1][0]) if len(create_calls[0]) > 1 and len(create_calls[0][1]) == 1 else None
            effect_id = resolved_integer(ability_calls[0][1][0]) if len(ability_calls[0]) > 1 and len(ability_calls[0][1]) == 1 else None
            order_id = resolved_integer(order_calls[0][1][0]) if len(order_calls[0]) > 1 and len(order_calls[0][1]) == 1 else None
            lifetime: str | None = None
            if len(life_calls[0]) > 2:
                try:
                    lifetime = _numeric_literal_text(life_calls[0][2])
                except ValueError:
                    lifetime = None
            if carrier_id is not None and effect_id is not None and order_id is not None and lifetime is not None:
                item_spell_mechanics.append({
                    "trigger_ability_id": int(registration["trigger_ability_id"]),
                    "mechanic_kind": "point-triggered-dummy-effect",
                    "effect_ability_ids": [effect_id],
                    "parameters": {
                        "carrier_unit_rawcode_integer": carrier_id,
                        "order_id": order_id,
                        "dummy_lifetime_seconds": lifetime,
                    },
                    "source_functions": [str(registration["registration_function"]), handler_name],
                    "byte_offset": int(registration["byte_offset"]),
                })
                continue

        # Orb of Lightning: target cast through DummyCaster, with the effect
        # level scaling from round minutes. The configured delay is post-cast
        # dummy recycle time, not a cast delay.
        cast_calls = calls("DummyCaster_DummyCaster_castTarget")
        if cast_calls:
            args = cast_calls[0]
            if len(args) >= 5:
                def global_integer(argument: list[LuaToken]) -> int | None:
                    if len(argument) != 1:
                        return None
                    token = argument[0]
                    if token.kind == "number" and token.integer_value is not None:
                        return int(token.integer_value)
                    if token.kind == "ident":
                        value = global_constants.get(token.text)
                        if isinstance(value, int):
                            return value
                    return None

                effect_id = global_integer(args[1])
                order_id = global_integer(args[3])
                recycle_delay: str | None = None
                delay_calls = calls("DummyCaster_DummyCaster_delay")
                if delay_calls and len(delay_calls[0]) > 1 and len(delay_calls[0][1]) == 1:
                    token = delay_calls[0][1][0]
                    if token.kind == "ident" and token.text in global_constants:
                        recycle_delay = str(global_constants[token.text])
                    elif token.kind == "number":
                        recycle_delay = token.text
                level_step_minutes: int | None = None
                for index, token in enumerate(tokens):
                    if token.kind == "ident" and token.text == "__wurst_intDiv":
                        try:
                            div_args, _next = _call_arguments(tokens, index)
                        except ValueError:
                            continue
                        if len(div_args) == 2 and len(div_args[1]) == 1:
                            level_step_minutes = resolved_integer(div_args[1][0])
                            if level_step_minutes is not None:
                                break
                if effect_id is not None and order_id is not None and level_step_minutes is not None:
                    item_spell_mechanics.append({
                        "trigger_ability_id": int(registration["trigger_ability_id"]),
                        "mechanic_kind": "target-triggered-scaling-dummy-effect",
                        "effect_ability_ids": [effect_id],
                        "parameters": {
                            "order_id": order_id,
                            "effect_level_formula": f"1 + floor(round_minutes / {level_step_minutes})",
                            "effect_level_step_minutes": level_step_minutes,
                            "dummy_recycle_delay_seconds": recycle_delay,
                        },
                        "source_functions": [str(registration["registration_function"]), handler_name],
                        "byte_offset": int(registration["byte_offset"]),
                    })

    return {
        "shop_slots": shop_slots,
        "item_values": item_values,
        "pickup": pickup,
        "item_spell_mechanics": item_spell_mechanics,
    }


def _extract_protected_filter_bindings(
    data: bytes,
    functions: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Resolve generated Filter wrappers only when their binding is structurally proven.

    W3P replaces many Wurst filter-function values with opaque ``_I[key]``
    lookups while leaving the concrete generated predicate functions readable.
    Source adjacency alone is *not* enough to pair them: initializers can emit
    helper functions between wrappers and predicates, and the predicate function
    order can differ from the wrapper priority order. The previous resolver
    skipped non-matching functions independently for each wrapper, which could
    silently shift a whole group of bindings.

    This extractor therefore uses two conservative sources of evidence:
    1. exact Castle Fight bindings whose use sites prove wrapper priority/role;
    2. an atomic adjacency fallback only when the number of wrappers exactly
       matches the complete nearby enemy/ally predicate sequence.
    Everything else remains explicit unresolved/dynamic provenance.
    """
    ordered_functions = sorted(functions, key=lambda function: int(function["start"]))
    bindings: list[dict[str, object]] = []

    def function_tokens(function_name: str) -> tuple[int, list[LuaToken]] | None:
        return _function_body_tokens(data, functions, function_name)

    def token_texts(function_name: str) -> set[str]:
        body = function_tokens(function_name)
        if body is None:
            raise ValueError(f"protected filter binding target is missing: {function_name}")
        return {token.text for token in body[1]}

    def classic_predicate_kind(tokens: list[LuaToken]) -> str | None:
        texts = {token.text for token in tokens}
        if "isAliveCombatSapper" not in texts or "mIb" not in texts:
            return None
        if "unit_isEnemyOf" in texts:
            return "alive-combat-sapper;enemy-of-mIb"
        if "unit_isAllyOf" in texts:
            return "alive-combat-sapper;ally-of-mIb"
        return None

    # These mappings are not guesses from source order. Their surrounding use
    # sites make the role unique. Examples: Assassin chooses its common mana
    # group first, then normal (<150 HP) versus Royal (<300 HP) ambush groups;
    # Desert Replenishment must progress from >200 missing HP to any wounded
    # ally to the remaining ally-state filter; Peyote alone consumes V0 and
    # therefore proves the enemy filter; QX is enumerated after fJ sets UX/TX
    # specifically for wL's side-effect healing callback.
    exact_specs: dict[tuple[str, str], tuple[str, str, set[str]]] = {
        ("jG", "Gib"): (
            "kG",
            "wurst-closure-for-groups-dispatch;filterCallback(GetFilterUnit);side-effect-only",
            {"_I", "_b"},
        ),
        ("lE", "NAb"): (
            "mE",
            "life>0.405;valid-assassin-target;max-mana>10",
            {"widget_getLife", "isValidAssassinTarget", "UNIT_STATE_MAX_MANA", "10."},
        ),
        ("lE", "MAb"): (
            "nE",
            "life>0.405;valid-assassin-target;life<150",
            {"widget_getLife", "isValidAssassinTarget", "150."},
        ),
        ("lE", "LAb"): (
            "oE",
            "life>0.405;valid-assassin-target;life<300",
            {"widget_getLife", "isValidAssassinTarget", "300."},
        ),
        ("lN", "xdo"): (
            "KN",
            "destructable-type-in-generated-six-type-set",
            {"GetFilterDestructable", "__wurst_safe_GetDestructableTypeId"},
        ),
        ("main", "vFb"): (
            "IC",
            "always-true-marketplace-stock-filter",
            {"true"},
        ),
        ("main", "cHb"): (
            "UC",
            "life>0.405;enemy-of-mIb;sapper;vulnerable;not-tentacle;unit-type-not-h06C",
            {"__wurst_safe_GetWidgetLife", "__wurst_safe_IsUnitEnemy", "UNIT_TYPE_SAPPER", "isVulnerable", "isNotTentacle", "1747990082"},
        ),
        ("main", "dHb"): (
            "TC",
            "life>0.405;sapper;vulnerable",
            {"__wurst_safe_GetWidgetLife", "UNIT_TYPE_SAPPER", "isVulnerable"},
        ),
        ("main", "ZGb"): (
            "VC",
            "life>0.405;peon",
            {"__wurst_safe_GetWidgetLife", "UNIT_TYPE_PEON"},
        ),
        ("nH", "ycb"): (
            "oH",
            "alive;structure;gobbo-repairable-target",
            {"widget_getLife", "UNIT_TYPE_STRUCTURE", "isGobboRepairableTarget"},
        ),
        ("nH", "xcb"): (
            "pH",
            "alive;mechanical;gobbo-repairable-target",
            {"widget_getLife", "UNIT_TYPE_MECHANICAL", "isGobboRepairableTarget"},
        ),
        ("oL", "e0"): (
            "pL",
            "normal-player-owner;missing-A02E;unit-type-not-x002;unit-type-not-Tdb",
            {"bj_MAX_PLAYERS", "unit_getAbilityLevel", "1093677637", "2016423986", "Tdb"},
        ),
        ("pJ", "G6"): (
            "qJ",
            "alive-combat-sapper;enemy-of-mIb;vulnerable;not-tentacle;mana>100",
            {"isAliveCombatSapper", "unit_isEnemyOf", "isVulnerable", "isNotTentacle", "unit_getMana", "100."},
        ),
        ("pJ", "F6"): (
            "rJ",
            "alive-combat-sapper;enemy-of-mIb;vulnerable;not-tentacle;mana>0",
            {"isAliveCombatSapper", "unit_isEnemyOf", "isVulnerable", "isNotTentacle", "unit_getMana", "0.0"},
        ),
        ("rK", "Y0"): (
            "tK",
            "life>0.405;ally-of-mIb;combat-sapper;missing-hp>200",
            {"widget_getLife", "unit_isAllyOf", "isCombatSapper", "__wurst_safe_BlzGetUnitMaxHP", "200"},
        ),
        ("rK", "X0"): (
            "sK",
            "life>0.405;ally-of-mIb;combat-sapper;wounded",
            {"widget_getLife", "unit_isAllyOf", "isCombatSapper", "__wurst_safe_BlzGetUnitMaxHP"},
        ),
        ("rK", "W0"): (
            "uK",
            "alive-combat-sapper;ally-of-mIb;protected-state>0-and<99.9",
            {"isAliveCombatSapper", "unit_isAllyOf", "0.0", "99.9"},
        ),
        ("rK", "V0"): (
            "vK",
            "alive-combat-sapper;enemy-of-mIb;vulnerable;not-tentacle",
            {"isAliveCombatSapper", "unit_isEnemyOf", "isVulnerable", "isNotTentacle"},
        ),
        ("uL", "RX"): (
            "AC",
            "life>0.405;ally-of-mIb;structure;missing-hp>1;not-vIb-handle-child7",
            {"__wurst_safe_GetWidgetLife", "__wurst_safe_IsUnitAlly", "UNIT_TYPE_STRUCTURE", "UNIT_STATE_MAX_LIFE", "vIb", "7"},
        ),
        ("uL", "SX"): (
            "vL",
            "alive-combat-sapper;enemy-of-mIb",
            {"isAliveCombatSapper", "unit_isEnemyOf"},
        ),
        ("uL", "QX"): (
            "wL",
            "side-effect-heal-alive-allied-combat-sappers;matching-UX:+TX;others:+0.2*TX;always-false",
            {"isAliveCombatSapper", "unit_isAllyOf", "UX", "TX", "__wurst_safe_SetWidgetLife", "false"},
        ),
    }

    for initializer in ordered_functions:
        initializer_name = str(initializer["name"])
        body = function_tokens(initializer_name)
        if body is None:
            continue
        initializer_start, tokens = body
        filter_assignments: list[tuple[str, int]] = []
        for index in range(len(tokens) - 3):
            if (
                tokens[index].kind == "ident"
                and tokens[index + 1].text == "="
                and tokens[index + 2].kind == "ident"
                and tokens[index + 2].text in {"Filter", "__wurst_safe_Filter"}
                and tokens[index + 3].text == "("
            ):
                filter_assignments.append((tokens[index].text, initializer_start + tokens[index].start))
        if not filter_assignments:
            continue

        # Safe generic fallback for small unprotected/synthetic compiler shapes:
        # pair only if the *complete* nearby classic predicate sequence has the
        # same cardinality as the wrapper sequence. No per-wrapper skipping.
        following_classic: list[tuple[str, str]] = []
        for candidate in ordered_functions:
            if int(candidate["start"]) < int(initializer["end"]):
                continue
            if int(candidate["start"]) - int(initializer["end"]) > 10000:
                break
            candidate_body = function_tokens(str(candidate["name"]))
            if candidate_body is None:
                continue
            kind = classic_predicate_kind(candidate_body[1])
            if kind is not None:
                following_classic.append((str(candidate["name"]), kind))
        atomic_fallback = following_classic if len(following_classic) == len(filter_assignments) else []

        for assignment_index, (variable, byte_offset) in enumerate(filter_assignments):
            spec = exact_specs.get((initializer_name, variable))
            if spec is not None:
                resolved_function, predicate, required_tokens = spec
                missing = required_tokens - token_texts(resolved_function)
                if missing:
                    raise ValueError(
                        f"protected filter binding {initializer_name}.{variable}->{resolved_function} changed; "
                        f"missing tokens={sorted(missing)}"
                    )
                bindings.append({
                    "symbol": variable,
                    "initializer_function": initializer_name,
                    "resolved_function": resolved_function,
                    "predicate": predicate,
                    "resolution_status": "resolved",
                    "evidence_kind": "static-use-site-and-generated-function-structure",
                    "byte_offset": byte_offset,
                })
                continue

            # registerPlayerUnitEvent wraps a caller-supplied function value; SCr
            # is a transient local, not one globally fixed protected predicate.
            if initializer_name == "registerPlayerUnitEvent" and variable == "SCr":
                bindings.append({
                    "symbol": variable,
                    "initializer_function": initializer_name,
                    "resolved_function": "",
                    "predicate": "function-argument-NCr",
                    "resolution_status": "dynamic",
                    "evidence_kind": "runtime-function-argument-filter-wrapper",
                    "byte_offset": byte_offset,
                })
                continue

            if atomic_fallback:
                resolved_function, predicate = atomic_fallback[assignment_index]
                bindings.append({
                    "symbol": variable,
                    "initializer_function": initializer_name,
                    "resolved_function": resolved_function,
                    "predicate": predicate,
                    "resolution_status": "resolved",
                    "evidence_kind": "static-generated-filter-atomic-adjacency",
                    "byte_offset": byte_offset,
                })
                continue

            bindings.append({
                "symbol": variable,
                "initializer_function": initializer_name,
                "resolved_function": "",
                "predicate": "",
                "resolution_status": "unresolved",
                "evidence_kind": "protected-filter-symbol-unresolved",
                "byte_offset": byte_offset,
            })

    bindings.sort(key=lambda row: (str(row["initializer_function"]), int(row["byte_offset"]), str(row["symbol"])))
    return bindings


def _extract_production_unit_special_mechanics(
    data: bytes,
    functions: list[dict[str, object]],
    protected_filter_bindings: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Normalize runtime-only production-unit mechanics outside spell registries.

    These mechanics are implemented by shared damage/death/unit-enter handlers,
    so they are invisible to the ordinary unit-spell registration extractor.
    Keep this intentionally strict: every row is emitted only after checking
    the exact generated functions and constants that define the behavior.
    """

    def require_tokens(function_name: str, required: set[str]) -> tuple[int, set[str]]:
        body = _function_body_tokens(data, functions, function_name)
        if body is None:
            raise ValueError(f"special unit mechanic source function is missing: {function_name}")
        start, tokens = body
        texts = {token.text for token in tokens}
        missing = required - texts
        if missing:
            raise ValueError(
                f"special unit mechanic source {function_name} changed; missing tokens={sorted(missing)}"
            )
        return start, texts

    # analyze_lua is also exercised on intentionally tiny synthetic snippets.
    # This pass is map-specific and should simply be absent for those fixtures;
    # once the complete Castle Fight handler set is present, all assertions
    # below remain strict and extraction fails on any structural drift.
    available_functions = {str(function["name"]) for function in functions}
    required_map_functions = {
        "onUnitEnteredMap",
        "castNatureAttackTree",
        "CallbackSingle_doAfter_UnitEnterRuntime_call_doAfter_UnitEnterRuntime",
        "CallbackSingle_doAfter_doAfter_UnitEnterRuntime_call_doAfter_doAfter_UnitEnterRuntime",
        "DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire",
        "fJ",
        "onSummonedUnit",
        "onUnitTrained",
        "CallbackSingle_doAfter_ReengageRuntime_call_doAfter_ReengageRuntime1",
        "CallbackSingle_doAfter_doAfter_ReengageRuntime_call_doAfter_doAfter_ReengageRuntime1",
        "EventListener_add_doAfter_FixDefend_onEvent_add_doAfter_FixDefend",
        "CallbackSingle_doAfter_add_doAfter_FixDefend_call_doAfter_add_doAfter_FixDefend",
        "EventListener_add_FireElemental_onEvent_add_FireElemental",
        "setupFireSummon",
        "CallbackSingle_doAfter_FireElemental_call_doAfter_FireElemental",
        "handleSourceDamageEffects",
        "dryadDispelProc",
        "keeperDispelProc",
        "ancientKeeperDispelProc",
        "ForGroupCallback_forUnitsInRange_DamageProcs_callback_forUnitsInRange_DamageProcs",
        "handleTargetDamageEffects",
        "castLightningShield",
        "feralRage",
        "startBearHibernate",
        "CallbackSingle_doAfter_DamageRuntime_call_doAfter_DamageRuntime1",
        "startBearSleep",
        "CallbackSingle_doAfter_DamageRuntime_call_doAfter_DamageRuntime",
        "finishBearHibernate",
        "handleFanOfKnives",
        "vampireCharge",
        "randomN02YOrder",
        "tornadoStackProc",
        "code__addAction_DamageRuntime",
        "applyVampireArmorReduction",
        "devourAttackProc",
        "hL", "shouldApplyRiptideAirBonus", "addRiptideAirDamageBonus",
        "DamageListener_addListener_RiptideAttack_onEvent_addListener_RiptideAttack",
        "IN", "DamageListener_addListener_TrollBlood_onEvent_addListener_TrollBlood",
        "qP", "DamageListener_addListener_doAfter_Whirlwind_onEvent_addListener_doAfter_Whirlwind",
        "ForGroupCallback_forUnitsInRange_addListener_doAfter_Whirlwind_callback_forUnitsInRange_addListener_doAfter_Whirlwind",
    }
    if not required_map_functions.issubset(available_functions):
        return []

    rows: list[dict[str, object]] = []

    # Mountain Giant: every newly-entered e00F automatically gets a temporary
    # VTlt tree in front of it and is issued the native Grab Tree order. The
    # follow-up callback resumes attack after 1.6 s and schedules destruction
    # of that temporary destructable another 2 s later.
    enter_start, _ = require_tokens(
        "onUnitEnteredMap",
        {"1848652098", "1697656902", "__wurst_safe_SetUnitVertexColor", "castNatureAttackTree"},
    )
    enter_function = next(function for function in functions if function["name"] == "onUnitEnteredMap")
    enter_source = data[int(enter_function["start"]):int(enter_function["end"])]
    if b"if(qcs==1848652098)then __wurst_safe_SetUnitVertexColor(ocs,82,0,135,102)" not in enter_source:
        raise ValueError("Shadow Drake on-enter branch is no longer the verified visual-only vertex tint")
    giant_start, _ = require_tokens(
        "castNatureAttackTree",
        {
            "1448373364", "32.", "360.", "852511", "1.6",
            "CreateDestructable", "__wurst_safe_IssueTargetOrderById", "doAfter",
        },
    )
    giant_function = next(function for function in functions if function["name"] == "castNatureAttackTree")
    giant_source = data[int(giant_function["start"]):int(giant_function["end"])]
    if b"GetRandomReal(.5,.8)" not in giant_source:
        raise ValueError("Mountain Giant temporary-tree scale range changed")
    require_tokens(
        "CallbackSingle_doAfter_UnitEnterRuntime_call_doAfter_UnitEnterRuntime",
        {"orderCodeAttack", "2.", "doAfter"},
    )
    resume_function = next(
        function for function in functions
        if function["name"] == "CallbackSingle_doAfter_UnitEnterRuntime_call_doAfter_UnitEnterRuntime"
    )
    resume_source = data[int(resume_function["start"]):int(resume_function["end"])]
    if b"widget_getLife(L8n.u)>.405" not in resume_source:
        raise ValueError("Mountain Giant post-Grab-Tree alive threshold changed")
    require_tokens(
        "CallbackSingle_doAfter_doAfter_UnitEnterRuntime_call_doAfter_doAfter_UnitEnterRuntime",
        {"__wurst_safe_RemoveDestructable"},
    )
    rows.append({
        "unit_id": 1697656902,
        "mechanic_kind": "auto-spawn-tree-and-grab-war-club",
        "trigger": "on-unit-entered-map",
        "parameters": {
            "war_club_ability_id": 1093681731,
            "tree_destructable_id": 1448373364,
            "tree_forward_offset": 32,
            "tree_facing_random_degrees": [0, 360],
            "tree_scale_random": [0.5, 0.8],
            "grab_tree_order_id": 852511,
            "resume_attack_delay_seconds": 1.6,
            "remove_tree_delay_after_resume_seconds": 2.0,
            "tree_total_lifetime_seconds": 3.6,
            "bypasses_wrong_order_guard": True,
        },
        "related_rawcode_ids": [1093681731, 1448373364],
        "source_functions": [
            "onUnitEnteredMap",
            "castNatureAttackTree",
            "CallbackSingle_doAfter_UnitEnterRuntime_call_doAfter_UnitEnterRuntime",
            "CallbackSingle_doAfter_doAfter_UnitEnterRuntime_call_doAfter_doAfter_UnitEnterRuntime",
        ],
        "evidence_kind": "exact-runtime-handler-and-callback-chain",
        "byte_offset": min(enter_start, giant_start),
    })

    # Echofoot Mystic: the shared damage listener performs the blink/remnant
    # spawn, while fJ applies the remnant's scripted death explosion. Native
    # A0HD land-mine data supplies the enemy-proximity activation behavior.
    echo_start, _ = require_tokens(
        "DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire",
        {
            "1848652617", "1848652626", "1093683252", "62500.0", "1200.", "15.", "60.",
            "unit_isAlive", "isCombatSapper", "__wurst_safe_BlzGetUnitAbilityCooldownRemaining",
            "vec2_distanceToSq", "createUnit", "__wurst_safe_UnitApplyTimedLife",
            "__wurst_safe_SetUnitPosition", "orderCodeAttack", "__wurst_safe_BlzStartUnitAbilityCooldown",
        },
    )
    death_start, _ = require_tokens(
        "fJ",
        {
            "1848652626", "cHb", "300.", "150.", "ATTACK_TYPE_NORMAL", "DAMAGE_TYPE_MAGIC",
            "__wurst_safe_GroupEnumUnitsInRange", "__wurst_safe_UnitDamageTarget",
        },
    )
    filter_binding = next(
        (row for row in protected_filter_bindings if row["symbol"] == "cHb"),
        None,
    )
    if filter_binding is None or filter_binding["resolution_status"] != "resolved":
        raise ValueError("Echo Remnant mechanic requires resolved protected filter cHb")
    if filter_binding["resolved_function"] != "UC":
        raise ValueError(f"Echo Remnant cHb binding changed: {filter_binding}")

    rows.append({
        "unit_id": 1848652617,
        "mechanic_kind": "damage-triggered-echo-step-and-remnant",
        "trigger": "damage-event",
        "parameters": {
            "trigger_source_requires_combat_sapper": True,
            "trigger_target_requires_alive": True,
            "trigger_range": 250,
            "trigger_range_squared": 62500,
            "trigger_ability_id": 1093683252,
            "runtime_cooldown_seconds": 15,
            "blink_distance_toward_own_castle": 1200,
            "resume_attack_immediately": True,
            "remnant_unit_id": 1848652626,
            "remnant_timed_life_seconds": 60,
            "remnant_activation_ability_id": 1093683268,
            "remnant_explosion_radius": 300,
            "remnant_explosion_damage": 150,
            "remnant_explosion_attack_type": "normal",
            "remnant_explosion_damage_type": "magic",
            "remnant_target_filter_symbol": "cHb",
            "remnant_target_filter_function": str(filter_binding["resolved_function"]),
            "remnant_target_predicate": str(filter_binding["predicate"]),
        },
        "related_rawcode_ids": [1093683252, 1848652626, 1093683268],
        "source_functions": [
            "DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire",
            "fJ",
            "UC",
        ],
        "evidence_kind": "exact-damage-and-death-handler-with-resolved-filter",
        "byte_offset": min(echo_start, death_start),
    })

    retaliation_start, _ = require_tokens(
        "DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire",
        {
            "1848652371", "1848652372", "UNIT_TYPE_FLYING", "2.5", "unit_getIndex",
            "getElapsedGameTime", "unit_issueTargetOrder",
        },
    )
    retaliation_function = next(
        function for function in functions
        if function["name"] == "DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire"
    )
    retaliation_source = data[int(retaliation_function["start"]):int(retaliation_function["end"])]
    if b'unit_issueTargetOrder(x6n,"attack",w6n)' not in retaliation_source:
        raise ValueError("Gnoll anti-air retaliation order changed")
    for unit_id in (1848652371, 1848652372):
        rows.append({
            "unit_id": unit_id,
            "mechanic_kind": "retarget-flying-damage-source",
            "trigger": "damage-event",
            "parameters": {
                "trigger_source_requires_flying": True,
                "retaliation_target": "damage-source",
                "issued_order": "attack",
                "per_unit_retarget_throttle_seconds": 2.5,
                "requires_positive_unit_index": True,
            },
            "related_rawcode_ids": [],
            "source_functions": [
                "DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire",
            ],
            "evidence_kind": "exact-shared-damage-handler",
            "byte_offset": retaliation_start,
        })

    # Winged Riptide Serpent: the shared damage listener rewrites every positive
    # attack-damage event against a flying target to 145% of the current event
    # amount. This applies at the damage-event layer, including bounce recipients,
    # rather than being encoded in the native bouncing weapon itself.
    riptide_init_start, _ = require_tokens("hL", {"1848652629", "0.45", "DamageEvent_addListener"})
    riptide_pred_start, _ = require_tokens(
        "shouldApplyRiptideAirBonus",
        {"u0", "0"},
    )
    riptide_bonus_start, _ = require_tokens("addRiptideAirDamageBonus", {"q0"})
    riptide_start, _ = require_tokens(
        "DamageListener_addListener_RiptideAttack_onEvent_addListener_RiptideAttack",
        {
            "DamageEvent_getSource", "DamageEvent_getTarget", "DamageEvent_getAmount",
            "UNIT_TYPE_FLYING", "shouldApplyRiptideAirBonus", "addRiptideAirDamageBonus",
            "DamageInstance_DamageInstance_setAmount",
        },
    )
    riptide_pred_function = next(function for function in functions if function["name"] == "shouldApplyRiptideAirBonus")
    riptide_pred_source = data[int(riptide_pred_function["start"]):int(riptide_pred_function["end"])]
    if b"((UDr==u0)and VDr)and(WDr==0)" not in riptide_pred_source or b"XDr>0." not in riptide_pred_source:
        raise ValueError("Winged Riptide Serpent air-bonus predicate changed")
    riptide_bonus_function = next(function for function in functions if function["name"] == "addRiptideAirDamageBonus")
    riptide_bonus_source = data[int(riptide_bonus_function["start"]):int(riptide_bonus_function["end"])]
    if b"return(YDr+(YDr*q0))" not in riptide_bonus_source:
        raise ValueError("Winged Riptide Serpent air-bonus formula changed")
    rows.append({
        "unit_id": 1848652629,
        "mechanic_kind": "attack-damage-flying-target-multiplier",
        "trigger": "damage-event",
        "parameters": {
            "required_damage_event_type": 0,
            "requires_positive_damage": True,
            "target_requires_flying": True,
            "bonus_fraction": 0.45,
            "damage_multiplier": 1.45,
            "modifies_current_damage_instance": True,
            "applies_to_any_qualifying_damage_recipient": True,
        },
        "related_rawcode_ids": [],
        "source_functions": [
            "hL", "shouldApplyRiptideAirBonus", "addRiptideAirDamageBonus",
            "DamageListener_addListener_RiptideAttack_onEvent_addListener_RiptideAttack",
        ],
        "evidence_kind": "exact-damage-listener-current-instance-rewrite",
        "byte_offset": min(riptide_init_start, riptide_pred_start, riptide_bonus_start, riptide_start),
    })

    # Forest Troll Trapper: taking damage promotes a hidden permanent A0HR
    # attack-bonus ability as successive HP-ratio thresholds are crossed. The
    # handler has no downgrade/removal path, so healing does not undo a reached
    # tier during that unit's lifetime.
    troll_blood_init_start, _ = require_tokens("IN", {"1093683282", "DamageEvent_addListener"})
    troll_blood_start, _ = require_tokens(
        "DamageListener_addListener_TrollBlood_onEvent_addListener_TrollBlood",
        {
            "1848652359", "unit_getHPRatio", "0.25", "0.5", "0.75", "jS",
            "addProtectedAbility", "__wurst_safe_SetUnitAbilityLevel", "__wurst_safe_SetUnitVertexColor",
        },
    )
    troll_blood_function = next(
        function for function in functions
        if function["name"] == "DamageListener_addListener_TrollBlood_onEvent_addListener_TrollBlood"
    )
    troll_blood_source = data[int(troll_blood_function["start"]):int(troll_blood_function["end"])]
    for fragment in (
        b"if(p8n<0.25)then",
        b"__wurst_safe_SetUnitAbilityLevel(q8n,r8n,3)",
        b"__wurst_safe_SetUnitVertexColor(s8n,143,8,8,255)",
        b"elseif(p8n<0.5)then",
        b"__wurst_safe_SetUnitAbilityLevel(t8n,u8n,2)",
        b"__wurst_safe_SetUnitVertexColor(v8n,179,66,66,255)",
        b"elseif(p8n<0.75)then",
        b"addProtectedAbility(w8n,x8n)",
        b"__wurst_safe_SetUnitVertexColor(y8n,229,138,138,255)",
    ):
        if fragment not in troll_blood_source:
            raise ValueError("Forest Troll Trapper Troll Blood threshold state machine changed")
    rows.append({
        "unit_id": 1848652359,
        "mechanic_kind": "damage-triggered-persistent-low-hp-attack-bonus",
        "trigger": "target-damage-event",
        "parameters": {
            "bonus_ability_id": 1093683282,
            "thresholds": [
                {"hp_ratio_below": 0.75, "ability_level": 1, "vertex_rgba": [229, 138, 138, 255]},
                {"hp_ratio_below": 0.50, "ability_level": 2, "vertex_rgba": [179, 66, 66, 255]},
                {"hp_ratio_below": 0.25, "ability_level": 3, "vertex_rgba": [143, 8, 8, 255]},
            ],
            "levels_only_increase": True,
            "healing_does_not_downgrade_reached_level": True,
        },
        "related_rawcode_ids": [1093683282],
        "source_functions": ["IN", "DamageListener_addListener_TrollBlood_onEvent_addListener_TrollBlood"],
        "evidence_kind": "exact-damage-listener-hp-threshold-state-machine",
        "byte_offset": min(troll_blood_init_start, troll_blood_start),
    })

    # Ironpaw Guardian: attack-damage events have a 20% real-valued proc chance
    # to deal a second 175 universal-damage packet to all alive ground enemy
    # combat sappers in 160 range of the damaged target. The map tooltip says
    # 150 damage; retain the exact script value for importer behavior.
    whirlwind_init_start, _ = require_tokens("qP", {"1093683271", "0.20", "175.0", "160.0"})
    whirlwind_start, _ = require_tokens(
        "DamageListener_addListener_doAfter_Whirlwind_onEvent_addListener_doAfter_Whirlwind",
        {
            "DamageEvent_getType", "unit_hasAbility", "GetRandomReal", "0.", "1.",
            "forUnitsInRange", "__wurst_safe_SetUnitAnimation", "__wurst_safe_QueueUnitAnimation",
        },
    )
    whirlwind_cb_start, _ = require_tokens(
        "ForGroupCallback_forUnitsInRange_addListener_doAfter_Whirlwind_callback_forUnitsInRange_addListener_doAfter_Whirlwind",
        {
            "unit_isEnemyOf1", "unit_isAlive", "isCombatSapper", "UNIT_TYPE_FLYING",
            "ATTACK_TYPE_NORMAL", "DAMAGE_TYPE_UNIVERSAL", "__wurst_safe_UnitDamageTarget",
        },
    )
    whirlwind_function = next(
        function for function in functions
        if function["name"] == "DamageListener_addListener_doAfter_Whirlwind_onEvent_addListener_doAfter_Whirlwind"
    )
    whirlwind_source = data[int(whirlwind_function["start"]):int(whirlwind_function["end"])]
    if b"if((DamageEvent_getType()==0)and unit_hasAbility(zao,KQ))then" not in whirlwind_source:
        raise ValueError("Ironpaw Whirlwind damage-event gate changed")
    if b"if(JQ>=GetRandomReal(0.,1.))then" not in whirlwind_source:
        raise ValueError("Ironpaw Whirlwind proc comparison changed")
    if b'__wurst_safe_SetUnitAnimation(Gao,"Attack Walk Stand Spin")' not in whirlwind_source or b'__wurst_safe_QueueUnitAnimation(Hao,"stand")' not in whirlwind_source:
        raise ValueError("Ironpaw Whirlwind animation sequence changed")
    whirlwind_cb_function = next(
        function for function in functions
        if function["name"] == "ForGroupCallback_forUnitsInRange_addListener_doAfter_Whirlwind_callback_forUnitsInRange_addListener_doAfter_Whirlwind"
    )
    whirlwind_cb_source = data[int(whirlwind_cb_function["start"]):int(whirlwind_cb_function["end"])]
    if b"__wurst_safe_UnitDamageTarget(Oao,Pao,Qao,false,false,Rao,DAMAGE_TYPE_UNIVERSAL,WEAPON_TYPE_WHOKNOWS)" not in whirlwind_cb_source:
        raise ValueError("Ironpaw Whirlwind damage packet changed")
    rows.append({
        "unit_id": 1848652618,
        "mechanic_kind": "attack-proc-ground-whirlwind-aoe",
        "trigger": "damage-event",
        "parameters": {
            "marker_ability_id": 1093683271,
            "required_damage_event_type": 0,
            "proc_probability": 0.20,
            "random_roll_min": 0.0,
            "random_roll_max": 1.0,
            "proc_comparison": "roll <= 0.20",
            "center": "damaged-target-position",
            "radius": 160,
            "damage": 175,
            "tooltip_damage": 150,
            "tooltip_damage_disagrees_with_runtime": True,
            "target_predicate": "enemy-of-source;alive;combat-sapper;not-flying",
            "is_attack": False,
            "is_ranged": False,
            "attack_type": "normal",
            "damage_type": "universal",
            "animation": "Attack Walk Stand Spin",
            "queued_animation": "stand",
        },
        "related_rawcode_ids": [1093683271],
        "source_functions": [
            "qP", "DamageListener_addListener_doAfter_Whirlwind_onEvent_addListener_doAfter_Whirlwind",
            "ForGroupCallback_forUnitsInRange_addListener_doAfter_Whirlwind_callback_forUnitsInRange_addListener_doAfter_Whirlwind",
        ],
        "evidence_kind": "exact-marker-damage-listener-proc-and-area-callback",
        "byte_offset": min(whirlwind_init_start, whirlwind_start, whirlwind_cb_start),
    })

    # Nature attack procs: Dryad and Keeper remove positive magic buffs from
    # their attacked target, while Ancient Keeper applies the same dispel to
    # dispellable enemies in 100 range around the attacked unit.
    source_damage_start, _ = require_tokens(
        "handleSourceDamageEffects",
        {"1697656886", "1697656889", "1697656899", "dryadDispelProc", "keeperDispelProc", "ancientKeeperDispelProc"},
    )
    require_tokens(
        "dryadDispelProc",
        {"GetRandomInt", "15", "__wurst_safe_UnitRemoveBuffsEx"},
    )
    require_tokens(
        "keeperDispelProc",
        {"GetRandomInt", "20", "__wurst_safe_UnitRemoveBuffsEx"},
    )
    require_tokens(
        "ancientKeeperDispelProc",
        {"GetRandomInt", "25", "100.", "forUnitsInRange", "kk", "create393"},
    )
    require_tokens(
        "ForGroupCallback_forUnitsInRange_DamageProcs_callback_forUnitsInRange_DamageProcs",
        {"isDispellableEnemyOf", "__wurst_safe_UnitRemoveBuffsEx"},
    )
    for unit_id, chance, radius in (
        (1697656886, 15, 0),
        (1697656889, 20, 0),
        (1697656899, 25, 100),
    ):
        rows.append({
            "unit_id": unit_id,
            "mechanic_kind": "attack-proc-dispel-positive-buffs",
            "trigger": "source-damage-event",
            "parameters": {
                "proc_chance_percent": chance,
                "center": "attacked-target",
                "effect_radius": radius,
                "direct_target_only": radius == 0,
                "aoe_target_predicate": "isDispellableEnemyOf" if radius else "",
                "unit_remove_buffs_ex_args": [True, False, True, False, False, False, False],
            },
            "related_rawcode_ids": [],
            "source_functions": (
                ["handleSourceDamageEffects", "ancientKeeperDispelProc", "ForGroupCallback_forUnitsInRange_DamageProcs_callback_forUnitsInRange_DamageProcs"]
                if radius
                else ["handleSourceDamageEffects", "dryadDispelProc" if chance == 15 else "keeperDispelProc"]
            ),
            "evidence_kind": "exact-source-damage-handler",
            "byte_offset": source_damage_start,
        })

    # Bear/Ancient Bear: taking damage can proc two native buffs (damage and
    # attack speed) and also drives a custom low-HP retreat/sleep state machine.
    target_damage_start, _ = require_tokens(
        "handleTargetDamageEffects",
        {"1848652345", "1848652354", "15", "20", "255.", "326.", "feralRage", "startBearHibernate"},
    )
    require_tokens(
        "feralRage",
        {"GetRandomInt", "852066", "852101", "addProtectedAbility", "__wurst_safe_UnitApplyTimedLife", "2."},
    )
    require_tokens(
        "startBearHibernate",
        {"1093681972", "400.", "issueCodeTowardsOwnCastle", "5.", "doAfter"},
    )
    require_tokens(
        "CallbackSingle_doAfter_DamageRuntime_call_doAfter_DamageRuntime1",
        {"startBearSleep"},
    )
    require_tokens(
        "startBearSleep",
        {"270.", "__wurst_safe_UnitRemoveBuffs", "851993", "1098083425", "1093681974", "1093681975", "1093681976", "10.", "doAfter"},
    )
    require_tokens(
        "CallbackSingle_doAfter_DamageRuntime_call_doAfter_DamageRuntime",
        {"finishBearHibernate"},
    )
    require_tokens(
        "finishBearHibernate",
        {"1098083425", "1093681974", "1093681975", "orderCodeAttack"},
    )
    for unit_id, proc_chance, damage_ability, speed_ability, threshold, level, regen_ability, extra_sleep_ability in (
        (1848652345, 15, 1093681719, 1093681717, 255, 1, 1093681974, None),
        (1848652354, 20, 1093681720, 1093681718, 326, 2, 1093681975, 1093681976),
    ):
        related = [1093681973, 1093681972, damage_ability, speed_ability, 1098083425, regen_ability]
        if extra_sleep_ability is not None:
            related.append(extra_sleep_ability)
        rows.append({
            "unit_id": unit_id,
            "mechanic_kind": "damage-triggered-feral-rage-and-hibernation",
            "trigger": "target-damage-event",
            "parameters": {
                "runtime_damage_marker_ability_id": 1093681973,
                "feral_rage_proc_chance_percent": proc_chance,
                "feral_rage_damage_ability_id": damage_ability,
                "feral_rage_attack_speed_ability_id": speed_ability,
                "feral_rage_damage_order_id": 852066,
                "feral_rage_attack_speed_order_id": 852101,
                "feral_rage_dummy_lifetime_seconds": 2,
                "hibernate_enabled_ability_id": 1093681972,
                "hibernate_trigger_life_below": threshold,
                "hibernate_level": level,
                "hibernate_marker_removed_on_trigger": True,
                "hibernate_retreat_move_speed": 400,
                "hibernate_retreat_direction": "toward-own-castle",
                "hibernate_retreat_seconds": 5,
                "hibernate_sleep_move_speed": 270,
                "hibernate_remove_positive_and_negative_buffs_before_sleep": True,
                "hibernate_sleep_order_id": 851993,
                "hibernate_sleep_ability_id": 1098083425,
                "hibernate_regen_ability_id": regen_ability,
                "hibernate_extra_sleep_ability_id": extra_sleep_ability,
                "hibernate_sleep_seconds": 10,
                "resume_attack_after_hibernate": True,
            },
            "related_rawcode_ids": related,
            "source_functions": [
                "handleTargetDamageEffects",
                "feralRage",
                "startBearHibernate",
                "CallbackSingle_doAfter_DamageRuntime_call_doAfter_DamageRuntime1",
                "startBearSleep",
                "CallbackSingle_doAfter_DamageRuntime_call_doAfter_DamageRuntime",
                "finishBearHibernate",
            ],
            "evidence_kind": "exact-target-damage-and-delayed-callback-state-machine",
            "byte_offset": target_damage_start,
        })

    # Desert Razormane: damage events auto-cast its native Fan of Knives only
    # for normal damage while the protected mana/cooldown gates are satisfied.
    razormane_start, _ = require_tokens(
        "handleFanOfKnives",
        {
            "1848652373", "1093683020", "DAMAGE_TYPE_NORMAL", "BlzGetEventDamageType",
            "__wurst_safe_BlzGetUnitAbilityCooldownRemaining", "__wurst_safe_BlzGetUnitAbilityManaCost",
            "UNIT_STATE_MANA", "__wurst_safe_IssueImmediateOrder", "orderCodeAttack",
        },
    )
    razormane_function = next(function for function in functions if function["name"] == "handleFanOfKnives")
    razormane_source = data[int(razormane_function["start"]):int(razormane_function["end"])]
    if b'__wurst_safe_IssueImmediateOrder(iqq,"fanofknives")' not in razormane_source:
        raise ValueError("Razormane Fan of Knives order changed")
    rows.append({
        "unit_id": 1848652373,
        "mechanic_kind": "damage-triggered-auto-fan-of-knives",
        "trigger": "damage-event",
        "parameters": {
            "ability_id": 1093683020,
            "required_event_damage_type": "normal",
            "requires_ability_level_positive": True,
            "requires_cooldown_ready": True,
            "requires_current_mana_at_least_native_cost": True,
            "issued_order": "fanofknives",
            "resume_attack_immediately": True,
        },
        "related_rawcode_ids": [1093683020],
        "source_functions": ["handleFanOfKnives"],
        "evidence_kind": "exact-damage-handler-plus-native-ability",
        "byte_offset": razormane_start,
    })

    # Greater Elemental of Wind: every damage amount is accumulated into mana.
    # At 680 the target stops accumulating, gains native Kaboom and is ordered
    # to self-destruct; linked native explosion fields are joined downstream.
    require_tokens(
        "handleTargetDamageEffects",
        {"1865429061", "vampireCharge", "GetEventDamage"},
    )
    wind_start, _ = require_tokens(
        "vampireCharge",
        {
            "UNIT_STATE_MANA", "680.", "1093681973", "450.", "1093682510", "852041",
            "unit_removeAbility", "__wurst_safe_SetUnitMoveSpeed", "addProtectedAbility",
            "unit_issueImmediateOrderById", "__wurst_safe_SetUnitState",
        },
    )
    rows.append({
        "unit_id": 1865429061,
        "mechanic_kind": "damage-to-mana-kaboom-charge",
        "trigger": "target-damage-event",
        "parameters": {
            "runtime_damage_marker_ability_id": 1093681973,
            "charge_resource": "mana",
            "charge_delta": "event-damage-amount",
            "detonation_threshold": 680,
            "remove_damage_marker_on_threshold": True,
            "detonation_move_speed": 450,
            "kaboom_ability_id": 1093682510,
            "kaboom_order_id": 852041,
        },
        "related_rawcode_ids": [1093681973, 1093682510],
        "source_functions": ["handleTargetDamageEffects", "vampireCharge"],
        "evidence_kind": "exact-target-damage-handler-plus-native-explosion",
        "byte_offset": min(target_damage_start, wind_start),
    })

    # Emerald Dragon: every qualifying source-damage event adds/increments A0C9
    # on the attacked target, capped at level 3. The Spell Book levels encode
    # the actual -2/-4/-6 armor states and are resolved downstream.
    require_tokens(
        "handleSourceDamageEffects",
        {"1848652353", "tornadoStackProc"},
    )
    emerald_start, _ = require_tokens(
        "tornadoStackProc",
        {"1093681977", "3", "addProtectedAbility", "__wurst_safe_SetUnitAbilityLevel"},
    )
    rows.append({
        "unit_id": 1848652353,
        "mechanic_kind": "attack-stacking-corrosion",
        "trigger": "source-damage-event",
        "parameters": {
            "source_damage_marker_ability_id": 1093677905,
            "target_stack_ability_id": 1093681977,
            "initial_stack_level": 1,
            "maximum_stack_level": 3,
            "stack_increment_per_hit": 1,
        },
        "related_rawcode_ids": [1093677905, 1093681977],
        "source_functions": ["handleSourceDamageEffects", "tornadoStackProc"],
        "evidence_kind": "exact-source-damage-handler-plus-levelled-object-state",
        "byte_offset": min(source_damage_start, emerald_start),
    })

    # Greater Elemental of Water: source damage has a 20% chance to issue the
    # native Mirror Image order directly. A0CU contains the one-image 60/200%
    # damage-dealt/taken configuration and duration.
    require_tokens(
        "handleSourceDamageEffects",
        {"1747989081", "randomN02YOrder"},
    )
    water_start, _ = require_tokens(
        "randomN02YOrder",
        {"GetRandomInt", "20", "852123", "jY", "unit_issueImmediateOrderById"},
    )
    rows.append({
        "unit_id": 1747989081,
        "mechanic_kind": "attack-proc-native-mirror-image",
        "trigger": "source-damage-event",
        "parameters": {
            "source_damage_marker_ability_id": 1093677905,
            "proc_chance_percent": 20,
            "mirror_image_ability_id": 1093682005,
            "mirror_image_order_id": 852123,
            "bypasses_wrong_order_guard": True,
        },
        "related_rawcode_ids": [1093677905, 1093682005],
        "source_functions": ["handleSourceDamageEffects", "randomN02YOrder"],
        "evidence_kind": "exact-source-damage-handler-plus-native-ability",
        "byte_offset": min(source_damage_start, water_start),
    })

    # Lich King: source damage uses one random roll for Mastery over Death. An
    # eligible roll <15 heals for half the target's current life then deals a
    # 10,000 chaos/death hit; otherwise any roll <25 casts native Death and
    # Decay at the target position. Thus devour-ineligible targets can enter the
    # Death and Decay branch even on rolls 0..14.
    require_tokens(
        "handleSourceDamageEffects",
        {"1966092356", "devourAttackProc"},
    )
    lich_start, _ = require_tokens(
        "devourAttackProc",
        {
            "GetRandomInt", "15", "25", "1093678896", "UNIT_TYPE_MECHANICAL",
            "2.", "10000.", "ATTACK_TYPE_CHAOS", "DAMAGE_TYPE_DEATH", "1093679158", "852221", "6.",
            "__wurst_safe_SetWidgetLife", "__wurst_safe_UnitDamageTarget", "addProtectedAbility",
            "unit_issuePointOrderById", "__wurst_safe_UnitApplyTimedLife",
        },
    )
    lich_function = next(function for function in functions if function["name"] == "devourAttackProc")
    lich_source = data[int(lich_function["start"]):int(lich_function["end"])]
    if b"widget_getLife(noq)<.405" not in lich_source:
        raise ValueError("Lich King Devour alive threshold changed")
    rows.append({
        "unit_id": 1966092356,
        "mechanic_kind": "source-damage-mastery-over-death",
        "trigger": "source-damage-event",
        "parameters": {
            "source_damage_marker_ability_id": 1093677905,
            "random_roll_min": 0,
            "random_roll_max": 99,
            "devour_roll_threshold_exclusive": 15,
            "devour_target_excluded_ability_id": 1093678896,
            "devour_target_requires_not_mechanical": True,
            "devour_target_requires_alive_after_damage": True,
            "devour_heal_fraction_of_target_current_hp": 0.5,
            "devour_damage": 10000,
            "devour_attack_type": "chaos",
            "devour_damage_type": "death",
            "devour_is_attack": True,
            "devour_is_ranged": False,
            "death_and_decay_roll_threshold_exclusive": 25,
            "death_and_decay_branch_runs_when_devour_condition_fails": True,
            "death_and_decay_probability_if_devour_eligible_percent": 10,
            "death_and_decay_probability_if_devour_ineligible_percent": 25,
            "death_and_decay_ability_id": 1093679158,
            "death_and_decay_order_id": 852221,
            "death_and_decay_cast_position": "damaged-target-position",
            "death_and_decay_dummy_lifetime_seconds": 6,
        },
        "related_rawcode_ids": [1093677905, 1093678896, 1093679158],
        "source_functions": ["handleSourceDamageEffects", "devourAttackProc"],
        "evidence_kind": "exact-source-damage-handler-plus-native-area-spell",
        "byte_offset": min(source_damage_start, lich_start),
    })

    # Vampire Lord: every damage event sourced from the Lord applies A0GY to a
    # non-structure target or increments it by one level, capped at level 5.
    damage_dispatch_start, _ = require_tokens(
        "code__addAction_DamageRuntime",
        {"GetEventDamageSource", "GetTriggerUnit", "applyVampireArmorReduction"},
    )
    vampire_armor_start, _ = require_tokens(
        "applyVampireArmorReduction",
        {"1747989832", "UNIT_TYPE_STRUCTURE", "1093683033", "5", "addProtectedAbility", "__wurst_safe_SetUnitAbilityLevel"},
    )
    rows.append({
        "unit_id": 1747989832,
        "mechanic_kind": "source-damage-stacking-blood-corrosion",
        "trigger": "damage-event",
        "parameters": {
            "target_must_not_be_structure": True,
            "target_stack_ability_id": 1093683033,
            "initial_stack_level": 1,
            "maximum_stack_level": 5,
            "stack_increment_per_damage_event": 1,
        },
        "related_rawcode_ids": [1093683033],
        "source_functions": ["code__addAction_DamageRuntime", "applyVampireArmorReduction"],
        "evidence_kind": "exact-damage-dispatch-plus-levelled-object-state",
        "byte_offset": min(damage_dispatch_start, vampire_armor_start),
    })

    # Earth Elementals: the shared source-damage handler adds a second,
    # health-scaled siege/demolition hit whenever A0DW is present. The formula
    # is exact script behavior and deliberately uses current HP / max HP.
    require_tokens(
        "handleSourceDamageEffects",
        {
            "1093682263", "50.", "unit_getLevel", "widget_getLife", "unit_getMaxHP",
            "ATTACK_TYPE_SIEGE", "DAMAGE_TYPE_DEMOLITION", "__wurst_safe_DisableTrigger",
            "__wurst_safe_UnitDamageTarget", "__wurst_safe_EnableTrigger",
        },
    )
    source_damage_function = next(
        function for function in functions if function["name"] == "handleSourceDamageEffects"
    )
    source_damage_source = data[int(source_damage_function["start"]):int(source_damage_function["end"])]
    earth_formula = (
        b"__wurst_safe_UnitDamageTarget(Qpq,Rpq,(((50.*unit_getLevel(Qpq))*widget_getLife(Qpq))/unit_getMaxHP(Qpq)),"
        b"true,false,ATTACK_TYPE_SIEGE,DAMAGE_TYPE_DEMOLITION"
    )
    if earth_formula not in source_damage_source:
        raise ValueError("Earth Elemental Aftershock formula changed")
    for unit_id, unit_level in ((1747989300, 1), (1747989326, 2)):
        rows.append({
            "unit_id": unit_id,
            "mechanic_kind": "source-damage-health-scaled-aftershock",
            "trigger": "source-damage-event",
            "parameters": {
                "source_damage_marker_ability_id": 1093677905,
                "aftershock_marker_ability_id": 1093682263,
                "unit_level": unit_level,
                "bonus_damage_formula": "50 * unit_level * current_hp / max_hp",
                "maximum_bonus_damage_at_full_hp": 50 * unit_level,
                "attack_type": "siege",
                "damage_type": "demolition",
                "is_attack": True,
                "is_ranged": False,
                "recursion_guard_trigger_disabled_during_bonus_hit": True,
            },
            "related_rawcode_ids": [1093677905, 1093682263],
            "source_functions": ["handleSourceDamageEffects"],
            "evidence_kind": "exact-marker-driven-source-damage-formula",
            "byte_offset": source_damage_start,
        })

    # Lightning Elementals: damage from a melee attacker can proc a dummy
    # Thunderbolt. The script uses an inclusive random comparison, so levels 1
    # and 2 have 16 and 31 successful integer rolls out of 100 respectively.
    lightning_start, _ = require_tokens(
        "castLightningShield",
        {
            "UNIT_TYPE_MELEE_ATTACKER", "ATTACK_TYPE_NORMAL", "GetRandomInt", "15", "1093682229",
            "852095", "4.", "unit_getLevel", "addProtectedAbility", "__wurst_safe_SetUnitAbilityLevel",
            "__wurst_safe_IssueTargetOrderById", "__wurst_safe_UnitApplyTimedLife",
        },
    )
    lightning_function = next(function for function in functions if function["name"] == "castLightningShield")
    lightning_source = data[int(lightning_function["start"]):int(lightning_function["end"])]
    if b"GetRandomInt(0,99)<=(15*ppq)" not in lightning_source:
        raise ValueError("Lightning Elemental retaliation probability changed")
    if b"not(BlzGetEventAttackType()==ATTACK_TYPE_NORMAL)" not in lightning_source:
        raise ValueError("Lightning Elemental attack-type gate changed")
    for unit_id, unit_level in ((1747989328, 1), (1747989330, 2)):
        rows.append({
            "unit_id": unit_id,
            "mechanic_kind": "target-damage-melee-thunderbolt-retaliation",
            "trigger": "target-damage-event",
            "parameters": {
                "target_marker_ability_id": 1093682264,
                "unit_level": unit_level,
                "attacker_requires_melee_attacker_type": True,
                "event_attack_type_must_not_equal": "normal",
                "random_roll_min": 0,
                "random_roll_max": 99,
                "proc_comparison": "roll <= 15 * unit_level",
                "successful_roll_count": 15 * unit_level + 1,
                "effective_proc_probability_percent": 15 * unit_level + 1,
                "thunderbolt_ability_id": 1093682229,
                "thunderbolt_ability_level": unit_level,
                "thunderbolt_order_id": 852095,
                "dummy_lifetime_seconds": 4,
                "bypasses_wrong_order_guard": True,
            },
            "related_rawcode_ids": [1093682264, 1093682229],
            "source_functions": ["handleTargetDamageEffects", "castLightningShield"],
            "evidence_kind": "exact-marker-driven-target-damage-native-cast",
            "byte_offset": min(target_damage_start, lightning_start),
        })

    # Paladin: the summon-event path overrides its object-data starting mana to
    # exactly 30 before returning it to normal attack behavior. Keep this scoped
    # to summoned Paladins; ordinary object-data mana_start remains separate.
    paladin_summon_start, _ = require_tokens(
        "onSummonedUnit",
        {"1747989315", "UNIT_STATE_MANA", "30.", "__wurst_safe_SetUnitState", "orderCodeAttackIfAllowed"},
    )
    paladin_summon_function = next(function for function in functions if function["name"] == "onSummonedUnit")
    paladin_summon_source = data[int(paladin_summon_function["start"]):int(paladin_summon_function["end"])]
    if b"if(tXr==1747989315)then" not in paladin_summon_source:
        raise ValueError("Paladin summon mana branch changed")
    rows.append({
        "unit_id": 1747989315,
        "mechanic_kind": "summon-event-mana-reset",
        "trigger": "unit-summon-event",
        "parameters": {
            "set_mana_to": 30,
            "resume_attack_if_allowed": True,
        },
        "related_rawcode_ids": [],
        "source_functions": ["onSummonedUnit"],
        "evidence_kind": "exact-summon-event-branch",
        "byte_offset": paladin_summon_start,
    })

    # Mine Layer and Goblin Rocketeer have train-event initialization that is
    # not represented by their object data or scripted spell registration.
    train_start, _ = require_tokens(
        "onUnitTrained",
        {
            "1747990101", "1848652365", "UNIT_STATE_MANA", "GetRandomReal", "30.",
            "__wurst_safe_SetUnitState", "__wurst_safe_SetUnitExploded",
        },
    )
    train_function = next(function for function in functions if function["name"] == "onUnitTrained")
    train_source = data[int(train_function["start"]):int(train_function["end"])]
    if b"GetRandomReal(.20,30.)" not in train_source:
        raise ValueError("Mine Layer trained-mana random range changed")
    if b"__wurst_safe_SetUnitExploded(cgs,true)" not in train_source:
        raise ValueError("Goblin Rocketeer exploded flag changed")
    rows.append({
        "unit_id": 1747990101,
        "mechanic_kind": "train-event-random-initial-mana",
        "trigger": "unit-trained-event",
        "parameters": {
            "mana_random_min": 0.20,
            "mana_random_max": 30,
            "mana_random_distribution": "uniform-real",
        },
        "related_rawcode_ids": [],
        "source_functions": ["onUnitTrained"],
        "evidence_kind": "exact-train-event-branch",
        "byte_offset": train_start,
    })
    rows.append({
        "unit_id": 1848652365,
        "mechanic_kind": "train-event-set-exploded-flag",
        "trigger": "unit-trained-event",
        "parameters": {
            "set_unit_exploded": True,
        },
        "related_rawcode_ids": [1093682778],
        "source_functions": ["onUnitTrained", "fJ"],
        "evidence_kind": "exact-train-event-flag-plus-native-death-explosion",
        "byte_offset": train_start,
    })

    # Human Defender: Defend is automatically enabled shortly after spawn, then
    # normal attack movement resumes. If the unit receives the native undefend
    # order, FixDefend re-enables Defend after 5.5 seconds. This keeps A03G's
    # native ranged-damage/deflection behavior active without custom damage math.
    defender_spawn_start, _ = require_tokens(
        "onSummonedUnit",
        {"1747989313", "Cy", "create1000", "doAfter"},
    )
    require_tokens(
        "CallbackSingle_doAfter_ReengageRuntime_call_doAfter_ReengageRuntime1",
        {"852055", "unit_issueImmediateOrderById", "jY", "Dy", "create1001", "doAfter"},
    )
    require_tokens(
        "CallbackSingle_doAfter_doAfter_ReengageRuntime_call_doAfter_doAfter_ReengageRuntime1",
        {"orderCodeAttackIfAllowed"},
    )
    defender_order_start, _ = require_tokens(
        "EventListener_add_doAfter_FixDefend_onEvent_add_doAfter_FixDefend",
        {"1747989313", "N6", "Um", "create506", "5.5", "doAfter"},
    )
    require_tokens(
        "CallbackSingle_doAfter_add_doAfter_FixDefend_call_doAfter_add_doAfter_FixDefend",
        {"U6", "unit_issueImmediateOrderById"},
    )
    spawn_function = next(function for function in functions if function["name"] == "onSummonedUnit")
    spawn_source = data[int(spawn_function["start"]):int(spawn_function["end"])]
    if b"doAfter(.7,xXr)" not in spawn_source:
        raise ValueError("Defender initial auto-defend delay changed")
    defend_callback = next(
        function for function in functions
        if function["name"] == "CallbackSingle_doAfter_ReengageRuntime_call_doAfter_ReengageRuntime1"
    )
    defend_callback_source = data[int(defend_callback["start"]):int(defend_callback["end"])]
    if b"doAfter(.1,yBn)" not in defend_callback_source:
        raise ValueError("Defender post-defend attack resume delay changed")
    rows.append({
        "unit_id": 1747989313,
        "mechanic_kind": "automatic-defend-state-maintenance",
        "trigger": "summon-and-order-events",
        "parameters": {
            "defend_ability_id": 1093677895,
            "defend_order_id": 852055,
            "undefend_order_id": 852056,
            "initial_defend_delay_seconds": 0.7,
            "resume_attack_delay_after_defend_seconds": 0.1,
            "undefend_reactivation_delay_seconds": 5.5,
            "auto_defend_bypasses_wrong_order_guard": True,
        },
        "related_rawcode_ids": [1093677895],
        "source_functions": [
            "onSummonedUnit",
            "CallbackSingle_doAfter_ReengageRuntime_call_doAfter_ReengageRuntime1",
            "CallbackSingle_doAfter_doAfter_ReengageRuntime_call_doAfter_doAfter_ReengageRuntime1",
            "EventListener_add_doAfter_FixDefend_onEvent_add_doAfter_FixDefend",
            "CallbackSingle_doAfter_add_doAfter_FixDefend_call_doAfter_add_doAfter_FixDefend",
        ],
        "evidence_kind": "exact-summon-order-and-delayed-callback-chain",
        "byte_offset": min(defender_spawn_start, defender_order_start),
    })

    # Shared death/kill handler: normalize the exact production-unit branches
    # that are otherwise invisible to both object data and the scripted spell
    # registries. M2q is the dying combat sapper and N2q is its killer.
    death_function = next(function for function in functions if function["name"] == "fJ")
    death_source = data[int(death_function["start"]):int(death_function["end"])]
    require_tokens(
        "fJ",
        {
            "1697656897", "1697656898", "1747988821", "1747988822", "1747989832",
            "1848651862", "1848652111", "1848652359", "350.", "14.", "20.", "852100",
            "ATTACK_TYPE_CHAOS", "DAMAGE_TYPE_NORMAL", "UNIT_TYPE_STRUCTURE", "UNIT_TYPE_MECHANICAL",
            "UNIT_TYPE_UNDEAD", "isNotTentacle", "createUnit", "__wurst_safe_RemoveUnit",
            "unit_issueImmediateOrderById", "orderCodeAttack", "__wurst_safe_UnitDamageTarget",
        },
    )
    required_death_fragments = {
        b"if(unit_getTypeId(M2q)==1697656897)then": "Avatar death retaliation branch",
        b"__wurst_safe_UnitDamageTarget(M2q,N2q,350.,true,false,ATTACK_TYPE_CHAOS,DAMAGE_TYPE_NORMAL": "Avatar death retaliation damage",
        b"R2q==1697656898": "Avenging Spirit kill-heal branch",
        b"GetUnitLifePercent(N2q)+14.": "Avenging Spirit kill-heal amount",
        b"R2q==1697656897": "Avatar kill-heal branch",
        b"GetUnitLifePercent(N2q)+20.": "Avatar kill-heal amount",
        b"R2q==1747988821": "Vampire servitude branch",
        b"createUnit(unit_getOwner(N2q),1747988822": "Vampire lesser-vampire spawn",
        b"R2q==1747989832": "Vampire Lord servitude branch",
        b"createUnit(unit_getOwner(N2q),1747988821": "Vampire Lord vampire spawn",
        b"R2q==1848651862": "Troll Berserker kill branch",
        b"R2q==1848652111": "Troll Trapper kill branch",
        b"R2q==1848652359": "Forest Troll Trapper kill branch",
        b"unit_issueImmediateOrderById(N2q,852100)": "Troll-family Berserk order",
    }
    for fragment, label in required_death_fragments.items():
        if fragment not in death_source:
            raise ValueError(f"{label} changed")

    rows.append({
        "unit_id": 1697656897,
        "mechanic_kind": "death-retaliation-damage-to-killer",
        "trigger": "death-event",
        "parameters": {
            "dying_unit_requires_combat_sapper": True,
            "killer_required": True,
            "killer_must_not_be_structure": True,
            "damage": 350,
            "attack_type": "chaos",
            "damage_type": "normal",
            "is_attack": True,
            "is_ranged": False,
            "damage_source": "dying-unit",
            "damage_target": "killer",
        },
        "related_rawcode_ids": [],
        "source_functions": ["fJ"],
        "evidence_kind": "exact-shared-death-handler",
        "byte_offset": death_start,
    })

    for unit_id, heal_percent in ((1697656898, 14), (1697656897, 20)):
        rows.append({
            "unit_id": unit_id,
            "mechanic_kind": "kill-heal-percent-max-hp",
            "trigger": "kill-event",
            "parameters": {
                "victim_requires_combat_sapper": True,
                "killer_must_not_be_structure": True,
                "heal_percent_of_max_hp": heal_percent,
                "implementation": "set-life-percent-current-plus-delta",
            },
            "related_rawcode_ids": [],
            "source_functions": ["fJ"],
            "evidence_kind": "exact-shared-death-handler",
            "byte_offset": death_start,
        })

    for unit_id, summoned_unit_id in ((1747988821, 1747988822), (1747989832, 1747988821)):
        rows.append({
            "unit_id": unit_id,
            "mechanic_kind": "organic-kill-eternal-servitude",
            "trigger": "kill-event",
            "parameters": {
                "victim_requires_combat_sapper": True,
                "killer_must_not_be_structure": True,
                "victim_requires_not_tentacle": True,
                "victim_requires_not_mechanical": True,
                "victim_requires_not_undead": True,
                "spawn_owner": "killer-owner",
                "spawn_position": "victim-position",
                "spawn_facing": 0,
                "spawned_unit_id": summoned_unit_id,
                "remove_original_dead_unit": True,
            },
            "related_rawcode_ids": [summoned_unit_id],
            "source_functions": ["fJ"],
            "evidence_kind": "exact-shared-death-handler",
            "byte_offset": death_start,
        })

    for unit_id in (1848651862, 1848652111, 1848652359):
        rows.append({
            "unit_id": unit_id,
            "mechanic_kind": "kill-triggered-native-berserk",
            "trigger": "kill-event",
            "parameters": {
                "victim_requires_combat_sapper": True,
                "killer_must_not_be_structure": True,
                "berserk_ability_id": 1093677641,
                "berserk_order_id": 852100,
                "resume_attack_immediately": True,
            },
            "related_rawcode_ids": [1093677641],
            "source_functions": ["fJ"],
            "evidence_kind": "exact-death-handler-plus-native-ability",
            "byte_offset": death_start,
        })

    # Greater Fire Elemental has an intentionally misleading train proxy. The
    # Altar of Blaze trains u00F, but onUnitTrained immediately hides that unit,
    # issues its native Lava Spawn order, and applies a one-second timed life.
    # A0DY/ANlm creates h030, which is the actual battlefield Greater Fire
    # Elemental. WC3's native ANlm split engine then owns the attack counter and
    # generation mechanics. Castle Fight's summon listener strips the summoned
    # classification from h030 children and converts an h030 child produced by
    # another h030 into ordinary Fire Elemental h02Z. Keep the native ANlm
    # fields joined downstream from object data rather than treating u00F's
    # deliberately poisoned Ghoul-derived combat fields as gameplay stats.
    train_start, _ = require_tokens(
        "onUnitTrained",
        {
            "1093682266", "__wurst_safe_ShowUnit", "852667", "__wurst_safe_UnitApplyTimedLife", "1112820806", "1.",
        },
    )
    train_function = next(function for function in functions if function["name"] == "onUnitTrained")
    train_source = data[int(train_function["start"]):int(train_function["end"])]
    for fragment in (
        b"unit_getAbilityLevel(cgs,1093682266)>0",
        b"__wurst_safe_ShowUnit(igs,false)",
        b"unit_issueImmediateOrderById(cgs,852667)",
        b"__wurst_safe_UnitApplyTimedLife(cgs,1112820806,1.)",
    ):
        if fragment not in train_source:
            raise ValueError(f"Greater Fire Elemental train-proxy handoff changed: missing {fragment!r}")

    fire_listener_start, _ = require_tokens(
        "EventListener_add_FireElemental_onEvent_add_FireElemental",
        {
            "1747989296", "1747989082", "1966092358", "UNIT_TYPE_SUMMONED",
            "__wurst_safe_UnitRemoveType", "__wurst_safe_ReplaceUnitBJ", "setupFireSummon", "doAfter",
        },
    )
    require_tokens(
        "setupFireSummon",
        {"trackTrainedUnitPathing", "orderCodeAttackIfAllowed", "Pm", "create501", "doAfter"},
    )
    require_tokens(
        "CallbackSingle_doAfter_FireElemental_call_doAfter_FireElemental",
        {"orderCodeAttackIfAllowed"},
    )
    require_tokens(
        "EventListener_add_PerkUtils_onEvent_add_PerkUtils",
        {"GetSummonedUnit", "GetSummoningUnit", "1747989296", "1966092358", "unit_getIndex", "J2"},
    )
    require_tokens(
        "setSpawnBuilding",
        {"unit_getIndex", "K2", "J2"},
    )
    listener_function = next(
        function for function in functions
        if function["name"] == "EventListener_add_FireElemental_onEvent_add_FireElemental"
    )
    listener_source = data[int(listener_function["start"]):int(listener_function["end"])]
    if b"__wurst_safe_ReplaceUnitBJ(jxm,1747989082,2)" not in listener_source:
        raise ValueError("Greater Fire Elemental child conversion changed")
    setup_function = next(function for function in functions if function["name"] == "setupFireSummon")
    setup_source = data[int(setup_function["start"]):int(setup_function["end"])]
    if b"doAfter(0.2,dzq)" not in setup_source:
        raise ValueError("Fire Elemental post-split attack setup delay changed")
    rows.append({
        "unit_id": 1966092358,
        "mechanic_kind": "native-lava-spawn-split-with-child-conversion",
        "trigger": "trained-proxy-native-ANlm-summon-and-split-events",
        "parameters": {
            "trained_proxy_unit_id": 1966092358,
            "trained_proxy_marker_ability_id": 1093682266,
            "trained_proxy_lava_spawn_ability_id": 1093682265,
            "trained_proxy_hidden_immediately": True,
            "trained_proxy_immediate_order_id": 852667,
            "trained_proxy_timed_life_ability_id": 1112820806,
            "trained_proxy_timed_life_seconds": 1.0,
            "trained_proxy_is_gameplay_combat_body": False,
            "runtime_combat_unit_id": 1747989296,
            "runtime_combat_unit_created_by_native_ANlm": True,
            "split_ability_id": 1093682265,
            "first_split_child_unit_id": 1747989296,
            "nested_h030_child_replacement_unit_id": 1747989082,
            "nested_h030_child_replace_method": 2,
            "remove_summoned_type_from_h030_child": True,
            "post_child_setup_tracks_pathing": True,
            "post_child_setup_orders_attack": True,
            "post_child_attack_order_delay_seconds": 0.2,
            "split_child_inherits_spawn_building_provenance": True,
            "split_child_provenance_parent_unit_id": 1966092358,
            "split_child_provenance_child_unit_id": 1747989296,
            "split_child_provenance_flow": "summon listener caches h030 by u00F parent index; setSpawnBuilding propagates the parent's production building to the cached child and clears the cache",
            "native_split_state_machine": "Warcraft-ANlm",
        },
        "related_rawcode_ids": [1093682265, 1747989296, 1747989082],
        "source_functions": [
            "onUnitTrained",
            "EventListener_add_FireElemental_onEvent_add_FireElemental",
            "setupFireSummon",
            "CallbackSingle_doAfter_FireElemental_call_doAfter_FireElemental",
            "EventListener_add_PerkUtils_onEvent_add_PerkUtils",
            "setSpawnBuilding",
        ],
        "evidence_kind": "exact-trained-proxy-handoff-plus-native-ANlm-object-data-and-summon-listener",
        "byte_offset": min(train_start, fire_listener_start),
    })

    return rows


def _extract_building_improvement_spawn_mechanics(
    data: bytes,
    functions: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Recover and classify cross-runtime code keyed off building improvements.

    Mana Generator's A0EL target improvement is registered as a unit spell. A
    later setupUnit branch also checks A0EL and can grant a one-shot trait, but
    legal Mana Generator targets explicitly exclude production buildings while
    setupUnit is only called for units cloned from production-building spawn
    paths. Preserve that code as unreachable evidence so importers do not treat
    it as live Mana Generator gameplay.
    """
    available = {str(function["name"]) for function in functions}
    required = {
        "CFBuilding_CFBuilding_setup", "improveSpecialBuilding", "setupUnit",
        "spawnSyncedCompanions", "EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning",
        "handleTargetDamageEffects", "deathPactEmergency", "ML", "NL",
        "CallbackSingle_doAfter_SpecialAttackRuntime_call_doAfter_SpecialAttackRuntime",
    }
    if not required.issubset(available):
        return []

    def source(name: str) -> tuple[int, bytes, set[str]]:
        body = _function_body_tokens(data, functions, name)
        if body is None:
            raise ValueError(f"building-improvement runtime source missing: {name}")
        start, tokens = body
        function = next(function for function in functions if function["name"] == name)
        raw = data[int(function["start"]):int(function["end"])]
        return start, raw, {token.text for token in tokens}

    catalog_start, catalog_source, catalog_tokens = source("CFBuilding_CFBuilding_setup")
    improve_start, improve_source, improve_tokens = source("improveSpecialBuilding")
    setup_start, setup_source, setup_tokens = source("setupUnit")
    companion_group_start, companion_group_source, companion_group_tokens = source("spawnSyncedCompanions")
    companion_listener_start, companion_listener_source, companion_listener_tokens = source(
        "EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning"
    )
    target_start, target_source, target_tokens = source("handleTargetDamageEffects")
    emergency_start, emergency_source, emergency_tokens = source("deathPactEmergency")
    attack_init_start, attack_init_source, attack_init_tokens = source("ML")
    attack_trigger_start, attack_trigger_source, attack_trigger_tokens = source("NL")
    attack_order_start, attack_order_source, attack_order_tokens = source(
        "CallbackSingle_doAfter_SpecialAttackRuntime_call_doAfter_SpecialAttackRuntime"
    )

    required_improve = {
        "1093682508", "1093682259", "2", "addProtectedAbility",
        "__wurst_safe_UnitMakeAbilityPermanent", "__wurst_safe_SetUnitAbilityLevel",
    }
    if not required_improve.issubset(improve_tokens):
        raise ValueError("Mana Generator building-improvement setup changed")
    if b"addProtectedAbility(ipr,1093682508)" not in improve_source:
        raise ValueError("Mana Generator no longer grants A0EL to improved building")
    if b"__wurst_safe_UnitMakeAbilityPermanent(bpr,true,1093682508)" not in improve_source:
        raise ValueError("Mana Generator A0EL permanence changed")
    if b"(__wurst_ensureInt(hIb[buildingTypeIndexOfUnit(bpr)])>0)" not in improve_source:
        raise ValueError("Mana Generator production-building rejection changed")
    if b"hIb[buildingTypeIndex(aTk)]=bTk" not in catalog_source:
        raise ValueError("CFBuilding production-spawn rawcode mapping changed")

    required_setup = {"1093682508", "1093682522", "1093681973", "GetRandomInt", "30", "addProtectedAbility"}
    if not required_setup.issubset(setup_tokens):
        raise ValueError("A0EL per-spawn trait setup changed")
    if b"(unit_getAbilityLevel(Ofs,1093682508)>0)and(GetRandomInt(0,99)<30)" not in setup_source:
        raise ValueError("A0EL per-spawn trait probability changed")
    if b"addProtectedAbility(Tfs,1093682522)" not in setup_source or b"addProtectedAbility(Ufs,1093681973)" not in setup_source:
        raise ValueError("A0EL per-spawn granted abilities changed")
    if companion_group_source.count(b"setupUnit(") != 2:
        raise ValueError("synchronized-companion setupUnit call count changed")
    if companion_listener_source.count(b"setupUnit(") != 3:
        raise ValueError("direct-companion setupUnit call count changed")
    if data.count(b"setupUnit(") != 6:
        raise ValueError("setupUnit acquired an unexpected caller")

    if not {"1093681973", "1093682522", "deathPactEmergency"}.issubset(target_tokens):
        raise ValueError("A0EL spawn trait target-damage dispatch changed")
    if b"unit_getAbilityLevel(gqq,1093681973)<=0" not in target_source:
        raise ValueError("A0EL spawn trait target-damage marker gate changed")
    if b"unit_getAbilityLevel(gqq,1093682522)>0" not in target_source:
        raise ValueError("A0EL spawn trait emergency marker branch changed")

    if not {"128.", "5000.", "1093682522", "__wurst_safe_SetWidgetLife", "unit_removeAbility"}.issubset(emergency_tokens):
        raise ValueError("A0EL spawn trait emergency handler changed")
    if b"(Bpq<128.)and(Bpq>.405)" not in emergency_source:
        raise ValueError("A0EL spawn trait emergency threshold changed")
    if b"__wurst_safe_SetWidgetLife(Apq,5000.)" not in emergency_source:
        raise ValueError("A0EL spawn trait emergency life reset changed")

    if "EVENT_PLAYER_UNIT_ATTACKED" not in attack_init_tokens or "lV" not in attack_init_tokens:
        raise ValueError("A0EL spawn trait attacked-event registration changed")
    if b"unit_getAbilityLevel(oRr,1093682522)<=0" not in attack_trigger_source:
        raise ValueError("A0EL attacked-event A0EZ gate changed")
    if b"unit_removeAbility(oRr,1093682522)" not in attack_trigger_source:
        raise ValueError("A0EL attacked-event A0EZ consumption changed")
    if not {"852601", "issueCodeTargetOrder", "orderCodeAttack"}.issubset(attack_order_tokens):
        raise ValueError("A0EL attacked-event special order changed")
    if b"if(not issueCodeTargetOrder(wOn.attacker,852601,wOn.target))then orderCodeAttack(wOn.attacker)end" not in attack_order_source:
        raise ValueError("A0EL attacked-event target-order fallback changed")

    return [{
        "source_unit_id": 1747990066,
        "mechanic_kind": "unreachable-production-spawn-branch-keyed-by-A0EL",
        "trigger": "setupUnit-companion-spawn-path-only",
        "parameters": {
            "improvement_ability_id": 1093682508,
            "elemental_building_marker_ability_id": 1093682259,
            "gameplay_reachable_from_legal_mana_generator_target": False,
            "mana_generator_rejects_production_buildings": True,
            "production_building_detection": "hIb[buildingTypeIndexOfUnit(target)] > 0",
            "production_building_mapping_semantics": "hIb[buildingTypeIndex(buildingRawcode)] = spawnedUnitRawcode",
            "setup_unit_callers": [
                "spawnSyncedCompanions",
                "EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning",
            ],
            "normal_unit_train_finish_uses_setup_unit": False,
            "normal_improvement_level": 1,
            "elemental_improvement_level": 2,
            "spawn_trait_roll_min": 0,
            "spawn_trait_roll_max": 99,
            "spawn_trait_roll_threshold_exclusive": 30,
            "spawn_trait_probability_percent": 30,
            "spawn_granted_emergency_ability_id": 1093682522,
            "spawn_granted_damage_dispatch_marker_id": 1093681973,
            "emergency_trigger_current_life_below": 128,
            "emergency_trigger_current_life_above": 0.405,
            "emergency_set_current_life_to": 5000,
            "emergency_removes_ability_after_trigger": True,
            "attacked_event_consumes_same_ability": True,
            "attacked_event_attempted_target_order_id": 852601,
            "attacked_event_attempted_target_order_name": "parasite",
            "attacked_event_target": "attacking-event-target",
            "attacked_event_fallback_if_order_fails": "orderCodeAttack",
            "attacked_and_emergency_branches_are_mutually_exclusive_after_first_A0EZ_consumption": True,
        },
        "related_rawcode_ids": [1093682508, 1093682259, 1093682522, 1093681973],
        "source_functions": [
            "CFBuilding_CFBuilding_setup", "improveSpecialBuilding", "setupUnit",
            "spawnSyncedCompanions", "EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning",
            "handleTargetDamageEffects", "deathPactEmergency", "ML", "NL",
            "CallbackSingle_doAfter_SpecialAttackRuntime_call_doAfter_SpecialAttackRuntime",
        ],
        "evidence_kind": "exact-unreachable-cross-runtime-branch-with-production-target-exclusion-and-complete-setupUnit-caller-proof",
        "byte_offset": min(
            catalog_start, improve_start, setup_start, companion_group_start, companion_listener_start,
            target_start, emergency_start, attack_init_start, attack_trigger_start, attack_order_start,
        ),
    }]


def _extract_runtime_system_mechanics(
    data: bytes,
    functions: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Recover gameplay systems whose effects cut across unit/building rows."""
    available = {str(function["name"]) for function in functions}
    required = {
        "dK", "registerPowerPlant",
        "ForGroupCallback_forEachIn_PowerPlantRuntime_callback_forEachIn_PowerPlantRuntime",
        "ForGroupCallback_forUnitsInRange_PowerPlantRuntime_callback_forUnitsInRange_PowerPlantRuntime",
        "applyPowerArmor", "hasNoPowerArmorExclusion", "setupUnit",
        "kP", "acquireHeroicShrine", "spawnSyncedCompanions",
        "EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning",
        "ME", "acquireElvenShrine", "elvenShrineEffectiveReviveChance", "fJ",
        "CallbackSingle_doAfter_OnUnitDeathHandler_call_doAfter_OnUnitDeathHandler",
        "markShrineReviveBlocked", "onBuildingFinished", "acquireLinker", "eleLinkerHealAmount", "changeEleBuildingCount", "wL",
        "GH", "calcTreasureBoxMultiplier", "rawIncomeWithTreasureBox", "acquireTreasureBox", "SE",
        "applyTreasureBoxModeEnabled", "AH", "registerHumanArtillery",
        "ForGroupCallback_forEachIn_HumanArtilleryRuntime_callback_forEachIn_HumanArtilleryRuntime",
        "EventListener_add_HumanArtilleryRuntime_onEvent_add_HumanArtilleryRuntime1",
        "EventListener_add_HumanArtilleryRuntime_onEvent_add_HumanArtilleryRuntime2",
        "cG", "replaceChaosPortalSummon", "onChaosPortalUnitSummoned",
        "mH", "EventListener_add_GjallarHorn_onEvent_add_GjallarHorn",
        "Action_watch_GjallarHorn_run_watch_GjallarHorn",
        "BuildingSpellClosure_registerBuildingSpell_GjallarHorn_cast_registerBuildingSpell_GjallarHorn",
        "lE", "nH", "isSupportOrderUnit", "orderAssassinOrGoboW", "shouldRunPeriodicSupportOrderForState",
        "prepareAssassinTargets", "isValidAssassinTarget", "orderAssassinW", "restoreAssassinTargetOrderW",
        "prepareGobboTargets", "isGobboRepairableTarget", "isGobboRallyTarget", "orderGoboW",
        "setGobboSpawner", "SupportOrderTask_SupportOrderTask_run", "ensureSupportOrderTask",
        "sH", "applyGobboTimedLife", "onSummonedUnit",
        "IL", "CallbackSingle_doAfter_SpamPrevention_call_doAfter_SpamPrevention",
        "EventListener_add_doAfter_SpamPrevention_onEvent_add_doAfter_SpamPrevention",
        "EventListener_add_doAfter_SpamPrevention_onEvent_add_doAfter_SpamPrevention1",
        "EventListener_add_doAfter_SpamPrevention_onEvent_add_doAfter_SpamPrevention2",
        "getOrderSpamTimeoutSeconds", "nextOrderSpamTimeout", "shouldStartOrderSpamPenalty",
        "disabledUntilForNewOrderSpamPenalty", "startOrderSpamPenalty",
        "punishSelectedLocalControllers", "isAssassinSupportUnit", "shouldRestoreSupportOrderAfterExternalOrder",
        "restoreExpectedOrderAfterExternalOrder", "KL", "kJ",
        "completeRoundStart", "startIdleAttackTimer", "isIdleAttackUnit", "code__TimerStart_IdleAttackRuntime",
        "ForGroupCallback_forUnitsInRect_IdleAttackRuntime_callback_forUnitsInRect_IdleAttackRuntime",
        "uL", "CE", "EventListener_add_Blink_onEvent_add_Blink",
        "gL", "OnPointCast_onPointCast_RescueStrikeRuntime_fireEx_onPointCast_RescueStrikeRuntime",
        "CallbackSingle_doAfter_RescueStrikeRuntime_call_doAfter_RescueStrikeRuntime1",
        "applyRescueStrike", "startRescueStrikeCooldown", "startTeamRescueStrikeCooldown", "finishRescueStrike",
        "holyShrineSpell", "handleSourceDamageEffects", "cleaveHitsTarget",
        "ForGroupCallback_forUnitsInRange_DamageRuntime_callback_forUnitsInRange_DamageRuntime",
        "DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire",
        "CallbackSingle_doAfter_addListener_doAfter_ThunderpawSpire_call_doAfter_addListener_doAfter_ThunderpawSpire2",
        "CallbackSingle_doAfter_doAfter_addListener_doAfter_ThunderpawSpire_call_doAfter_doAfter_addListener_doAfter_ThunderpawSpire",
        "createVision",
        "YF", "isCastleProtected", "completeRoundStart", "yd",
        "DamageListener_addListener_CastleProtection_onEvent_addListener_CastleProtection",
        "nK", "DamageListener_addListener_RaceCorrupted_onEvent_addListener_RaceCorrupted",
        "TI", "startObeliskOfLight", "stopObeliskOfLight",
        "CallbackSingle_doAfter_ObeliskOfLight_call_doAfter_ObeliskOfLight",
        "EventListener_add_doAfter_ObeliskOfLight_onEvent_add_doAfter_ObeliskOfLight",
        "EventListener_add_doAfter_ObeliskOfLight_onEvent_add_doAfter_ObeliskOfLight1",
        "code__onLeave_doAfter_ObeliskOfLight",
        "DamageListener_addListener_doAfter_ObeliskOfLight_onEvent_addListener_doAfter_ObeliskOfLight",
        "EE", "rollBody", "randomizeBloodFiend", "onUnitTrained",
    }
    if not required.issubset(available):
        return []

    def source(name: str) -> tuple[int, bytes, set[str]]:
        body = _function_body_tokens(data, functions, name)
        if body is None:
            raise ValueError(f"runtime-system source function missing: {name}")
        start, tokens = body
        function = next(function for function in functions if function["name"] == name)
        raw = data[int(function["start"]):int(function["end"])]
        return start, raw, {token.text for token in tokens}

    rows: list[dict[str, object]] = []

    # Power Plant / Power Surge. The native auras supply building armor, mana
    # regeneration and tower damage. A four-second scripted sweep removes the
    # two freeze buffs in a wider 252 radius, and B01K on a production building
    # causes spawned units to receive the hidden HP/armor/damage package plus a
    # 15% spell-resistance ability unless they already own one of the exclusions.
    init_start, init_source, init_tokens = source("dK")
    register_start, register_source, register_tokens = source("registerPowerPlant")
    sweep_start, sweep_source, sweep_tokens = source(
        "ForGroupCallback_forUnitsInRange_PowerPlantRuntime_callback_forUnitsInRange_PowerPlantRuntime"
    )
    apply_start, apply_source, apply_tokens = source("applyPowerArmor")
    exclusion_start, exclusion_source, exclusion_tokens = source("hasNoPowerArmorExclusion")
    setup_start, setup_source, setup_tokens = source("setupUnit")
    if not {"1747990868", "252.", "4.", "__wurst_safe_TimerStart"}.issubset(init_tokens):
        raise ValueError("Power Plant runtime initialization changed")
    if b"O1=1747990868 N1=252." not in init_source or b"TimerStart(qir,4.,true" not in init_source:
        raise ValueError("Power Plant radius/timer constants changed")
    if not {"1093679443", "1093679440", "1093679446", "addProtectedAbility"}.issubset(register_tokens):
        raise ValueError("Power Plant aura registration changed")
    for fragment in (
        b"addProtectedAbility(uir,1093679443)",
        b"addProtectedAbility(vir,1093679440)",
        b"addProtectedAbility(wir,1093679446)",
    ):
        if fragment not in register_source:
            raise ValueError("Power Plant granted aura set changed")
    if not {"1114010234", "1110454349", "UNIT_TYPE_STRUCTURE", "unit_removeAbility"}.issubset(sweep_tokens):
        raise ValueError("Power Plant disable-cleanse sweep changed")
    if b"unit_removeAbility(Ajn,1114010234)" not in sweep_source or b"unit_removeAbility(Ajn,1110454349)" not in sweep_source:
        raise ValueError("Power Plant freeze-buff cleanup changed")
    if not {"1093679437", "4", "1093679441", "1093679442", "1093683286"}.issubset(apply_tokens):
        raise ValueError("Power Armor spawn package changed")
    required_apply = (
        b"addProtectedAbility(Ifs,1093679437)",
        b"SetUnitAbilityLevel(Jfs,1093679437,4)",
        b"unit_removeAbility(Hfs,1093679437)",
        b"addProtectedAbility(Kfs,1093679441)",
        b"addProtectedAbility(Lfs,1093679442)",
        b"addProtectedAbility(Mfs,1093683286)",
    )
    if any(fragment not in apply_source for fragment in required_apply):
        raise ValueError("Power Armor spawn package sequence changed")
    exclusion_ids = [
        1093679179, 1093679183, 1093679411, 1093681480, 1093681481, 1093681484,
        1093681486, 1093681497, 1093681498, 1093681750, 1093681990, 1093682760,
        1093682761, 1093682762, 1093682766, 1093683030, 1093683248,
    ]
    if not {str(value) for value in exclusion_ids}.issubset(exclusion_tokens):
        raise ValueError("Power Armor spell-resistance exclusion set changed")
    if b"unit_getAbilityLevel(Ofs,1110454603)>0" not in setup_source:
        raise ValueError("Power Armor production-building buff gate changed")
    rows.append({
        "system_id": "power-plant-power-surge",
        "mechanic_kind": "area-building-buffs-cleanse-and-spawn-augmentation",
        "trigger": "construction-plus-periodic-sweep-plus-unit-spawn",
        "parameters": {
            "power_plant_unit_id": 1747990868,
            "building_armor_aura_ability_id": 1093679443,
            "building_armor_buff_id": 1110454603,
            "building_armor_bonus": 2,
            "building_aura_radius": 180,
            "mana_regen_aura_ability_id": 1093679440,
            "mana_regen_bonus_per_second": 0.20,
            "tower_damage_aura_ability_id": 1093679446,
            "tower_attack_damage_bonus_fraction": 0.25,
            "disable_cleanse_sweep_interval_seconds": 4,
            "disable_cleanse_radius": 252,
            "disable_cleanse_requires_structure": True,
            "disable_cleanse_excludes_power_plant_itself": True,
            "disable_cleanse_requires_allied_structure": False,
            "cleaned_buff_ids": [1114010234, 1110454349],
            "spawn_requires_building_power_armor_buff_id": 1110454603,
            "spawn_hp_stack_ability_id": 1093679437,
            "spawn_hp_stack_temporary_level": 4,
            "spawn_permanent_max_hp_bonus": 150,
            "spawn_armor_bonus_ability_id": 1093679441,
            "spawn_armor_bonus": 4,
            "spawn_damage_bonus_ability_id": 1093679442,
            "spawn_attack_damage_bonus_fraction": 0.20,
            "spawn_spell_resist_ability_id": 1093683286,
            "spawn_spell_damage_reduction": 0.15,
            "spell_resist_exclusion_ability_ids": exclusion_ids,
        },
        "related_rawcode_ids": [
            1747990868, 1093679443, 1110454603, 1093679440, 1093679446,
            1114010234, 1110454349, 1093679437, 1093679441, 1093679442, 1093683286,
            *exclusion_ids,
        ],
        "source_functions": [
            "dK", "registerPowerPlant",
            "ForGroupCallback_forEachIn_PowerPlantRuntime_callback_forEachIn_PowerPlantRuntime",
            "ForGroupCallback_forUnitsInRange_PowerPlantRuntime_callback_forUnitsInRange_PowerPlantRuntime",
            "setupUnit", "applyPowerArmor", "hasNoPowerArmorExclusion",
        ],
        "evidence_kind": "exact-native-aura-object-data-and-cross-runtime-script",
        "byte_offset": min(init_start, register_start, sweep_start, apply_start, exclusion_start, setup_start),
    })

    # Heroic Shrine / Companion Spawning. The real runtime is 17% per shrine,
    # not the 16% shown in the 9.27 tooltip. Up to two team shrines roll
    # independently. TypeMurloc doubles each successful shrine clone and also
    # receives one unconditional same-owner copy on the original train event;
    # the hidden Twins carrier instead becomes n02K+n02L.
    companion_init_start, companion_init_source, companion_init_tokens = source("kP")
    shrine_start, shrine_source, shrine_tokens = source("acquireHeroicShrine")
    synced_start, synced_source, synced_tokens = source("spawnSyncedCompanions")
    train_companion_start, train_companion_source, train_companion_tokens = source(
        "EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning"
    )
    if not {"2", "17", "EVENT_PLAYER_UNIT_TRAIN_FINISH"}.issubset(companion_init_tokens):
        raise ValueError("Heroic Shrine companion constants changed")
    if b"fR=2 eR=17" not in companion_init_source:
        raise ValueError("Heroic Shrine max-count/chance constants changed")
    if not {"vGb", "unit_getIndex", "unit_getOwner", "__wurst_safe_GroupAddUnit"}.issubset(shrine_tokens):
        raise ValueError("Heroic Shrine team registration changed")
    if not {"GetRandomInt", "1093679432", "setupUnit", "__wurst_safe_CreateUnit"}.issubset(synced_tokens):
        raise ValueError("Heroic Shrine synced companion spawning changed")
    if b"min(group_size(vGb[Xfs]),fR)" not in synced_source or b"GetRandomInt(0,99)<eR" not in synced_source:
        raise ValueError("Heroic Shrine roll loop changed")
    if b"unit_getAbilityLevel(Vfs,1093679432)>0" not in synced_source:
        raise ValueError("Heroic Shrine TypeMurloc double-clone rule changed")
    if not {"1093679432", "1093682767", "1848652363", "1848652364", "setupUnit"}.issubset(train_companion_tokens):
        raise ValueError("Companion train-event transformations changed")
    if b"unit_getAbilityLevel(eYl,1093679432)>0" not in train_companion_source:
        raise ValueError("TypeMurloc unconditional companion branch changed")
    if b"unit_getAbilityLevel(eYl,1093682767)>0" not in train_companion_source:
        raise ValueError("Twins conversion branch changed")
    rows.append({
        "system_id": "heroic-shrine-companion-spawning",
        "mechanic_kind": "team-shrine-independent-clone-rolls-and-train-transformations",
        "trigger": "unit-train-finish",
        "parameters": {
            "heroic_shrine_unit_id": 1747989831,
            "maximum_shrines_checked": 2,
            "per_shrine_roll_min": 0,
            "per_shrine_roll_max": 99,
            "per_shrine_success_threshold_exclusive": 17,
            "per_shrine_actual_probability_percent": 17,
            "tooltip_probability_percent_per_shrine": 16,
            "tooltip_disagrees_with_runtime": True,
            "successful_clone_unit_type": "same-as-original-trained-unit",
            "successful_clone_spawn_position": "original-trained-unit-position",
            "successful_clone_owner": "heroic-shrine-owner",
            "successful_clone_runs_setup_unit": True,
            "type_murloc_marker_ability_id": 1093679432,
            "type_murloc_extra_clone_per_successful_shrine_roll": 1,
            "type_murloc_unconditional_local_extra_copy": 1,
            "twins_marker_ability_id": 1093682767,
            "twins_carrier_unit_id": 1697656901,
            "twins_replacement_unit_ids": [1848652363, 1848652364],
            "twins_replacements_run_setup_unit": True,
        },
        "related_rawcode_ids": [
            1747989831, 1093679432, 1093682767, 1697656901, 1848652363, 1848652364,
        ],
        "source_functions": [
            "kP", "acquireHeroicShrine", "spawnSyncedCompanions",
            "EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning", "setupUnit",
        ],
        "evidence_kind": "exact-team-registration-roll-loop-and-train-event-transformations",
        "byte_offset": min(companion_init_start, shrine_start, synced_start, train_companion_start),
    })

    # Golden Shrine of Justice. Each team shrine contributes 20 percentage
    # points, capped at 40. Eligible non-legendary/non-summoned deaths roll once;
    # success is delayed two seconds and recreates the same type at the death
    # position only if the death-generation token has not changed. The new unit
    # is permanently blocked from receiving the shrine revive a second time.
    lifecycle_start, lifecycle_source, lifecycle_tokens = source("ME")
    elven_start, elven_source, elven_tokens = source("acquireElvenShrine")
    revive_chance_start, revive_chance_source, revive_chance_tokens = source("elvenShrineEffectiveReviveChance")
    death_start, death_source, death_tokens = source("fJ")
    revive_start, revive_source, revive_tokens = source(
        "CallbackSingle_doAfter_OnUnitDeathHandler_call_doAfter_OnUnitDeathHandler"
    )
    block_start, block_source, block_tokens = source("markShrineReviveBlocked")
    if not {"20", "40"}.issubset(lifecycle_tokens) or b"Ctb=20 Btb=40" not in lifecycle_source:
        raise ValueError("Golden Shrine revive chance constants changed")
    if not {"utb", "Ctb"}.issubset(elven_tokens) or b"utb[gDp]=(__wurst_ensureInt(utb[gDp])+Ctb)" not in elven_source:
        raise ValueError("Golden Shrine team chance accumulation changed")
    if b"min(__wurst_ensureInt(utb[SBp]),Btb)" not in revive_chance_source:
        raise ValueError("Golden Shrine chance cap changed")
    required_death_tokens = {
        "1093678919", "1093678920", "UNIT_TYPE_SUMMONED", "2.", "_Ir",
        "elvenShrineEffectiveReviveChance", "doAfter",
    }
    if not required_death_tokens.issubset(death_tokens):
        raise ValueError("Golden Shrine death eligibility/roll branch changed")
    if b"unit_getAbilityLevel(M2q,1093678919)<=0" not in death_source or b"unit_getAbilityLevel(M2q,1093678920)<=0" not in death_source:
        raise ValueError("Golden Shrine summoned/legendary exclusions changed")
    if b"doAfter(2.,f3q)" not in death_source:
        raise ValueError("Golden Shrine revive delay changed")
    if not {"__wurst_safe_CreateUnit", "__wurst_safe_RemoveUnit", "markShrineReviveBlocked"}.issubset(revive_tokens):
        raise ValueError("Golden Shrine delayed revive callback changed")
    if b"if(E8m.deathIndex==A7)then" not in revive_source:
        raise ValueError("Golden Shrine death-generation guard changed")
    if not {"z7", "true"}.issubset(block_tokens):
        raise ValueError("Golden Shrine one-revive block changed")
    rows.append({
        "system_id": "golden-shrine-revival",
        "mechanic_kind": "team-stacked-one-time-delayed-unit-revival",
        "trigger": "combat-sapper-death",
        "parameters": {
            "golden_shrine_unit_id": 1747989817,
            "chance_percent_per_shrine": 20,
            "maximum_effective_chance_percent": 40,
            "chance_roll_min": 0,
            "chance_roll_max": 99,
            "exclude_summoned_unit_marker_ability_id": 1093678919,
            "exclude_legendary_marker_ability_id": 1093678920,
            "exclude_wc3_summoned_unit_type": True,
            "exclude_already_revived_units": True,
            "scripted_death_suppression_is_one_shot": True,
            "requires_unit_actually_dead": True,
            "revive_delay_seconds": 2,
            "revive_requires_same_death_generation": True,
            "revived_unit_type": "same-as-dead-unit",
            "revived_unit_owner": "original-owner",
            "revived_unit_position": "exact-death-position",
            "remove_original_dead_unit_on_success": True,
            "revived_unit_cannot_trigger_golden_shrine_again": True,
        },
        "related_rawcode_ids": [1747989817, 1093678919, 1093678920],
        "source_functions": [
            "ME", "acquireElvenShrine", "elvenShrineEffectiveReviveChance", "fJ",
            "CallbackSingle_doAfter_OnUnitDeathHandler_call_doAfter_OnUnitDeathHandler",
            "markShrineReviveBlocked",
        ],
        "evidence_kind": "exact-team-counter-death-branch-and-delayed-replacement-callback",
        "byte_offset": min(lifecycle_start, elven_start, revive_chance_start, death_start, revive_start, block_start),
    })

    # Elemental Linker. One or more team Linkers merely enable the effect; the
    # heal amount is based on the dead Elemental owner's own production-building
    # count. The runtime uses 17 hp/building (not tooltip 17.5), caps at 500,
    # heals same-type allied units in 350 range for the full amount and all other
    # allied combat sappers for only 20% (not the tooltip's stated 30%).
    finish_start, finish_source, finish_tokens = source("onBuildingFinished")
    linker_start, linker_source, linker_tokens = source("acquireLinker")
    heal_start, heal_source, heal_tokens = source("eleLinkerHealAmount")
    count_start, count_source, count_tokens = source("changeEleBuildingCount")
    side_effect_start, side_effect_source, side_effect_tokens = source("wL")
    if b"Ktb=100 Jtb=.024 Itb=256. Htb=6 Gtb=5 Ftb=17 Etb=500 Dtb=5 Ctb=20 Btb=40" not in lifecycle_source:
        raise ValueError("Elemental Linker lifecycle constants changed")
    if b"YDp==1747990067)then acquireLinker(VDp,XDp)" not in finish_source:
        raise ValueError("Elemental Linker lifecycle dispatch changed")
    if not {"Atb", "ztb", "CreateTextTag"}.issubset(linker_tokens):
        raise ValueError("Elemental Linker acquisition bookkeeping changed")
    if b"Atb[OCp]=(__wurst_ensureInt(Atb[OCp])+1)" not in linker_source:
        raise ValueError("Elemental Linker team presence count changed")
    if b"return min((Ftb*eleBuildingTotal(bCp)),Etb)" not in heal_source:
        raise ValueError("Elemental Linker heal amount formula changed")
    if not {"eleBuildingTotal", "wtb", "mtb", "max1"}.issubset(count_tokens):
        raise ValueError("Elemental production-building count maintenance changed")
    if b"unit_getAbilityLevel(M2q,1093682741)>0" not in death_source:
        raise ValueError("Elemental Linker dead-unit marker gate changed")
    if b"(__wurst_ensureInt(Atb[R2q])>0)" not in death_source:
        raise ValueError("Elemental Linker team-presence gate changed")
    if b"TX=eleLinkerHealAmount(P2q)" not in death_source or b"350.,C3q" not in death_source:
        raise ValueError("Elemental Linker death heal amount/radius changed")
    if not {"isAliveCombatSapper", "unit_isAllyOf", "UX", "TX", "widget_getLife"}.issubset(side_effect_tokens):
        raise ValueError("Elemental Linker heal target callback changed")
    if b"unit_getTypeId(jHr)==UX" not in side_effect_source or b"(.2*TX)" not in side_effect_source:
        raise ValueError("Elemental Linker same/other-type heal factors changed")
    rows.append({
        "system_id": "elemental-linker-death-heal",
        "mechanic_kind": "team-presence-gated-owner-scaled-elemental-death-heal",
        "trigger": "elemental-combat-sapper-death",
        "parameters": {
            "linker_unit_id": 1747990067,
            "elemental_unit_marker_ability_id": 1093682741,
            "team_linker_presence_required": True,
            "additional_linkers_do_not_stack_heal": True,
            "elemental_building_bucket_count": 5,
            "heal_per_owned_elemental_production_building": 17,
            "tooltip_heal_per_building": 17.5,
            "heal_per_building_tooltip_disagrees_with_runtime": True,
            "maximum_heal": 500,
            "heal_radius": 350,
            "target_predicate": "alive-combat-sapper;ally-of-dead-unit-owner",
            "same_unit_type_heal_factor": 1.0,
            "other_unit_type_heal_factor": 0.2,
            "tooltip_other_race_reduction_percent": 70,
            "runtime_other_type_reduction_percent": 80,
            "other_type_tooltip_disagrees_with_runtime": True,
            "building_count_scope": "dead-unit-owner",
            "linker_presence_scope": "team",
            "owner_change_updates_counts": True,
            "building_death_updates_counts": True,
        },
        "related_rawcode_ids": [1747990067, 1093682741],
        "source_functions": [
            "ME", "acquireLinker", "acquireEleBuilding", "changeEleBuildingCount", "eleLinkerHealAmount",
            "fJ", "wL", "migrateBuildingLifecycleOwner", "NE", "OE",
        ],
        "evidence_kind": "exact-lifecycle-counters-death-branch-and-resolved-filter-side-effect",
        "byte_offset": min(lifecycle_start, finish_start, linker_start, heal_start, count_start, death_start, side_effect_start),
    })

    # Treasure Box income multiplier. Construction/death maintain a per-player
    # count. Counts 1..9 use a precomputed cumulative table; count 10+ switches
    # to the explicit linear tail in calcTreasureBoxMultiplier. The multiplier
    # is applied to base income before the ordinary progressive income tax.
    treasure_init_start, treasure_init_source, treasure_init_tokens = source("GH")
    multiplier_start, multiplier_source, multiplier_tokens = source("calcTreasureBoxMultiplier")
    income_start, income_source, income_tokens = source("rawIncomeWithTreasureBox")
    treasure_acquire_start, treasure_acquire_source, treasure_acquire_tokens = source("acquireTreasureBox")
    treasure_remove_start, treasure_remove_source, treasure_remove_tokens = source("SE")
    treasure_mode_start, treasure_mode_source, treasure_mode_tokens = source("applyTreasureBoxModeEnabled")
    treasure_table = [0.0, 1.0, 1.85, 2.57, 3.18, 3.7, 4.14, 4.52, 4.84, 5.11]
    expected_table_fragment = (
        b"Fab[0]=0. Fab[1]=1. Fab[2]=1.85 Fab[3]=2.57 Fab[4]=3.18 Fab[5]=3.7 "
        b"Fab[6]=4.14 Fab[7]=4.52 Fab[8]=4.84 Fab[9]=5.11"
    )
    if expected_table_fragment not in treasure_init_source:
        raise ValueError("Treasure Box cumulative multiplier table changed")
    if b"if(XEq<=0)then return 1." not in multiplier_source:
        raise ValueError("Treasure Box zero-count multiplier changed")
    if b"if(XEq<10)then return((__wurst_ensureReal(Fab[XEq])*0.25)+1.)" not in multiplier_source:
        raise ValueError("Treasure Box table multiplier formula changed")
    if b"return((((int_toReal((XEq-9))*0.25)+5.11)*0.25)+1.)" not in multiplier_source:
        raise ValueError("Treasure Box count>=10 tail formula changed")
    if b"iGb[YEq])*10.)*calcTreasureBoxMultiplier" not in income_source:
        raise ValueError("Treasure Box raw-income application changed")
    if b"YDp==1747988536)then acquireTreasureBox(VDp)" not in finish_source:
        raise ValueError("Treasure Box lifecycle dispatch changed")
    if not {"qGb", "unit_getOwner"}.issubset(treasure_acquire_tokens):
        raise ValueError("Treasure Box lifecycle acquisition changed")
    if b"qGb[uDp]=(__wurst_ensureInt(qGb[uDp])+1)" not in treasure_acquire_source:
        raise ValueError("Treasure Box count increment changed")
    if b"qGb[xDp]=max1(0,(__wurst_ensureInt(qGb[xDp])-1))" not in treasure_remove_source:
        raise ValueError("Treasure Box count decrement changed")
    if not {"1747988536", "__wurst_safe_SetPlayerUnitAvailableBJ"}.issubset(treasure_mode_tokens):
        raise ValueError("No-Treasure-Box mode availability gate changed")
    if b"zX=(not QQq)" not in treasure_mode_source:
        raise ValueError("Treasure Box mode flag changed")
    rows.append({
        "system_id": "treasure-box-income-multiplier",
        "mechanic_kind": "per-player-building-count-income-multiplier",
        "trigger": "income-calculation-with-building-lifecycle-count",
        "parameters": {
            "treasure_box_unit_id": 1747988536,
            "count_scope": "player",
            "base_income_units_scale": 10,
            "multiplier_table_indexes_0_through_9": treasure_table,
            "zero_count_multiplier": 1.0,
            "counts_1_through_9_formula": "1 + 0.25 * table[count]",
            "count_10_plus_formula": "1 + 0.25 * (5.11 + 0.25 * (count - 9))",
            "count_10_plus_marginal_multiplier_per_box": 0.0625,
            "multipliers_0_through_9": [
                1.0 if count == 0 else 1.0 + (0.25 * treasure_table[count])
                for count in range(10)
            ],
            "applied_before_progressive_income_tax": True,
            "tooltip_first_box_bonus_percent": 25,
            "tooltip_later_box_reduction_percent": 15,
            "runtime_uses_precomputed_table_then_linear_tail": True,
            "no_treasure_box_mode_disables_building_for_player_slots_0_through_11": True,
            "building_death_decrements_count": True,
            "owner_change_migrates_count": True,
        },
        "related_rawcode_ids": [1747988536],
        "source_functions": [
            "GH", "calcTreasureBoxMultiplier", "rawIncomeWithTreasureBox", "acquireTreasureBox", "SE",
            "migrateBuildingLifecycleOwner", "applyTreasureBoxModeEnabled",
        ],
        "evidence_kind": "exact-lifecycle-count-precomputed-table-and-income-formula",
        "byte_offset": min(
            treasure_init_start, multiplier_start, income_start, treasure_acquire_start,
            treasure_remove_start, treasure_mode_start,
        ),
    })

    # Human Artillery. The h001 building is registered on construction and
    # forced to issue attack-ground at a random point inside the enemy castle
    # rectangle. A 9-second maintenance timer reissues the order to every live
    # registered Artillery, while any externally issued smart order is
    # immediately converted back into the same random attack-ground order.
    artillery_init_start, artillery_init_source, artillery_init_tokens = source("AH")
    artillery_register_start, artillery_register_source, artillery_register_tokens = source("registerHumanArtillery")
    artillery_tick_start, artillery_tick_source, artillery_tick_tokens = source(
        "ForGroupCallback_forEachIn_HumanArtilleryRuntime_callback_forEachIn_HumanArtilleryRuntime"
    )
    artillery_target_start, artillery_target_source, artillery_target_tokens = source(
        "EventListener_add_HumanArtilleryRuntime_onEvent_add_HumanArtilleryRuntime1"
    )
    artillery_point_start, artillery_point_source, artillery_point_tokens = source(
        "EventListener_add_HumanArtilleryRuntime_onEvent_add_HumanArtilleryRuntime2"
    )
    if b"ncb=1747988529 mcb=851971 lcb=_Dr()" not in artillery_init_source:
        raise ValueError("Human Artillery unit/smart-order constants changed")
    if b"TimerStart(aEq,9.,true" not in artillery_init_source:
        raise ValueError("Human Artillery maintenance interval changed")
    if not {"EVENT_PLAYER_UNIT_CONSTRUCT_FINISH", "EVENT_PLAYER_UNIT_ISSUED_TARGET_ORDER", "EVENT_PLAYER_UNIT_ISSUED_POINT_ORDER"}.issubset(artillery_init_tokens):
        raise ValueError("Human Artillery event registration changed")
    if not {"enemyCastleRect", "851984", "GetRandomReal", "group_add"}.issubset(artillery_register_tokens):
        raise ValueError("Human Artillery initial bombardment registration changed")
    if b"unit_issuePointOrderById(gEq,851984" not in artillery_register_source:
        raise ValueError("Human Artillery initial attack-ground order changed")
    if not {"851984", "enemyCastleRect", "GetRandomReal"}.issubset(artillery_tick_tokens):
        raise ValueError("Human Artillery maintenance bombardment changed")
    for event_source in (artillery_target_source, artillery_point_source):
        if b"unit_getTypeId" not in event_source or b"GetIssuedOrderId()==mcb" not in event_source:
            raise ValueError("Human Artillery smart-order interception changed")
        if b"unit_issuePointOrderById" not in event_source or b"851984" not in event_source:
            raise ValueError("Human Artillery smart-order redirect changed")
    rows.append({
        "system_id": "human-artillery-auto-bombardment",
        "mechanic_kind": "building-random-enemy-base-attack-ground-controller",
        "trigger": "construction-plus-periodic-order-maintenance-plus-smart-order-intercept",
        "parameters": {
            "artillery_unit_id": 1747988529,
            "maintenance_interval_seconds": 9,
            "intercepted_order_id": 851971,
            "intercepted_order_name": "smart",
            "forced_order_id": 851984,
            "forced_order_name": "attackground",
            "target_region": "enemy-castle-rect-by-team",
            "target_point_distribution": "uniform-random-x-and-y-within-enemy-castle-rect",
            "initial_order_on_construction": True,
            "periodically_reissues_for_live_registered_buildings": True,
            "smart_point_or_target_orders_are_redirected": True,
        },
        "related_rawcode_ids": [1747988529, 1093677643],
        "source_functions": [
            "AH", "registerHumanArtillery",
            "ForGroupCallback_forEachIn_HumanArtilleryRuntime_callback_forEachIn_HumanArtilleryRuntime",
            "EventListener_add_HumanArtilleryRuntime_onEvent_add_HumanArtilleryRuntime1",
            "EventListener_add_HumanArtilleryRuntime_onEvent_add_HumanArtilleryRuntime2",
            "enemyCastleRect",
        ],
        "evidence_kind": "exact-construction-registration-periodic-order-and-order-intercept",
        "byte_offset": min(
            artillery_init_start, artillery_register_start, artillery_tick_start,
            artillery_target_start, artillery_point_start,
        ),
    })

    # Chaos Portal / Raise Dead summon replacement. Warcraft Raise Dead creates
    # carrier units u004/u008/u00E; the map immediately replaces them with a
    # random skeleton from an eight-entry table and orders the result to attack.
    # Lich King's u00E carrier draws only Greater results (or the General) and
    # receives the explicit +12 attack / +3 armor ability pair.
    chaos_init_start, chaos_init_source, chaos_init_tokens = source("cG")
    chaos_replace_start, chaos_replace_source, chaos_replace_tokens = source("replaceChaosPortalSummon")
    chaos_summon_start, chaos_summon_source, chaos_summon_tokens = source("onChaosPortalUnitSummoned")
    chaos_table = [1966092338, 1848651844, 1966092353, 1848651843, 1966092354, 1848652108, 1966092339, 1848652109]
    expected_chaos_table = (
        b"ajb[0]=1966092338 ajb[1]=1848651844 ajb[2]=1966092353 ajb[3]=1848651843 "
        b"ajb[4]=1966092354 ajb[5]=1848652108 ajb[6]=1966092339 ajb[7]=1848652109"
    )
    if expected_chaos_table not in chaos_init_source or "EVENT_PLAYER_UNIT_SUMMON" not in chaos_init_tokens:
        raise ValueError("Chaos Portal skeleton replacement table/registration changed")
    if not {"__wurst_safe_ReplaceUnitBJ", "orderCodeAttack"}.issubset(chaos_replace_tokens):
        raise ValueError("Chaos Portal replacement primitive changed")
    if b"__wurst_safe_ReplaceUnitBJ(yeq,zeq,2)" not in chaos_replace_source:
        raise ValueError("Chaos Portal replacement method changed")
    for carrier in (1966092340, 1966092344, 1966092357):
        if str(carrier) not in chaos_summon_tokens:
            raise ValueError(f"Chaos Portal carrier missing from summon handler: {carrier}")
    if b"ajb[GetRandomInt(0,2)]" not in chaos_summon_source:
        raise ValueError("Necromancer skeleton selection range changed")
    if b"ajb[GetRandomInt(0,6)]" not in chaos_summon_source:
        raise ValueError("Mighty Necromancer skeleton selection range changed")
    if b"Deq=GetRandomInt(0,7)if(Deq==0)then Deq=7 else Deq=GetRandomInt(3,6)end" not in chaos_summon_source:
        raise ValueError("Lich King skeleton selection distribution changed")
    if b"addProtectedAbility(Feq,1093679160)" not in chaos_summon_source or b"addProtectedAbility(Geq,1093679161)" not in chaos_summon_source:
        raise ValueError("Lich King summoned-skeleton bonus abilities changed")
    rows.append({
        "system_id": "raise-dead-skeleton-randomization",
        "mechanic_kind": "summoned-carrier-random-unit-replacement",
        "trigger": "player-unit-summon",
        "parameters": {
            "replacement_method": 2,
            "replacement_orders_attack": True,
            "skeleton_table_unit_ids": chaos_table,
            "necromancer_carrier_unit_id": 1966092340,
            "necromancer_table_indexes": [0, 1, 2],
            "necromancer_distribution": "uniform",
            "mighty_necromancer_carrier_unit_id": 1966092344,
            "mighty_necromancer_table_indexes": [0, 1, 2, 3, 4, 5, 6],
            "mighty_necromancer_distribution": "uniform",
            "lich_king_carrier_unit_id": 1966092357,
            "lich_king_general_table_index": 7,
            "lich_king_general_probability_percent": 12.5,
            "lich_king_other_table_indexes": [3, 4, 5, 6],
            "lich_king_each_other_probability_percent": 21.875,
            "lich_king_bonus_damage_ability_id": 1093679160,
            "lich_king_bonus_armor_ability_id": 1093679161,
        },
        "related_rawcode_ids": [
            *chaos_table, 1966092340, 1966092344, 1966092357, 1093679160, 1093679161,
        ],
        "source_functions": ["cG", "replaceChaosPortalSummon", "onChaosPortalUnitSummoned"],
        "evidence_kind": "exact-summon-carriers-replacement-table-random-branches-and-bonus-abilities",
        "byte_offset": min(chaos_init_start, chaos_replace_start, chaos_summon_start),
    })

    # Gjallarhorn building-count scaling. The ordinary building-spell row owns
    # the cast itself, but its effect level reads a separate team-scoped counter
    # incremented by each constructed h010. The counter is reset by the watched
    # round signal and the cast clamps the resulting level to four.
    gjallar_init_start, gjallar_init_source, gjallar_init_tokens = source("mH")
    gjallar_construct_start, gjallar_construct_source, gjallar_construct_tokens = source(
        "EventListener_add_GjallarHorn_onEvent_add_GjallarHorn"
    )
    gjallar_reset_start, gjallar_reset_source, gjallar_reset_tokens = source(
        "Action_watch_GjallarHorn_run_watch_GjallarHorn"
    )
    gjallar_cast_start, gjallar_cast_source, gjallar_cast_tokens = source(
        "BuildingSpellClosure_registerBuildingSpell_GjallarHorn_cast_registerBuildingSpell_GjallarHorn"
    )
    if not {"1747988784", "1093677387", "EVENT_PLAYER_UNIT_CONSTRUCT_FINISH"}.issubset(gjallar_init_tokens):
        raise ValueError("Gjallarhorn spell/lifecycle registration changed")
    if b"unit_getTypeId(nym)==1747988784" not in gjallar_construct_source or b"Icb[pym]=(__wurst_ensureInt(Icb[pym])+1)" not in gjallar_construct_source:
        raise ValueError("Gjallarhorn team counter increment changed")
    if b"while true do if(tym>3)then break end Icb[tym]=0" not in gjallar_reset_source:
        raise ValueError("Gjallarhorn round-reset counter clearing changed")
    if b"Wxm=min(4,__wurst_ensureInt(Icb[__wurst_ensureInt(lGb[player_getId(Vxm)])]))" not in gjallar_cast_source:
        raise ValueError("Gjallarhorn cast-level counter formula changed")
    rows.append({
        "system_id": "gjallarhorn-team-count-scaling",
        "mechanic_kind": "team-constructed-building-count-to-spell-level",
        "trigger": "building-construction-and-round-reset-consumed-by-spell-cast",
        "parameters": {
            "gjallarhorn_unit_id": 1747988784,
            "gjallarhorn_trigger_ability_id": 1093677387,
            "counter_scope": "team",
            "increment_per_constructed_gjallarhorn": 1,
            "effect_level_formula": "min(4, team_constructed_gjallarhorn_count)",
            "maximum_effect_level": 4,
            "counter_resets_on_round_signal": True,
            "counter_decrement_on_building_death": False,
            "building_spell_row_owns_cast_delivery": True,
        },
        "related_rawcode_ids": [1747988784, 1093677387, 1093677366],
        "source_functions": [
            "mH", "EventListener_add_GjallarHorn_onEvent_add_GjallarHorn",
            "Action_watch_GjallarHorn_run_watch_GjallarHorn",
            "BuildingSpellClosure_registerBuildingSpell_GjallarHorn_cast_registerBuildingSpell_GjallarHorn",
        ],
        "evidence_kind": "exact-construct-counter-round-reset-and-spell-level-consumer",
        "byte_offset": min(gjallar_init_start, gjallar_construct_start, gjallar_reset_start, gjallar_cast_start),
    })

    # Shared support-order controller for Assassin, Royal Assassin and Gobbo.
    assassin_init_start, assassin_init_source, assassin_init_tokens = source("lE")
    support_init_start, support_init_source, support_init_tokens = source("nH")
    support_gate_start, support_gate_source, support_gate_tokens = source("isSupportOrderUnit")
    support_dispatch_start, support_dispatch_source, support_dispatch_tokens = source("orderAssassinOrGoboW")
    support_state_start, support_state_source, support_state_tokens = source("shouldRunPeriodicSupportOrderForState")
    assassin_targets_start, assassin_targets_source, assassin_targets_tokens = source("prepareAssassinTargets")
    assassin_valid_start, assassin_valid_source, assassin_valid_tokens = source("isValidAssassinTarget")
    assassin_order_start, assassin_order_source, assassin_order_tokens = source("orderAssassinW")
    assassin_restore_start, assassin_restore_source, assassin_restore_tokens = source("restoreAssassinTargetOrderW")
    gobbo_targets_start, gobbo_targets_source, gobbo_targets_tokens = source("prepareGobboTargets")
    gobbo_repair_start, gobbo_repair_source, gobbo_repair_tokens = source("isGobboRepairableTarget")
    gobbo_rally_start, gobbo_rally_source, gobbo_rally_tokens = source("isGobboRallyTarget")
    gobbo_order_start, gobbo_order_source, gobbo_order_tokens = source("orderGoboW")
    gobbo_spawner_start, gobbo_spawner_source, gobbo_spawner_tokens = source("setGobboSpawner")
    task_run_start, task_run_source, task_run_tokens = source("SupportOrderTask_SupportOrderTask_run")
    ensure_task_start, ensure_task_source, ensure_task_tokens = source("ensureSupportOrderTask")
    gobbo_life_init_start, gobbo_life_init_source, gobbo_life_init_tokens = source("sH")
    gobbo_life_start, gobbo_life_source, gobbo_life_tokens = source("applyGobboTimedLife")
    train_start, train_source, train_tokens = source("onUnitTrained")
    summon_start, summon_source, summon_tokens = source("onSummonedUnit")

    if b"YAb=1848652122 XAb=1848652336 WAb=0.10 OAb=851983" not in assassin_init_source:
        raise ValueError("Assassin support-order constants changed")
    if b"KAb=" not in assassin_init_source or b"(-6016.),(-4096.),1920.,4096." not in assassin_init_source:
        raise ValueError("Assassin left-team search rectangle changed")
    if b"JAb=" not in assassin_init_source or b"(-1888.),(-4096.),6048.,4096." not in assassin_init_source:
        raise ValueError("Assassin right-team search rectangle changed")
    if b"Gcb=0.20 Fcb=12 Ecb=0.10 Dcb=1848652117" not in support_init_source or b"wcb=3.5" not in support_init_source:
        raise ValueError("Support-order queue constants changed")
    if b"TaskQueue_TaskQueue_executePeriodic(Hcb,Gcb)" not in support_init_source:
        raise ValueError("Support-order queue periodic scheduling changed")
    if not {"YAb", "XAb", "Dcb", "1093678925", "isCombatSapper"}.issubset(support_gate_tokens):
        raise ValueError("Support-order actor gate changed")
    if b"if(sBq==0)then return true" not in support_state_source or b"if uBq then return false" not in support_state_source or b"return(tBq>=wcb)" not in support_state_source:
        raise ValueError("Assassin support-order retry-state logic changed")
    if b"if((mBq==YAb)or(mBq==XAb))" not in support_dispatch_source or b"elseif(mBq==Dcb)then orderGoboW(lBq)" not in support_dispatch_source:
        raise ValueError("Support-order actor dispatch changed")
    if b"if(K7o<6)then N7o=KAb else N7o=JAb end" not in assassin_targets_source:
        raise ValueError("Assassin primary search-rect team selection changed")
    for symbol in (b"R7o=NAb", b"V7o=MAb", b"Z7o=LAb", b"d8o=NAb", b"h8o=MAb", b"l8o=LAb"):
        if symbol not in assassin_targets_source:
            raise ValueError("Assassin priority target-group wiring changed")
    if not {"UNIT_TYPE_FLYING", "1110454578", "1112040046", "isNotTentacle", "isNotBanished", "isVulnerable"}.issubset(assassin_valid_tokens):
        raise ValueError("Assassin base target predicate changed")
    if not {"851986", "852129", "OAb", "15", "group_getRandom"}.issubset(assassin_order_tokens):
        raise ValueError("Assassin order state machine changed")
    if b"__wurst_ensureInt(OX[n8o])>15" not in assassin_order_source:
        raise ValueError("Assassin target retry count changed")
    if b"isCloaked(m8o)or issueAssassinImmediateOrder(m8o,852129)" not in assassin_order_source:
        raise ValueError("Assassin Wind Walk/retarget gate changed")
    if b"issueAssassinTargetOrder(v8o,OAb,PX[w8o])" not in assassin_restore_source:
        raise ValueError("Assassin target-order restoration changed")
    if b"(UBq-__wurst_ensureReal(zcb[TBq]))<Ecb" not in gobbo_targets_source:
        raise ValueError("Gobbo target-group cache interval changed")
    if b"YBq=ycb" not in gobbo_targets_source or b"bCq=xcb" not in gobbo_targets_source:
        raise ValueError("Gobbo structure/mechanical priority filters changed")
    if not {"UNIT_TYPE_MECHANICAL", "UNIT_TYPE_STRUCTURE", "unit_getMaxHP", "unit_isInConstruction"}.issubset(gobbo_repair_tokens):
        raise ValueError("Gobbo repairable-target predicate changed")
    if b"(unit_getMaxHP(vBq)-unit_getHP(vBq))<=1." not in gobbo_repair_source:
        raise ValueError("Gobbo damaged-target threshold changed")
    if b"unit_isInConstruction(xBq)" not in gobbo_rally_source:
        raise ValueError("Gobbo rally-target construction exclusion changed")
    if b"group_getRandom(Acb[NBq])" not in gobbo_order_source:
        raise ValueError("Gobbo random repair-target selection changed")
    if b"unit_getRallyUnit(PBq)" not in gobbo_order_source:
        raise ValueError("Gobbo spawner rally fallback changed")
    if b"unit_issueTargetOrder(KBq,\"repair\",QBq)" not in gobbo_order_source or b"unit_issueTargetOrder(KBq,\"smart\",QBq)" not in gobbo_order_source:
        raise ValueError("Gobbo repair/smart rally orders changed")
    if not {"Ccb", "doAfter"}.issubset(gobbo_spawner_tokens):
        raise ValueError("Gobbo production-building association changed")
    if not {"orderAssassinOrGoboW", "TaskQueue_TaskQueue_add"}.issubset(task_run_tokens):
        raise ValueError("Support-order task loop changed")
    if not {"Bcb", "TaskQueue_TaskQueue_add"}.issubset(ensure_task_tokens):
        raise ValueError("Support-order task registration changed")
    if b"ucb=45. tcb=1848652117" not in gobbo_life_init_source:
        raise ValueError("Gobbo timed-life constants changed")
    if b"__wurst_safe_UnitApplyTimedLife(pCq,1112820806,ucb)" not in gobbo_life_source:
        raise ValueError("Gobbo one-shot timed-life application changed")
    if b"unit_getAbilityLevel(sXr,1093679435)>0" not in summon_source or b"__wurst_safe_UnitApplyTimedLife(sXr,1112820806,45.)" not in summon_source:
        raise ValueError("Gobbo summon timed-life path changed")
    if b"unit_getAbilityLevel(cgs,1093679435)>0" not in train_source or b"__wurst_safe_UnitApplyTimedLife(cgs,1112820806,45.)" not in train_source:
        raise ValueError("Gobbo train timed-life path changed")

    rows.append({
        "system_id": "support-order-controller",
        "mechanic_kind": "periodic-assassin-ambush-and-gobbo-repair-order-controller",
        "trigger": "indexed-support-unit-periodic-task-plus-train-summon-association",
        "parameters": {
            "actor_unit_ids": [1848652122, 1848652336, 1848652117],
            "support_marker_ability_id": 1093678925,
            "queue_interval_seconds": 0.20,
            "queue_tasks_per_tick": 12,
            "queue_maximum_interval_seconds": 0.75,
            "assassin_retarget_min_interval_seconds": 3.5,
            "target_group_refresh_min_interval_seconds": 0.10,
            "assassin_attack_order_id": 851983,
            "assassin_attack_order_name": "attack",
            "assassin_move_order_id": 851986,
            "assassin_move_order_name": "move",
            "assassin_windwalk_order_id": 852129,
            "assassin_windwalk_order_name": "windwalk",
            "assassin_existing_target_retry_limit": 15,
            "assassin_search_rect_player_id_lt_6": [-6016, -4096, 1920, 4096],
            "assassin_search_rect_player_id_gte_6": [-1888, -4096, 6048, 4096],
            "fallback_battlefield_rect": [-6176, -3584, 6176, 3584],
            "assassin_base_target_predicate": "alive-combat-sapper;enemy;vulnerable;not-tentacle;not-banished;not-flying;lacks-1110454578;lacks-1112040046",
            "assassin_priority_filters": [
                {"priority": 1, "scope": "team-selected-primary-rect", "predicate": "valid-target;max-mana>10"},
                {"priority": 2, "scope": "team-selected-primary-rect", "normal_predicate": "valid-target;life<150", "royal_predicate": "valid-target;life<300"},
                {"priority": 3, "scope": "fallback-battlefield-rect", "predicate": "valid-target;max-mana>10"},
                {"priority": 4, "scope": "fallback-battlefield-rect", "normal_predicate": "valid-target;life<150", "royal_predicate": "valid-target;life<300"},
            ],
            "assassin_random_choice_within_priority_group": True,
            "assassin_does_not_retarget_while_attacking_valid_assigned_target": True,
            "assassin_attempts_windwalk_before_new_target_selection": True,
            "gobbo_target_predicate": "alive;ally;damaged-by-more-than-1-hp;not-in-construction;structure-or-mechanical",
            "gobbo_priority_1": "damaged-allied-structures",
            "gobbo_priority_2_if_no_structures": "damaged-allied-mechanical-units",
            "gobbo_random_choice_within_priority_group": True,
            "gobbo_fallback": "production-building-rally-unit-if-allied-and-not-in-construction",
            "gobbo_fallback_order": "repair-if-damaged-structure-or-mechanical-else-smart",
            "gobbo_timed_life_seconds": 45,
            "gobbo_timed_life_buff_id": 1112820806,
            "gobbo_timed_life_applied_once_per_indexed_gobbo": True,
            "gobbo_one_per_target_claim_proven_by_script": False,
        },
        "related_rawcode_ids": [
            1848652122, 1848652336, 1848652117, 1093678925, 1093679435,
            1110454578, 1112040046, 1112820806,
        ],
        "source_functions": [
            "lE", "nH", "isSupportOrderUnit", "orderAssassinOrGoboW", "shouldRunPeriodicSupportOrderForState",
            "prepareAssassinTargets", "isValidAssassinTarget", "orderAssassinW", "restoreAssassinTargetOrderW",
            "prepareGobboTargets", "isGobboRepairableTarget", "isGobboRallyTarget", "orderGoboW",
            "setGobboSpawner", "SupportOrderTask_SupportOrderTask_run", "ensureSupportOrderTask",
            "sH", "applyGobboTimedLife", "onUnitTrained", "onSummonedUnit",
        ],
        "evidence_kind": "exact-shared-task-queue-target-filters-order-state-machine-and-timed-life",
        "byte_offset": min(
            assassin_init_start, support_init_start, support_gate_start, support_dispatch_start,
            assassin_targets_start, assassin_valid_start, assassin_order_start, gobbo_targets_start,
            gobbo_order_start, task_run_start, gobbo_life_init_start, gobbo_life_start,
        ),
    })

    # External/player order suppression. Castle Fight units are autonomous;
    # three unit-order event listeners reject player stop/move/attack orders
    # unless they match narrow scripted exceptions, restore the unit's expected
    # autonomous/support order, and penalize allied players who had the unit
    # selected. The escalating control lock is tracked separately from the
    # order restoration itself.
    spam_init_start, spam_init_source, spam_init_tokens = source("IL")
    spam_register_start, spam_register_source, spam_register_tokens = source(
        "CallbackSingle_doAfter_SpamPrevention_call_doAfter_SpamPrevention"
    )
    spam_point_start, spam_point_source, spam_point_tokens = source(
        "EventListener_add_doAfter_SpamPrevention_onEvent_add_doAfter_SpamPrevention"
    )
    spam_target_start, spam_target_source, spam_target_tokens = source(
        "EventListener_add_doAfter_SpamPrevention_onEvent_add_doAfter_SpamPrevention1"
    )
    spam_immediate_start, spam_immediate_source, spam_immediate_tokens = source(
        "EventListener_add_doAfter_SpamPrevention_onEvent_add_doAfter_SpamPrevention2"
    )
    spam_check_start, spam_check_source, spam_check_tokens = source("checkForWrongOrder")
    spam_timeout_start, spam_timeout_source, spam_timeout_tokens = source("getOrderSpamTimeoutSeconds")
    spam_next_start, spam_next_source, spam_next_tokens = source("nextOrderSpamTimeout")
    spam_should_start_start, spam_should_start_source, spam_should_start_tokens = source("shouldStartOrderSpamPenalty")
    spam_disabled_until_start, spam_disabled_until_source, spam_disabled_until_tokens = source(
        "disabledUntilForNewOrderSpamPenalty"
    )
    spam_start_start, spam_start_source, spam_start_tokens = source("startOrderSpamPenalty")
    spam_punish_start, spam_punish_source, spam_punish_tokens = source("punishSelectedLocalControllers")
    spam_assassin_start, spam_assassin_source, spam_assassin_tokens = source("isAssassinSupportUnit")
    spam_restore_gate_start, spam_restore_gate_source, spam_restore_gate_tokens = source(
        "shouldRestoreSupportOrderAfterExternalOrder"
    )
    spam_restore_start, spam_restore_source, spam_restore_tokens = source("restoreExpectedOrderAfterExternalOrder")
    spam_release_start, spam_release_source, spam_release_tokens = source("KL")
    spam_order_init_start, spam_order_init_source, spam_order_init_tokens = source("kJ")

    if b"oV=600. nV=300. mV=0.25" not in spam_init_source or b"doAfter(.1,oQr)" not in spam_init_source:
        raise ValueError("Order-suppression timing constants changed")
    for event_name in (
        b"EVENT_PLAYER_UNIT_ISSUED_POINT_ORDER",
        b"EVENT_PLAYER_UNIT_ISSUED_TARGET_ORDER",
        b"EVENT_PLAYER_UNIT_ISSUED_ORDER",
    ):
        if event_name not in spam_register_source:
            raise ValueError("Order-suppression event registration changed")
    for listener_source in (spam_point_source, spam_target_source, spam_immediate_source):
        if b"checkForWrongOrder()" not in listener_source:
            raise ValueError("Order-suppression event adapter changed")
    if b"L6=851971" not in spam_order_init_source or b"O6=851972" not in spam_order_init_source or b"X6=851983" not in spam_order_init_source:
        raise ValueError("Order-suppression stop/move/attack ids changed")
    for fragment in (
        b"if(jY or(not isCombatSapper(cRr)))then jY=false return end",
        b"if(eRr==L6)then dRr=true",
        b"elseif(eRr==O6)then",
        b"unit_getAbilityLevel(cRr,1093678925)>=1",
        b"elseif(eRr==X6)then",
        b"unit_getTypeId(fRr)==1697656902",
        b"unit_getTypeId(cRr)==1848652371",
        b"unit_getTypeId(cRr)==1848652372",
        b"unit_isType(fRr,UNIT_TYPE_FLYING)",
        b"hRr=real_abs(GetOrderPointX())",
        b"iRr=GetOrderPointY()",
        b"hRr>=2047.99",
        b"hRr<=2048.01",
        b"hRr>=4991.99",
        b"hRr<=4992.01",
        b"kRr=(iRr==0.0)",
        b"punishSelectedLocalControllers(cRr,lRr)",
        b"restoreExpectedOrderAfterExternalOrder(cRr)",
    ):
        if fragment not in spam_check_source:
            raise ValueError("Order-suppression predicate/restoration changed")
    if b"if(tQr<=1)then return 5." not in spam_timeout_source or b"if(tQr==2)then return 15." not in spam_timeout_source:
        raise ValueError("Order-spam first/second penalty durations changed")
    if b"uQr=(uQr*2.)" not in spam_timeout_source or b"if(uQr>nV)then return nV" not in spam_timeout_source:
        raise ValueError("Order-spam exponential penalty/cap changed")
    if b"(FQr-__wurst_ensureReal(qV[EQr]))>=oV" not in spam_next_source or b"rV[EQr]=0" not in spam_next_source:
        raise ValueError("Order-spam offense reset window changed")
    if b"rV[EQr]=(__wurst_ensureInt(rV[EQr])+1)" not in spam_next_source or b"qV[EQr]=FQr" not in spam_next_source:
        raise ValueError("Order-spam offense counter changed")
    if b"return(not xQr)" not in spam_should_start_source:
        raise ValueError("Order-spam active-penalty gate changed")
    if b"if yQr then CQr=AQr else CQr=(zQr+BQr)end" not in spam_disabled_until_source:
        raise ValueError("Order-spam disabled-until computation changed")
    if b"if(not shouldStartOrderSpamPenalty(__wurst_ensureBool(vV[KQr])))then return false" not in spam_start_source:
        raise ValueError("Order-spam duplicate-penalty suppression changed")
    if b"vV[KQr]=true return true" not in spam_start_source:
        raise ValueError("Order-spam activation state changed")
    for fragment in (
        b"player_isAllyOf(UQr,RQr)",
        b"__wurst_safe_IsUnitSelected(QQr,UQr)",
        b"VQr=startOrderSpamPenalty(UQr)",
        b"player_clearSelection(UQr)",
        b"player_setUserControl(WQr,false)",
        b"DisplayTimedTextToPlayer",
        b"player_unselect(UQr,QQr)",
    ):
        if fragment not in spam_punish_source:
            raise ValueError("Order-spam selected-controller punishment changed")
    if b"1093678925" not in spam_assassin_source or b"YAb" not in spam_assassin_source or b"XAb" not in spam_assassin_source:
        raise ValueError("Order-suppression Assassin support-unit exception changed")
    if b"aRr and(((ZQr==YAb)or(ZQr==XAb))or(ZQr==Dcb)" not in spam_restore_gate_source:
        raise ValueError("Order-suppression support-order restoration gate changed")
    if b"restoreSupportOrderW(bRr)" not in spam_restore_source or b"orderCodeAttack(bRr)" not in spam_restore_source:
        raise ValueError("Order-suppression restoration action changed")
    if b"player_setUserControl(PQr,true)" not in spam_release_source or b"vV[NQr]=false pV[NQr]=0." not in spam_release_source:
        raise ValueError("Order-spam control-release handler changed")

    rows.append({
        "system_id": "external-unit-order-suppression",
        "mechanic_kind": "player-order-rejection-autonomous-order-restoration-and-escalating-control-penalty",
        "trigger": "player-issued-point-target-or-immediate-unit-order",
        "parameters": {
            "listener_install_delay_seconds": 0.1,
            "listened_event_types": [
                "EVENT_PLAYER_UNIT_ISSUED_POINT_ORDER",
                "EVENT_PLAYER_UNIT_ISSUED_TARGET_ORDER",
                "EVENT_PLAYER_UNIT_ISSUED_ORDER",
            ],
            "unit_scope": "combat-sapper",
            "script_issued_order_bypass_flag": "jY",
            "script_issued_order_bypass_consumes_next_listener_event": True,
            "rejected_order_ids": {
                "stop": 851971,
                "move": 851972,
                "attack": 851983,
            },
            "move_order_allowed_if": "unit has A07M and is not Assassin/Royal-Assassin support unit",
            "attack_target_order_allowed_if": [
                "unit has A07M and is not Assassin/Royal-Assassin support unit",
                "target is Mountain Giant e00F",
                "source is Gnoll n02S or Fire Gnoll n02T and target is flying",
            ],
            "attack_point_order_allowed_if": "order-point Y equals 0, or abs(X) is within 0.01 of 2048 or 4992",
            "assassin_support_attack_point_order_is_rejected": True,
            "rejected_order_restoration": "restore Assassin/Royal-Assassin/Gobbo support order when A07M support state applies; otherwise issue ordinary Castle Fight attack order",
            "penalty_applies_to": "allied players currently selecting the rejected-order unit",
            "first_violation_control_lock_seconds": 5,
            "second_violation_control_lock_seconds": 15,
            "later_control_lock_formula": "15 * 2^(offense_count-2), capped at 300 seconds",
            "control_lock_sequence_seconds": [5, 15, 30, 60, 120, 240, 300],
            "control_lock_cap_seconds": 300,
            "offense_count_reset_gap_seconds": 600,
            "already_locked_repeat_does_not_extend_lock": True,
            "new_lock_clears_player_selection": True,
            "already_locked_repeat_only_unselects_this_unit": True,
            "new_lock_disables_user_control": True,
            "warning_text_duration_seconds": 5,
            "protected_release_timer_period_seconds": 0.25,
            "control_release_handler_reenables_user_control": True,
            "protected_release_timer_to_KL_binding_proven": False,
        },
        "related_rawcode_ids": [
            1093678925, 1848652371, 1848652372, 1697656902,
            1848652122, 1848652336, 1848652117,
        ],
        "source_functions": [
            "IL", "CallbackSingle_doAfter_SpamPrevention_call_doAfter_SpamPrevention",
            "EventListener_add_doAfter_SpamPrevention_onEvent_add_doAfter_SpamPrevention",
            "EventListener_add_doAfter_SpamPrevention_onEvent_add_doAfter_SpamPrevention1",
            "EventListener_add_doAfter_SpamPrevention_onEvent_add_doAfter_SpamPrevention2",
            "checkForWrongOrder", "getOrderSpamTimeoutSeconds", "nextOrderSpamTimeout",
            "shouldStartOrderSpamPenalty", "disabledUntilForNewOrderSpamPenalty", "startOrderSpamPenalty",
            "punishSelectedLocalControllers", "isAssassinSupportUnit", "shouldRestoreSupportOrderAfterExternalOrder",
            "restoreExpectedOrderAfterExternalOrder", "KL", "kJ",
        ],
        "evidence_kind": "exact-order-listeners-exceptions-restoration-and-escalating-player-control-penalty-with-protected-release-binding-left-unclaimed",
        "byte_offset": min(
            spam_init_start, spam_register_start, spam_point_start, spam_target_start, spam_immediate_start,
            spam_check_start, spam_timeout_start, spam_next_start, spam_start_start, spam_punish_start,
            spam_restore_start, spam_release_start, spam_order_init_start,
        ),
    })

    # Global idle-unit re-engagement. At round start the map starts a 4-second
    # timer that scans the battlefield and reissues the ordinary Castle Fight
    # attack order only to idle, alive/vulnerable combat sappers. Units carrying
    # either of the two channel/support markers are deliberately excluded.
    round_start_start, round_start_source, round_start_tokens = source("completeRoundStart")
    idle_timer_start, idle_timer_source, idle_timer_tokens = source("startIdleAttackTimer")
    idle_pred_start, idle_pred_source, idle_pred_tokens = source("isIdleAttackUnit")
    idle_tick_start, idle_tick_source, idle_tick_tokens = source("code__TimerStart_IdleAttackRuntime")
    idle_cb_start, idle_cb_source, idle_cb_tokens = source(
        "ForGroupCallback_forUnitsInRect_IdleAttackRuntime_callback_forUnitsInRect_IdleAttackRuntime"
    )
    if b"Dd()startIdleAttackTimer()" not in round_start_source:
        raise ValueError("Idle-attack timer no longer starts with the round")
    if b"__wurst_safe_TimerStart(Nab,4.,true" not in idle_timer_source:
        raise ValueError("Idle-attack sweep interval changed")
    for fragment in (
        b"widget_getLife(lEq)>.405",
        b"unit_getCurrentOrder(lEq)==0",
        b"isCombatSapper(lEq)",
        b"isVulnerable(lEq)",
        b"unit_getAbilityLevel(lEq,1093678921)<=0",
        b"unit_getAbilityLevel(lEq,1093678925)<=0",
    ):
        if fragment not in idle_pred_source:
            raise ValueError("Idle-attack unit predicate changed")
    if b"pEq=uIb" not in idle_tick_source or b"__wurst_safe_GroupEnumUnitsInRect(Oib,pEq,Gib)" not in idle_tick_source:
        raise ValueError("Idle-attack battlefield enumeration changed")
    if b"if isIdleAttackUnit(TCm)then orderCodeAttack(TCm)end" not in idle_cb_source:
        raise ValueError("Idle-attack re-engage callback changed")
    rows.append({
        "system_id": "global-idle-attack-reengage",
        "mechanic_kind": "periodic-idle-combat-unit-attack-order-recovery",
        "trigger": "round-start-then-every-4-seconds",
        "parameters": {
            "interval_seconds": 4,
            "enumeration_rect_symbol": "uIb",
            "target_predicate": "alive;current-order-zero;combat-sapper;vulnerable;lacks-A07I;lacks-A07M",
            "excluded_ability_ids": [1093678921, 1093678925],
            "action": "orderCodeAttack",
            "starts_each_round": True,
        },
        "related_rawcode_ids": [1093678921, 1093678925],
        "source_functions": [
            "completeRoundStart", "startIdleAttackTimer", "isIdleAttackUnit",
            "code__TimerStart_IdleAttackRuntime",
            "ForGroupCallback_forUnitsInRect_IdleAttackRuntime_callback_forUnitsInRect_IdleAttackRuntime",
        ],
        "evidence_kind": "exact-round-start-periodic-global-enumeration-and-unit-predicate",
        "byte_offset": min(round_start_start, idle_timer_start, idle_pred_start, idle_tick_start, idle_cb_start),
    })

    # Builder Blink. Every spawned builder receives A0-1. The cast handler does
    # not trust the requested point: it selects the owner's own castle rectangle,
    # clamps X/Y to a 64-unit inset, teleports immediately, then emits O6.
    blink_init_start, blink_init_source, blink_init_tokens = source("uL")
    blink_register_start, blink_register_source, blink_register_tokens = source("CE")
    blink_cast_start, blink_cast_source, blink_cast_tokens = source("EventListener_add_Blink_onEvent_add_Blink")
    order_init_start, order_init_source, order_init_tokens = source("kJ")
    enemy_rect_start, enemy_rect_source, enemy_rect_tokens = source("enemyCastleRect")
    if b"kY=1093676337" not in blink_init_source:
        raise ValueError("Builder Blink ability rawcode changed")
    if b"EVENT_PLAYER_UNIT_SPELL_CAST" not in blink_register_source or b"Xb:create35()" not in blink_register_source:
        raise ValueError("Builder Blink spell-cast registration changed")
    if b"bYq=kY addProtectedAbility(aYq,bYq)" not in round_start_source:
        raise ValueError("Round-start builder Blink grant changed")
    if b"if(GetSpellAbilityId()==kY)" not in blink_cast_source:
        raise ValueError("Builder Blink ability gate changed")
    if b"if(__wurst_ensureInt(lGb[player_getId(PAk)])==0)then RAk=NFb else RAk=MFb end" not in blink_cast_source:
        raise ValueError("Builder Blink castle-rect selection changed")
    if b"if(__wurst_ensureInt(lGb[player_getId(unit_getOwner(dEq))])==0)then eEq=MFb else eEq=NFb end" not in enemy_rect_source:
        raise ValueError("Enemy-castle rectangle orientation changed")
    for fragment in (
        b"rect_getMinX(QAk)+64.", b"rect_getMaxX(QAk)-64.",
        b"rect_getMinY(QAk)+64.", b"rect_getMaxY(QAk)-64.",
        b"__wurst_safe_SetUnitPosition(pBk,qBk[1],qBk[2])",
        b"unit_issuePointOrderById(bBk,O6,cBk)",
    ):
        if fragment not in blink_cast_source:
            raise ValueError("Builder Blink clamp/teleport sequence changed")
    if b"O6=851972" not in order_init_source:
        raise ValueError("Builder Blink post-teleport order id changed")
    rows.append({
        "system_id": "builder-castle-blink",
        "mechanic_kind": "builder-point-teleport-clamped-to-own-castle",
        "trigger": "A0-1-spell-cast",
        "parameters": {
            "ability_id": 1093676337,
            "ability_rawcode": "A0-1",
            "granted_to_spawned_builders": True,
            "destination_rect": "owner-own-castle-rect",
            "destination_rect_proof": "team0 uses NFb while enemyCastleRect(team0)=MFb; team1 uses MFb while enemyCastleRect(team1)=NFb",
            "rect_inset_world_units": 64,
            "clamp_x": True,
            "clamp_y": True,
            "teleport_primitive": "SetUnitPosition",
            "post_teleport_order_id": 851972,
            "post_teleport_order_symbol": "O6",
        },
        "related_rawcode_ids": [1093676337],
        "source_functions": ["uL", "CE", "completeRoundStart", "EventListener_add_Blink_onEvent_add_Blink", "enemyCastleRect", "kJ"],
        "evidence_kind": "exact-builder-grant-spell-cast-rect-clamp-and-position-set",
        "byte_offset": min(blink_init_start, blink_register_start, round_start_start, blink_cast_start, enemy_rect_start, order_init_start),
    })

    # Rescue Strike. A005 is a point-cast, once-available builder ability. The
    # script removes it immediately, creates an invisible marker, waits 0.35s,
    # then applies two lethal-scale damage packets to each eligible enemy in a
    # 700 radius. The team receives a short coordination cooldown; a zero-kill
    # strike alone is refunded, with a punitive 180-second cooldown.
    rescue_register_start, rescue_register_source, rescue_register_tokens = source("gL")
    rescue_cast_start, rescue_cast_source, rescue_cast_tokens = source(
        "OnPointCast_onPointCast_RescueStrikeRuntime_fireEx_onPointCast_RescueStrikeRuntime"
    )
    rescue_delay_start, rescue_delay_source, rescue_delay_tokens = source(
        "CallbackSingle_doAfter_RescueStrikeRuntime_call_doAfter_RescueStrikeRuntime1"
    )
    rescue_apply_start, rescue_apply_source, rescue_apply_tokens = source("applyRescueStrike")
    rescue_cd_start, rescue_cd_source, rescue_cd_tokens = source("startRescueStrikeCooldown")
    rescue_team_cd_start, rescue_team_cd_source, rescue_team_cd_tokens = source("startTeamRescueStrikeCooldown")
    rescue_finish_start, rescue_finish_source, rescue_finish_tokens = source("finishRescueStrike")
    if b"EventListener_addSpellInternal(nil,1093677109,aDr)" not in rescue_register_source:
        raise ValueError("Rescue Strike A005 registration changed")
    for fragment in (
        b"createUnit(VBn,1747989592,UBn,{0.})",
        b"__wurst_safe_SetUnitVertexColor(YBn,0,0,0,0)",
        b"unit_removeAbility(TBn,1093677109)",
        b"unit_removeAbility(TBn,1093678661)",
        b"doAfter(.35,XBn)",
    ):
        if fragment not in rescue_cast_source:
            raise ValueError("Rescue Strike cast/marker setup changed")
    if b"applyRescueStrike(LBn.caster,LBn.marker,LBn.p,LBn.targetPos)" not in rescue_delay_source:
        raise ValueError("Rescue Strike delayed apply callback changed")
    for fragment in (
        b"__wurst_safe_GroupEnumUnitsInRange(GDr,MDr[1],MDr[2],700.,nil)",
        b"unit_isEnemyOf(JDr,EDr)",
        b"widget_getLife(JDr)>0.405",
        b"unit_getAbilityLevel(JDr,1098282348)<=0",
        b"__wurst_safe_UnitDamageTarget(CDr,JDr,4444.,true,false,ATTACK_TYPE_CHAOS,DAMAGE_TYPE_DEATH,WEAPON_TYPE_WHOKNOWS)",
        b"__wurst_safe_UnitDamageTarget(CDr,JDr,4444.,true,false,ATTACK_TYPE_NORMAL,DAMAGE_TYPE_MAGIC,WEAPON_TYPE_WHOKNOWS)",
        b"unit_issueImmediateOrderById(DDr,852526)",
        b"startTeamRescueStrikeCooldown(EDr)",
        b"doAfter(1.5,LDr)",
    ):
        if fragment not in rescue_apply_source:
            raise ValueError("Rescue Strike damage/finalization sequence changed")
    if b"__wurst_safe_BlzStartUnitAbilityCooldown(fDr,1093677109,2.)" not in rescue_cd_source:
        raise ValueError("Rescue Strike team coordination cooldown changed")
    if not {"oGb", "nGb", "startRescueStrikeCooldown"}.issubset(rescue_team_cd_tokens):
        raise ValueError("Rescue Strike team cooldown fan-out changed")
    for fragment in (
        b"__wurst_safe_RemoveUnit(zDr)",
        b"kX[xDr]=(__wurst_ensureInt(kX[xDr])-1)",
        b"if(vDr==0)then",
        b"addProtectedAbility(ADr,1093677109)",
        b"addProtectedAbility(BDr,1093678661)",
        b"__wurst_safe_BlzStartUnitAbilityCooldown(rDr,1093677109,180.0)",
        b"kX[yDr]=(__wurst_ensureInt(kX[yDr])+1)",
    ):
        if fragment not in rescue_finish_source:
            raise ValueError("Rescue Strike finish/refund behavior changed")
    rows.append({
        "system_id": "rescue-strike",
        "mechanic_kind": "builder-point-cast-team-coordinated-area-execution",
        "trigger": "A005-point-cast",
        "parameters": {
            "ability_id": 1093677109,
            "effect_ability_id": 1093678661,
            "marker_unit_id": 1747989592,
            "cast_delay_seconds": 0.35,
            "radius": 700,
            "target_predicate": "enemy;alive;lacks-Avul",
            "excluded_ability_id": 1098282348,
            "marks_targets_as_scripted_death": True,
            "damage_packets": [
                {"amount": 4444, "attack_type": "chaos", "damage_type": "death"},
                {"amount": 4444, "attack_type": "normal", "damage_type": "magic"},
            ],
            "marker_post_damage_order_id": 852526,
            "team_coordination_cooldown_seconds": 2,
            "finish_delay_seconds": 1.5,
            "ability_removed_on_cast": True,
            "effect_ability_removed_on_cast": True,
            "zero_kill_refunds_ability_and_effect": True,
            "zero_kill_refund_cooldown_seconds": 180,
            "nonzero_kill_does_not_refund_in_finish_handler": True,
            "records_sum_of_pre_damage_target_life": True,
            "team_available_strike_counter_decremented_on_resolution": True,
        },
        "related_rawcode_ids": [1093677109, 1093678661, 1747989592, 1098282348],
        "source_functions": [
            "gL", "OnPointCast_onPointCast_RescueStrikeRuntime_fireEx_onPointCast_RescueStrikeRuntime",
            "CallbackSingle_doAfter_RescueStrikeRuntime_call_doAfter_RescueStrikeRuntime1",
            "applyRescueStrike", "startRescueStrikeCooldown", "startTeamRescueStrikeCooldown", "finishRescueStrike",
        ],
        "evidence_kind": "exact-cast-removal-delayed-area-damage-team-cooldown-and-zero-kill-refund",
        "byte_offset": min(
            rescue_register_start, rescue_cast_start, rescue_delay_start, rescue_apply_start,
            rescue_cd_start, rescue_team_cd_start, rescue_finish_start,
        ),
    })

    # Obelisk of Elements / Forge Weapon. A0F1 is only a marker; the actual
    # 50% cleave is implemented in DamageRuntime and requires the generic A03Q
    # source-damage dispatcher marker as well. Preserve the generated compacted
    # choice-index quirk: holyShrineSpell adds A03Q only when the selected array
    # index is 2, which is guaranteed to mean A0F1 only while all three options
    # are still absent.
    holy_start, holy_source, holy_tokens = source("holyShrineSpell")
    source_damage_start, source_damage_source, source_damage_tokens = source("handleSourceDamageEffects")
    cleave_pred_start, cleave_pred_source, cleave_pred_tokens = source("cleaveHitsTarget")
    cleave_cb_start, cleave_cb_source, cleave_cb_tokens = source(
        "ForGroupCallback_forUnitsInRange_DamageRuntime_callback_forUnitsInRange_DamageRuntime"
    )
    if b"Qpr[0]=1093682231 Qpr[1]=1093682739 Qpr[2]=1093682737" not in holy_source:
        raise ValueError("Obelisk of Elements enchant candidate table changed")
    if b"if(Spr==2)then Xpr=Opr addProtectedAbility(Xpr,1093677905)end" not in holy_source:
        raise ValueError("Forge Weapon A03Q dispatcher-marker grant condition changed")
    if b"unit_getAbilityLevel(Qpq,1093677905)<=0" not in source_damage_source:
        raise ValueError("DamageRuntime source dispatcher gate changed")
    if b"if(unit_getAbilityLevel(Qpq,1093682737)>0)then Tpq=(.5*GetEventDamage())" not in source_damage_source:
        raise ValueError("Forge Weapon cleave marker/damage multiplier changed")
    if b"forUnitsInRange(bqq,250.,false,cqq)" not in source_damage_source:
        raise ValueError("Forge Weapon cleave radius changed")
    for fragment in (
        b"unit_isEnemyOf(kpq,unit_getOwner(jpq))",
        b"isCombatSapper(kpq)",
        b"isVulnerable(kpq)",
        b"lpq and unit_isType(kpq,UNIT_TYPE_GROUND)",
        b"mpq and unit_isType(kpq,UNIT_TYPE_FLYING)",
    ):
        if fragment not in cleave_pred_source:
            raise ValueError("Forge Weapon cleave target predicate changed")
    if b"__wurst_safe_UnitDamageTarget(W9l.dummy,X9l,W9l.damage,true,false,ATTACK_TYPE_CHAOS,DAMAGE_TYPE_UNKNOWN,WEAPON_TYPE_WHOKNOWS)" not in cleave_cb_source:
        raise ValueError("Forge Weapon cleave damage packet changed")
    rows.append({
        "system_id": "elemental-forge-weapon-cleave",
        "mechanic_kind": "enchantment-marker-driven-half-damage-cleave",
        "trigger": "source-damage-event-with-A03Q-and-A0F1",
        "parameters": {
            "source_damage_dispatch_marker_ability_id": 1093677905,
            "forge_weapon_marker_ability_id": 1093682737,
            "enchantment_candidate_ability_ids": [1093682231, 1093682739, 1093682737],
            "dispatcher_marker_grant_condition": "compacted-selected-index-equals-2",
            "first_full_candidate_set_index_2_is_forge_weapon": True,
            "later_compaction_can_move_forge_weapon_to_another_index": True,
            "forge_weapon_can_therefore_exist_without_dispatcher_marker": True,
            "damage_multiplier_of_triggering_damage": 0.5,
            "radius": 250,
            "source_attack_capability_controls_ground_and_flying_hits": True,
            "target_predicate": "alive;enemy-of-source-owner;combat-sapper;vulnerable;matches-source-ground-or-flying-attack-capability",
            "dummy_unit_id": 1697656888,
            "dummy_timed_life_seconds": 1,
            "damage_is_attack": True,
            "damage_is_ranged": False,
            "attack_type": "chaos",
            "damage_type": "unknown",
        },
        "related_rawcode_ids": [1093677905, 1093682737, 1093682231, 1093682739, 1697656888],
        "source_functions": [
            "holyShrineSpell", "handleSourceDamageEffects", "cleaveHitsTarget",
            "ForGroupCallback_forUnitsInRange_DamageRuntime_callback_forUnitsInRange_DamageRuntime",
        ],
        "evidence_kind": "exact-enchantment-grant-condition-and-damage-runtime-cleave",
        "byte_offset": min(holy_start, source_damage_start, cleave_pred_start, cleave_cb_start),
    })

    # Locust Harpy. This summoned/non-production helper has a protected 142-146
    # spell attack, but DamageRuntime halves the final current damage instance
    # whenever its target is a structure.
    shared_damage_start, shared_damage_source, shared_damage_tokens = source(
        "DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire"
    )
    if b"if((unit_getTypeId(w6n)==1966092359)and unit_isType(x6n,UNIT_TYPE_STRUCTURE))then E7n=(DamageEvent_getAmount()*0.5)DamageInstance_DamageInstance_setAmount(khb,E7n)end" not in shared_damage_source:
        raise ValueError("Locust Harpy structure-damage penalty changed")
    rows.append({
        "system_id": "locust-harpy-structure-damage-penalty",
        "mechanic_kind": "unit-type-structure-target-current-damage-halving",
        "trigger": "damage-event-source-u00G-target-structure",
        "parameters": {
            "unit_id": 1966092359,
            "target_requires_structure": True,
            "damage_multiplier": 0.5,
            "modifies_current_damage_instance": True,
        },
        "related_rawcode_ids": [1966092359],
        "source_functions": ["DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire"],
        "evidence_kind": "exact-shared-damage-listener-current-damage-rewrite",
        "byte_offset": shared_damage_start,
    })

    # Celestial Chi Tower. Damage impacts are collected for 0.03 seconds per
    # owner, averaged, and converted into an 8-second 512-radius visible fog
    # modifier. This is a passive attack side effect, not an ordinary spell row.
    chi_batch_start, chi_batch_source, chi_batch_tokens = source(
        "CallbackSingle_doAfter_addListener_doAfter_ThunderpawSpire_call_doAfter_addListener_doAfter_ThunderpawSpire2"
    )
    chi_cleanup_start, chi_cleanup_source, chi_cleanup_tokens = source(
        "CallbackSingle_doAfter_doAfter_addListener_doAfter_ThunderpawSpire_call_doAfter_doAfter_addListener_doAfter_ThunderpawSpire"
    )
    vision_start, vision_source, vision_tokens = source("createVision")
    if b"if(unit_getTypeId(w6n)==1747990358)then" not in shared_damage_source:
        raise ValueError("Celestial Chi Tower damage-source branch changed")
    for fragment in (
        b"PS[O6n]=(__wurst_ensureReal(PS[O6n])+P6n[1])",
        b"OS[O6n]=(__wurst_ensureReal(OS[O6n])+P6n[2])",
        b"NS[O6n]=(__wurst_ensureInt(NS[O6n])+1)",
        b"doAfter(0.03,Q6n)",
    ):
        if fragment not in shared_damage_source:
            raise ValueError("Celestial Chi Tower impact batching changed")
    if b"U7n={(__wurst_ensureReal(PS[T7n.index])/__wurst_ensureInt(NS[T7n.index]));(__wurst_ensureReal(OS[T7n.index])/__wurst_ensureInt(NS[T7n.index]))}" not in chi_batch_source:
        raise ValueError("Celestial Chi Tower averaged impact position changed")
    if b"V7n=createVision(T7n.owner,U7n,512.,true)" not in chi_batch_source or b"doAfter(8.,W7n)" not in chi_batch_source:
        raise ValueError("Celestial Chi Tower vision radius/duration changed")
    if b"PS[T7n.index]=0. OS[T7n.index]=0. NS[T7n.index]=0" not in chi_batch_source:
        raise ValueError("Celestial Chi Tower impact accumulator reset changed")
    if b"__wurst_safe_FogModifierStop(a8n)" not in chi_cleanup_source or b"__wurst_safe_DestroyFogModifier(b8n)" not in chi_cleanup_source:
        raise ValueError("Celestial Chi Tower vision cleanup changed")
    if b"CreateFogModifierRadius(fzq,FOG_OF_WAR_VISIBLE" not in vision_source:
        raise ValueError("Celestial Chi Tower createVision semantics changed")
    rows.append({
        "system_id": "celestial-chi-tower-impact-vision",
        "mechanic_kind": "tower-damage-impact-batched-temporary-vision",
        "trigger": "damage-event-source-h07V",
        "parameters": {
            "tower_unit_id": 1747990358,
            "batch_window_seconds": 0.03,
            "batch_scope": "source-owner-player-id",
            "batch_position": "arithmetic-mean-of-damaged-target-positions",
            "vision_radius": 512,
            "vision_duration_seconds": 8,
            "fog_state": "visible",
            "fog_modifier_shared_vision_flag": True,
            "accumulators_reset_after_batch": True,
        },
        "related_rawcode_ids": [1747990358],
        "source_functions": [
            "DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire",
            "CallbackSingle_doAfter_addListener_doAfter_ThunderpawSpire_call_doAfter_addListener_doAfter_ThunderpawSpire2",
            "createVision",
            "CallbackSingle_doAfter_doAfter_addListener_doAfter_ThunderpawSpire_call_doAfter_doAfter_addListener_doAfter_ThunderpawSpire",
        ],
        "evidence_kind": "exact-damage-impact-accumulator-delayed-average-vision-and-cleanup",
        "byte_offset": min(shared_damage_start, chi_batch_start, vision_start, chi_cleanup_start),
    })

    # Castle protection. The round timer is reset to 0:00 by completeRoundStart
    # and yd advances its second/minute counters once per second. For the first
    # 15 seconds of an active round, any positive damage instance targeting
    # either team castle is rewritten to zero.
    castle_protect_init_start, castle_protect_init_source, castle_protect_init_tokens = source("YF")
    castle_protect_pred_start, castle_protect_pred_source, castle_protect_pred_tokens = source("isCastleProtected")
    round_start_start, round_start_source, round_start_tokens = source("completeRoundStart")
    round_tick_start, round_tick_source, round_tick_tokens = source("yd")
    castle_protect_damage_start, castle_protect_damage_source, castle_protect_damage_tokens = source(
        "DamageListener_addListener_CastleProtection_onEvent_addListener_CastleProtection"
    )
    if b"zkb=15" not in castle_protect_init_source or "DamageEvent_addListener" not in castle_protect_init_tokens:
        raise ValueError("Castle protection duration/listener registration changed")
    if b"isRoundStarted()and(VGb==0))and(UGb<zkb)" not in castle_protect_pred_source:
        raise ValueError("Castle protection round-time predicate changed")
    if b"(H_p==cX[0])or(H_p==cX[1])" not in castle_protect_pred_source:
        raise ValueError("Castle protection castle target predicate changed")
    if b"VGb=0 UGb=0" not in round_start_source:
        raise ValueError("Round timer reset at round start changed")
    if b"UGb=(UGb+1)if(UGb==60)then UGb=0 VGb=(VGb+1)" not in round_tick_source:
        raise ValueError("Round minute/second timer advancement changed")
    if b"DamageEvent_getAmount()>0." not in castle_protect_damage_source or b"isCastleProtected(BOl)" not in castle_protect_damage_source:
        raise ValueError("Castle protection damage-event gate changed")
    if b"DamageInstance_DamageInstance_setAmount(khb,0.)" not in castle_protect_damage_source:
        raise ValueError("Castle protection zero-damage rewrite changed")
    rows.append({
        "system_id": "round-start-castle-protection",
        "mechanic_kind": "first-fifteen-seconds-castle-damage-immunity",
        "trigger": "positive-damage-event-targeting-team-castle",
        "parameters": {
            "protection_duration_seconds": 15,
            "requires_round_started": True,
            "protected_targets": "both-team-castle-units-cX[0]-and-cX[1]",
            "positive_damage_only": True,
            "damage_rewrite": 0,
            "round_timer_reset_minute": 0,
            "round_timer_reset_second": 0,
            "round_timer_tick_seconds": 1,
        },
        "related_rawcode_ids": [],
        "source_functions": [
            "YF", "isCastleProtected", "completeRoundStart", "yd",
            "DamageListener_addListener_CastleProtection_onEvent_addListener_CastleProtection",
        ],
        "evidence_kind": "exact-round-timer-reset-tick-protection-predicate-and-current-damage-rewrite",
        "byte_offset": min(
            castle_protect_init_start, castle_protect_pred_start, round_start_start,
            round_tick_start, castle_protect_damage_start,
        ),
    })

    # Eye of Corruption. Corrupted race setup installs one global listener and
    # records B00Q plus a 1.12 multiplier. Any positive non-attack damage event
    # whose target currently has that buff rewrites the current damage instance
    # to 112% of its prior value. The native A02C aura supplies the separate -6
    # armor component; this row preserves the script-only spell vulnerability.
    corrupted_setup_start, corrupted_setup_source, corrupted_setup_tokens = source("nK")
    corrupted_damage_start, corrupted_damage_source, corrupted_damage_tokens = source(
        "DamageListener_addListener_RaceCorrupted_onEvent_addListener_RaceCorrupted"
    )
    if b"B1=1110454353 A1=1.12" not in corrupted_setup_source:
        raise ValueError("Eye of Corruption buff/damage multiplier constants changed")
    if "DamageEvent_addListener" not in corrupted_setup_tokens:
        raise ValueError("Eye of Corruption damage-listener registration changed")
    if not {
        "DamageEvent_getTarget", "DamageEvent_getAmount", "DamageEvent_getType",
        "unit_getAbilityLevel", "DamageInstance_DamageInstance_setAmount",
    }.issubset(corrupted_damage_tokens):
        raise ValueError("Eye of Corruption damage listener calls changed")
    for fragment in (
        b"DamageEvent_getAmount()>0.",
        b"not(DamageEvent_getType()==0)",
        b"unit_getAbilityLevel(pmn,B1)>0",
        b"qmn=(DamageEvent_getAmount()*A1)",
        b"DamageInstance_DamageInstance_setAmount(khb,qmn)",
    ):
        if fragment not in corrupted_damage_source:
            raise ValueError("Eye of Corruption damage listener semantics changed")
    rows.append({
        "system_id": "corrupted-eye-of-corruption-spell-vulnerability",
        "mechanic_kind": "buff-marker-non-attack-current-damage-multiplier",
        "trigger": "positive-non-attack-damage-event-target-with-B00Q",
        "parameters": {
            "source_building_unit_id": 1747989588,
            "aura_ability_id": 1093677635,
            "target_buff_id": 1110454353,
            "positive_damage_only": True,
            "excluded_damage_event_type": 0,
            "excluded_damage_event_semantics": "attack-damage",
            "damage_multiplier": 1.12,
            "extra_damage_fraction": 0.12,
            "modifies_current_damage_instance": True,
            "requires_target_buff_presence": True,
            "multiple_source_buildings_do_not_stack_script_multiplier": True,
        },
        "related_rawcode_ids": [1747989588, 1093677635, 1110454353],
        "source_functions": [
            "nK", "DamageListener_addListener_RaceCorrupted_onEvent_addListener_RaceCorrupted",
        ],
        "evidence_kind": "exact-race-setup-listener-registration-buff-gate-and-current-damage-rewrite",
        "byte_offset": min(corrupted_setup_start, corrupted_damage_start),
    })

    # Obelisk of Light. Construction creates one persistent invisible spell
    # carrier at the building position with A000 Phoenix Fire. Positive damage
    # caused by that carrier removes native positive/negative buffs and an exact
    # list of Castle Fight persistent enchantment/bonus abilities from the hit
    # target. Building death or leave removes the carrier.
    obelisk_init_start, obelisk_init_source, obelisk_init_tokens = source("TI")
    obelisk_start_start, obelisk_start_source, obelisk_start_tokens = source("startObeliskOfLight")
    obelisk_stop_start, obelisk_stop_source, obelisk_stop_tokens = source("stopObeliskOfLight")
    obelisk_listener_start, obelisk_listener_source, obelisk_listener_tokens = source(
        "CallbackSingle_doAfter_ObeliskOfLight_call_doAfter_ObeliskOfLight"
    )
    obelisk_construct_start, obelisk_construct_source, obelisk_construct_tokens = source(
        "EventListener_add_doAfter_ObeliskOfLight_onEvent_add_doAfter_ObeliskOfLight"
    )
    obelisk_death_start, obelisk_death_source, obelisk_death_tokens = source(
        "EventListener_add_doAfter_ObeliskOfLight_onEvent_add_doAfter_ObeliskOfLight1"
    )
    obelisk_leave_start, obelisk_leave_source, obelisk_leave_tokens = source("code__onLeave_doAfter_ObeliskOfLight")
    obelisk_damage_start, obelisk_damage_source, obelisk_damage_tokens = source(
        "DamageListener_addListener_doAfter_ObeliskOfLight_onEvent_addListener_doAfter_ObeliskOfLight"
    )
    if b"O7=1747988533 N7=1093677104" not in obelisk_init_source:
        raise ValueError("Obelisk of Light unit/effect ability constants changed")
    if b"h2q=persistentDummyCarrierWithAbility(unit_getOwner(g2q),N7,unit_getPos(g2q))" not in obelisk_start_source:
        raise ValueError("Obelisk of Light persistent carrier creation changed")
    if b"P7:HashMap_put(__wurst_objectToIndex(g2q),__wurst_objectToIndex(h2q))" not in obelisk_start_source:
        raise ValueError("Obelisk of Light building-to-carrier tracking changed")
    if b"P7:HashMap_remove(__wurst_objectToIndex(i2q))" not in obelisk_stop_source or b"__wurst_safe_RemoveUnit(l2q)" not in obelisk_stop_source:
        raise ValueError("Obelisk of Light carrier cleanup changed")
    if not {"EVENT_PLAYER_UNIT_CONSTRUCT_FINISH", "EVENT_PLAYER_UNIT_DEATH", "DamageEvent_addListener"}.issubset(obelisk_listener_tokens):
        raise ValueError("Obelisk of Light lifecycle/damage listeners changed")
    if b"if(unit_getTypeId(s8m)==O7)then startObeliskOfLight(s8m)" not in obelisk_construct_source:
        raise ValueError("Obelisk of Light construction startup changed")
    if b"unit_getTypeId(v8m)==O7))then stopObeliskOfLight(v8m)" not in obelisk_death_source:
        raise ValueError("Obelisk of Light death cleanup changed")
    if b"stopObeliskOfLight(getEnterLeaveUnit())" not in obelisk_leave_source:
        raise ValueError("Obelisk of Light leave cleanup changed")
    removed_ability_ids = [
        1093679171, 1093679172, 1093679174,
        1093677890, 1093677889, 1093677892, 1093679180,
        1093678409, 1093678679, 1093679181, 1093678681, 1093679186, 1093678680,
        1093679441, 1093679442, 1093683286,
        1093682231, 1093682739, 1093682737, 1093677905, 1093682740,
        1093679436, 1093678676, 1093678677, 1093683033,
    ]
    if not {str(value) for value in removed_ability_ids}.issubset(obelisk_damage_tokens):
        raise ValueError("Obelisk of Light explicit cleanse ability set changed")
    if b"DamageEvent_getAmount()>0." not in obelisk_damage_source or b"unit_hasAbility(y8m,N7)" not in obelisk_damage_source:
        raise ValueError("Obelisk of Light cleanse damage-source gate changed")
    if b"__wurst_safe_UnitRemoveBuffs(A8m,true,true)" not in obelisk_damage_source:
        raise ValueError("Obelisk of Light native buff cleanse changed")
    for ability_id in removed_ability_ids:
        if f"unit_removeAbility(A8m,{ability_id})".encode() not in obelisk_damage_source:
            raise ValueError(f"Obelisk of Light cleanse lost ability {ability_id}")
    rows.append({
        "system_id": "obelisk-of-light-cleansing-light",
        "mechanic_kind": "persistent-carrier-auto-attack-damage-triggered-full-cleanse",
        "trigger": "building-construction-plus-carrier-damage-event",
        "parameters": {
            "building_unit_id": 1747988533,
            "carrier_effect_ability_id": 1093677104,
            "one_carrier_per_live_building": True,
            "carrier_owner": "building-owner",
            "carrier_position": "building-position",
            "carrier_removed_on_building_death": True,
            "carrier_removed_on_building_leave": True,
            "cleanse_requires_positive_damage": True,
            "cleanse_requires_damage_source_has_effect_ability": True,
            "remove_native_positive_and_negative_buffs": True,
            "unit_remove_buffs_positive": True,
            "unit_remove_buffs_negative": True,
            "removed_persistent_ability_ids": removed_ability_ids,
        },
        "related_rawcode_ids": [1747988533, 1093677104, *removed_ability_ids],
        "source_functions": [
            "TI", "startObeliskOfLight", "stopObeliskOfLight",
            "CallbackSingle_doAfter_ObeliskOfLight_call_doAfter_ObeliskOfLight",
            "EventListener_add_doAfter_ObeliskOfLight_onEvent_add_doAfter_ObeliskOfLight",
            "EventListener_add_doAfter_ObeliskOfLight_onEvent_add_doAfter_ObeliskOfLight1",
            "code__onLeave_doAfter_ObeliskOfLight",
            "DamageListener_addListener_doAfter_ObeliskOfLight_onEvent_addListener_doAfter_ObeliskOfLight",
        ],
        "evidence_kind": "exact-persistent-carrier-lifecycle-native-phoenix-fire-and-damage-cleanse-list",
        "byte_offset": min(
            obelisk_init_start, obelisk_start_start, obelisk_stop_start, obelisk_listener_start,
            obelisk_construct_start, obelisk_death_start, obelisk_leave_start, obelisk_damage_start,
        ),
    })

    # Blood Fiend procedural generation. The trained n00L carrier is first
    # replaced by one of six statistically equivalent body rawcodes, then six
    # independent random trait groups add abilities. Preserve the branch
    # probabilities explicitly so a native implementation need not emulate Lua
    # control flow just to reproduce the distribution.
    body_init_start, body_init_source, body_init_tokens = source("EE")
    body_start, body_source, body_tokens = source("rollBody")
    fiend_start, fiend_source, fiend_tokens = source("randomizeBloodFiend")
    train_start, train_source, train_tokens = source("onUnitTrained")
    body_ids = [1848651853, 1848651858, 1848651857, 1848651854, 1848651855, 1848651856]
    if not {str(value) for value in body_ids}.issubset(body_init_tokens):
        raise ValueError("Blood Fiend body rawcode table changed")
    if b"qAb=1848651853 pAb=1848651858" not in body_init_source:
        raise ValueError("Blood Fiend rare body assignments changed")
    if not {"5", "14", "GetRandomInt", "replaceUnitWithType"}.issubset(body_tokens):
        raise ValueError("Blood Fiend body roll changed")
    if b"GetRandomInt(0,3)" not in body_source:
        raise ValueError("Blood Fiend common body selection changed")
    trait_ids = [
        1093677113, 1093677382, 1093677634, 1093677378, 1093677146, 1093678924,
        1093677141, 1093677140, 1093678410, 1093677377, 1093677142, 1093677124,
        1093677397, 1093677360, 1093677392, 1093677110, 1093677394, 1093677362,
    ]
    if not {str(value) for value in trait_ids}.issubset(fiend_tokens):
        raise ValueError("Blood Fiend trait ability table changed")
    if b"unit_getAbilityLevel(cgs,1093678922)>0" not in train_source or b"randomizeBloodFiend(cgs)" not in train_source:
        raise ValueError("Blood Fiend train-time randomization marker changed")
    rows.append({
        "system_id": "blood-fiend-randomization",
        "mechanic_kind": "body-replacement-plus-independent-random-trait-groups",
        "trigger": "unit-train-finish-with-A07J-marker",
        "parameters": {
            "production_carrier_unit_id": 1848651852,
            "randomization_marker_ability_id": 1093678922,
            "body_distribution": [
                {"unit_id": 1848651853, "probability_percent": 5.0},
                {"unit_id": 1848651858, "probability_percent": 9.0},
                {"unit_id": 1848651857, "probability_percent": 21.5},
                {"unit_id": 1848651854, "probability_percent": 21.5},
                {"unit_id": 1848651855, "probability_percent": 21.5},
                {"unit_id": 1848651856, "probability_percent": 21.5},
            ],
            "trait_groups": [
                {"group": 1, "outcomes": [
                    {"ability_id": 1093677113, "probability_percent": 20},
                    {"ability_id": 1093677382, "probability_percent": 35},
                    {"ability_id": None, "probability_percent": 45},
                ]},
                {"group": 2, "outcomes": [
                    {"ability_id": 1093677634, "probability_percent": 18},
                    {"ability_id": 1093677378, "probability_percent": 12},
                    {"ability_id": 1093677146, "probability_percent": 3},
                    {"ability_id": 1093678924, "probability_percent": 10},
                    {"ability_id": None, "probability_percent": 57},
                ]},
                {"group": 3, "outcomes": [
                    {"ability_id": 1093677141, "probability_percent": 10},
                    {"ability_id": 1093677140, "probability_percent": 22},
                    {"ability_id": 1093678410, "probability_percent": 3},
                    {"ability_id": None, "probability_percent": 65},
                ]},
                {"group": 4, "outcomes": [
                    {"ability_id": 1093677377, "probability_percent": 25},
                    {"ability_id": None, "probability_percent": 75},
                ]},
                {"group": 5, "outcomes": [
                    {"ability_id": 1093677142, "probability_percent": 15},
                    {"ability_id": 1093677124, "probability_percent": 25},
                    {"ability_id": None, "probability_percent": 60},
                ]},
                {"group": 6, "outcomes": [
                    {"ability_id": 1093677397, "probability_percent": 8},
                    {"ability_id": 1093677360, "probability_percent": 8},
                    {"ability_id": 1093677392, "probability_percent": 8},
                    {"ability_id": 1093677110, "probability_percent": 10},
                    {"ability_id": 1093677394, "probability_percent": 10},
                    {"ability_id": 1093677362, "probability_percent": 3},
                    {"ability_id": None, "probability_percent": 53},
                ]},
            ],
        },
        "related_rawcode_ids": [1848651852, 1093678922, *body_ids, *trait_ids],
        "source_functions": ["EE", "rollBody", "randomizeBloodFiend", "onUnitTrained"],
        "evidence_kind": "exact-body-table-roll-thresholds-and-independent-trait-branches",
        "byte_offset": min(body_init_start, body_start, fiend_start, train_start),
    })

    return rows


def _enclosing_named_function(
    functions: list[dict[str, object]],
    byte_offset: int,
) -> str:
    containing = [
        function
        for function in functions
        if int(function["start"]) <= byte_offset < int(function["end"])
    ]
    if not containing:
        return "<top-level>"
    # Named nested functions are possible. The one with the latest start is the
    # innermost named lexical scope containing this source offset.
    return str(max(containing, key=lambda function: int(function["start"]))["name"])


def analyze_lua(data: bytes, known_rawcodes: set[int]) -> dict[str, object]:
    """Return lexical functions/calls/rawcode sites and runtime mutation sites."""
    stream = TokenStream(iter_lua_tokens(data))
    blocks: list[Block] = []
    parens: list[str | None] = []
    pending_call: str | None = None

    functions: list[dict[str, object]] = []
    active_named_functions: list[dict[str, object]] = []
    calls: Counter[str] = Counter()
    call_edges: Counter[tuple[str, str]] = Counter()
    rawcode_sites: list[dict[str, object]] = []
    runtime_mutators: list[dict[str, object]] = []

    while (token := stream.pop()) is not None:
        if token.kind == "ident":
            keyword = token.text
            if keyword == "function":
                name, opening = _parse_function_name(stream)
                block = Block("function", name=name, start=token.start)
                blocks.append(block)
                if name is not None:
                    record = {
                        "name": name,
                        "start": token.start,
                        "end": None,
                    }
                    functions.append(record)
                    active_named_functions.append(record)
                if opening is not None:
                    if opening.kind != "symbol" or opening.text != "(":
                        raise ValueError(f"unexpected Lua function declaration token at byte {opening.start}: {opening.text!r}")
                    parens.append(None)
                pending_call = None
                continue

            if keyword == "if":
                blocks.append(Block("if", start=token.start))
                continue
            if keyword in ("for", "while"):
                blocks.append(Block(keyword, start=token.start, awaiting_do=True))
                continue
            if keyword == "do":
                if blocks and blocks[-1].kind in ("for", "while") and blocks[-1].awaiting_do:
                    blocks[-1].awaiting_do = False
                else:
                    blocks.append(Block("do", start=token.start))
                continue
            if keyword == "repeat":
                blocks.append(Block("repeat", start=token.start))
                continue
            if keyword == "until":
                if not blocks or blocks[-1].kind != "repeat":
                    raise ValueError(f"unexpected Lua until at byte {token.start}")
                blocks.pop()
                continue
            if keyword == "end":
                if not blocks:
                    raise ValueError(f"unexpected Lua end at byte {token.start}")
                ended = blocks.pop()
                if ended.kind == "repeat":
                    raise ValueError(f"Lua repeat block closed by end at byte {token.start}")
                if ended.kind == "function" and ended.name is not None:
                    if not active_named_functions or active_named_functions[-1]["name"] != ended.name:
                        raise ValueError(f"Lua function scope mismatch at byte {token.start}")
                    active_named_functions.pop()["end"] = token.end
                continue

            # Other reserved words cannot form callable names. Treating them as
            # ordinary identifiers would create fake calls such as `return(...)`.
            if keyword in {
                "and", "break", "else", "elseif", "false", "goto", "in", "local",
                "nil", "not", "or", "return", "then", "true",
            }:
                continue

            parts = [keyword]
            while True:
                separator = stream.peek()
                member = stream.peek(1)
                if (
                    separator is None
                    or member is None
                    or separator.kind != "symbol"
                    or separator.text not in (".", ":")
                    or member.kind != "ident"
                ):
                    break
                parts.append(stream.pop().text)
                parts.append(stream.pop().text)
            callee = "".join(parts)
            if (opening := stream.peek()) is not None and opening.kind == "symbol" and opening.text == "(":
                caller = _current_function(blocks)
                calls[callee] += 1
                call_edges[(caller, callee)] += 1
                is_mutator, normalized = _is_runtime_mutator(callee)
                if is_mutator:
                    runtime_mutators.append({
                        "callee": callee,
                        "normalized_callee": normalized,
                        "byte_offset": token.start,
                        "function": caller,
                    })
                pending_call = callee
            else:
                pending_call = None
            continue

        if token.kind == "number" and token.integer_value in known_rawcodes:
            rawcode_sites.append({
                "rawcode_integer": token.integer_value,
                "byte_offset": token.start,
                "function": _current_function(blocks),
                "call": next((call for call in reversed(parens) if call is not None), ""),
            })
            pending_call = None
            continue

        if token.kind == "symbol":
            if token.text == "(":
                parens.append(pending_call)
            elif token.text == ")":
                if not parens:
                    raise ValueError(f"unexpected Lua ) at byte {token.start}")
                parens.pop()
            pending_call = None

    if blocks:
        kinds = ", ".join(block.kind for block in blocks[-5:])
        raise ValueError(f"unterminated Lua blocks: {kinds}")
    if parens:
        raise ValueError("unterminated Lua parenthesis stack")
    if any(function["end"] is None for function in functions):
        raise ValueError("named Lua function missing lexical end")

    function_rawcodes: Counter[tuple[str, int]] = Counter(
        (site["function"], site["rawcode_integer"]) for site in rawcode_sites
    )
    rawcodes_by_function: dict[str, set[int]] = defaultdict(set)
    for (function, rawcode), _count in function_rawcodes.items():
        rawcodes_by_function[function].add(rawcode)
    for site in runtime_mutators:
        site["direct_map_rawcodes"] = sorted(rawcodes_by_function.get(site["function"], ()))

    protected_ability_fields = _extract_protected_ability_fields(data, functions)
    jass_add_protected_fields = _extract_jass_add_protected_fields(data, functions)
    _cross_check_protected_ability_fields(protected_ability_fields, jass_add_protected_fields)
    unit_object_metadata, unit_object_metadata_fingerprint = _extract_unit_object_metadata(data, functions)
    unit_object_upgrades = _extract_unit_object_upgrades(data, functions)
    race_buildings = _extract_race_buildings(data, functions, call_edges)
    income_factor_constants = _extract_income_factor_constants(data, functions)
    race_building_semantics = _extract_race_building_semantics(
        data, functions, race_buildings, income_factor_constants
    )
    element_building_buckets = _extract_element_building_buckets(data, functions)
    effective_unit_stats = _extract_effective_unit_stats(data, functions)
    protected_unit_stats = _extract_protected_unit_stats(data, functions)

    rawcode_mutator_traces, resolved_call_edges = _rawcode_mutator_traces(
        functions,
        call_edges,
        function_rawcodes,
        runtime_mutators,
    )
    function_aliases, function_value_arguments = _function_value_links(data, functions)
    protected_filter_bindings = _extract_protected_filter_bindings(data, functions)
    production_unit_special_mechanics = _extract_production_unit_special_mechanics(
        data, functions, protected_filter_bindings
    )
    building_improvement_spawn_mechanics = _extract_building_improvement_spawn_mechanics(data, functions)
    runtime_system_mechanics = _extract_runtime_system_mechanics(data, functions)
    castle_item_mechanics = _extract_castle_item_mechanics(data, functions, function_aliases)
    building_spell_registrations = _extract_building_spell_registrations(data, functions, function_aliases)
    unit_spell_registrations = _extract_unit_spell_registrations(data, functions, function_aliases)
    unit_spell_mechanics = _extract_unit_spell_mechanics(
        data,
        functions,
        unit_spell_registrations,
        function_aliases,
        call_edges,
        function_rawcodes,
    )
    building_spell_evidence = _extract_building_spell_evidence(
        data,
        functions,
        building_spell_registrations,
        function_aliases,
        call_edges,
        function_rawcodes,
    )
    corpse_building_mechanics = _extract_corpse_building_mechanics(data, functions, building_spell_registrations)
    building_spell_mechanics = _extract_building_spell_mechanics(
        data, functions, building_spell_registrations, corpse_building_mechanics, protected_filter_bindings
    )
    protected_perk_registry_audit = _extract_protected_perk_registry_audit(data, functions)
    perk_mechanics = _extract_perk_mechanics(data, functions, protected_perk_registry_audit)
    runtime_ai_mechanics = _extract_runtime_ai_mechanics(data, functions, protected_filter_bindings)
    runtime_session_mechanics = _extract_runtime_session_mechanics(data, functions)
    runtime_mode_mechanics = _extract_runtime_mode_mechanics(data, functions, function_aliases, call_edges)
    runtime_campaign_mechanics = _extract_runtime_campaign_mechanics(data, functions)
    runtime_draft_mechanics = _extract_runtime_draft_mechanics(data, functions)
    damage_listener_coverage = _extract_damage_listener_coverage(
        functions,
        production_unit_special_mechanics,
        runtime_system_mechanics,
        building_spell_mechanics,
        perk_mechanics,
        runtime_ai_mechanics,
        protected_perk_registry_audit,
    )
    action_watch_coverage = _extract_action_watch_coverage(
        functions,
        production_unit_special_mechanics,
        runtime_system_mechanics,
        building_spell_mechanics,
        perk_mechanics,
        runtime_ai_mechanics,
        runtime_session_mechanics,
        runtime_mode_mechanics,
        runtime_campaign_mechanics,
        runtime_draft_mechanics,
        protected_perk_registry_audit,
    )
    callback_periodic_coverage = _extract_callback_periodic_coverage(
        functions,
        production_unit_special_mechanics,
        runtime_system_mechanics,
        building_spell_mechanics,
        perk_mechanics,
        runtime_ai_mechanics,
        runtime_session_mechanics,
        runtime_mode_mechanics,
        runtime_campaign_mechanics,
        runtime_draft_mechanics,
        unit_spell_mechanics,
        protected_perk_registry_audit,
    )
    event_listener_coverage = _extract_event_listener_coverage(
        functions,
        call_edges,
        production_unit_special_mechanics,
        runtime_system_mechanics,
        building_spell_mechanics,
        perk_mechanics,
        runtime_ai_mechanics,
        runtime_session_mechanics,
        runtime_mode_mechanics,
        runtime_campaign_mechanics,
        protected_perk_registry_audit,
    )
    for reference in function_value_arguments:
        reference["function"] = _enclosing_named_function(
            functions,
            int(reference["byte_offset"]),
        )

    return {
        "functions": functions,
        "calls": calls,
        "call_edges": call_edges,
        "resolved_call_edges": resolved_call_edges,
        "rawcode_sites": rawcode_sites,
        "function_rawcodes": function_rawcodes,
        "runtime_mutators": runtime_mutators,
        "rawcode_mutator_traces": rawcode_mutator_traces,
        "protected_ability_fields": protected_ability_fields,
        "jass_add_protected_fields": jass_add_protected_fields,
        "unit_object_metadata": unit_object_metadata,
        "unit_object_metadata_fingerprint": unit_object_metadata_fingerprint,
        "unit_object_upgrades": unit_object_upgrades,
        "race_buildings": race_buildings,
        "income_factor_constants": income_factor_constants,
        "race_building_semantics": race_building_semantics,
        "element_building_buckets": element_building_buckets,
        "effective_unit_stats": effective_unit_stats,
        "protected_unit_stats": protected_unit_stats,
            "function_aliases": function_aliases,
        "function_value_arguments": function_value_arguments,
        "protected_filter_bindings": protected_filter_bindings,
        "production_unit_special_mechanics": production_unit_special_mechanics,
        "building_improvement_spawn_mechanics": building_improvement_spawn_mechanics,
        "runtime_system_mechanics": runtime_system_mechanics,
        "castle_item_mechanics": castle_item_mechanics,
        "building_spell_registrations": building_spell_registrations,
        "building_spell_evidence": building_spell_evidence,
        "unit_spell_registrations": unit_spell_registrations,
        "unit_spell_mechanics": unit_spell_mechanics,
        "corpse_building_mechanics": corpse_building_mechanics,
        "building_spell_mechanics": building_spell_mechanics,
        "protected_perk_registry_audit": protected_perk_registry_audit,
        "perk_mechanics": perk_mechanics,
        "runtime_ai_mechanics": runtime_ai_mechanics,
        "runtime_session_mechanics": runtime_session_mechanics,
        "runtime_mode_mechanics": runtime_mode_mechanics,
        "runtime_campaign_mechanics": runtime_campaign_mechanics,
        "runtime_draft_mechanics": runtime_draft_mechanics,
        "damage_listener_coverage": damage_listener_coverage,
        "action_watch_coverage": action_watch_coverage,
        "callback_periodic_coverage": callback_periodic_coverage,
        "event_listener_coverage": event_listener_coverage,
    }
