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
    effective_unit_stats = _extract_effective_unit_stats(data, functions)
    protected_unit_stats = _extract_protected_unit_stats(data, functions)

    rawcode_mutator_traces, resolved_call_edges = _rawcode_mutator_traces(
        functions,
        call_edges,
        function_rawcodes,
        runtime_mutators,
    )
    function_aliases, function_value_arguments = _function_value_links(data, functions)
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
        "effective_unit_stats": effective_unit_stats,
        "protected_unit_stats": protected_unit_stats,
        "function_aliases": function_aliases,
        "function_value_arguments": function_value_arguments,
    }
