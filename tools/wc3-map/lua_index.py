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
            },
            (silver_handler_name, "addSilverGladeChargeForTeam", "OK", "PK", "NK", "resetSilverGladeCounterForTeam"),
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
        },
        ("createSnowveilSnow", "vec2_setSnow", "DamageListener_addListener_SnowveilFountain_onEvent_addListener_SnowveilFountain", "damageUnitsOnSnowInArea", "explodeSnowInArea", "FL"),
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
            },
            (thunder_handler, listener_name, "ForGroupCallback_forUnitsInRange_addListener_doAfter_ThunderpawSpire_callback_forUnitsInRange_addListener_doAfter_ThunderpawSpire"),
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
        "CallbackSingle_doAfter_ReengageRuntime_call_doAfter_ReengageRuntime1",
        "CallbackSingle_doAfter_doAfter_ReengageRuntime_call_doAfter_doAfter_ReengageRuntime1",
        "EventListener_add_doAfter_FixDefend_onEvent_add_doAfter_FixDefend",
        "CallbackSingle_doAfter_add_doAfter_FixDefend_call_doAfter_add_doAfter_FixDefend",
        "EventListener_add_FireElemental_onEvent_add_FireElemental",
        "setupFireSummon",
        "CallbackSingle_doAfter_FireElemental_call_doAfter_FireElemental",
    }
    if not required_map_functions.issubset(available_functions):
        return []

    rows: list[dict[str, object]] = []

    # Mountain Giant: every newly-entered e00F automatically gets a temporary
    # VTlt tree in front of it and is issued the native Grab Tree order. The
    # follow-up callback resumes attack after 1.6 s and schedules destruction
    # of that temporary destructable another 2 s later.
    enter_start, _ = require_tokens("onUnitEnteredMap", {"1697656902", "castNatureAttackTree"})
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

    # Greater Fire Elemental: WC3's native ANlm split engine owns the attack
    # counter/generation mechanics. Castle Fight adds a summon listener that
    # strips the summoned classification from h030 children and converts an
    # h030 child produced by another h030 into ordinary Fire Elemental h02Z.
    # Keep the native ANlm fields joined downstream from object data rather than
    # re-deriving engine internals from the listener.
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
        "trigger": "wc3-ANlm-split-and-summon-event",
        "parameters": {
            "split_ability_id": 1093682265,
            "first_split_child_unit_id": 1747989296,
            "nested_h030_child_replacement_unit_id": 1747989082,
            "nested_h030_child_replace_method": 2,
            "remove_summoned_type_from_h030_child": True,
            "post_child_setup_tracks_pathing": True,
            "post_child_setup_orders_attack": True,
            "post_child_attack_order_delay_seconds": 0.2,
            "native_split_state_machine": "Warcraft-ANlm",
        },
        "related_rawcode_ids": [1093682265, 1747989296, 1747989082],
        "source_functions": [
            "EventListener_add_FireElemental_onEvent_add_FireElemental",
            "setupFireSummon",
            "CallbackSingle_doAfter_FireElemental_call_doAfter_FireElemental",
        ],
        "evidence_kind": "native-ANlm-object-data-plus-exact-summon-listener",
        "byte_offset": fire_listener_start,
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
        "castle_item_mechanics": castle_item_mechanics,
        "building_spell_registrations": building_spell_registrations,
        "building_spell_evidence": building_spell_evidence,
        "unit_spell_registrations": unit_spell_registrations,
        "unit_spell_mechanics": unit_spell_mechanics,
        "corpse_building_mechanics": corpse_building_mechanics,
        "building_spell_mechanics": building_spell_mechanics,
    }
