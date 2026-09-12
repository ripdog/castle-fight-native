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

    rawcode_mutator_traces, resolved_call_edges = _rawcode_mutator_traces(
        functions,
        call_edges,
        function_rawcodes,
        runtime_mutators,
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
    }
