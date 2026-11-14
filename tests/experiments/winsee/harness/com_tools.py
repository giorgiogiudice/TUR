#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""From the type information of a COM automation object to typed tool
definitions P@x could be offered under policy (the wild idea of the winsee
experiment, docs/reports/exp-winsee-1.md).

`tools_from_typeinfo` takes what tur-winbridge's `com_typeinfo` answers and
returns tool definitions in the shape of TUR's tool manifests (a name, a
description, a class, a JSON schema of the input), with the class this
experiment proposes for each:

  class 0   a property read: it returns a value and takes plain arguments
  class 1   a method whose name says nothing consequential
  class 2   a method or a property write whose name matches the policy
            engine's tables of consequential words (delete, remove, send,
            install, run, format, ...) or that writes, saves, creates,
            copies or moves: refused today

A type library declares names and types. It does not say what a call does:
the class is a proposal from the name, the same heuristic the engine uses
for a button's label, and no stronger.
"""

import re

# The stems of services/tur-pax/src/patterns.rs, and what a file or a
# document object adds to them.
CONSEQUENTIAL = (
    "send resend post publish share upload transmit broadcast invit pay donat transfer withdraw deposit refund purchas buy "
    "order checkout subscri unsubscri reserv delet remov eras wipe destroy discard purg shred empty clear overwrit replac "
    "format partition reset restor revert rollback encrypt decrypt revok deactivat terminat unlink factory install uninstall "
    "reinstall upgrad downgrad updat flash firmware submit confirm accept agree sign login approv authori allow grant permit "
    "consent trust proceed continu finish complet verif register enrol vote terminal console shell command execut script "
    "macro sudo administrat run exec write save creat copy move renam kill quit close open print mail navigate regwrite regdelete"
).split()

SIMPLE_TYPES = {
    "string": {"type": "string", "maxLength": 2000},
    "boolean": {"type": "boolean"},
    "int8": {"type": "integer"},
    "uint8": {"type": "integer"},
    "int16": {"type": "integer"},
    "uint16": {"type": "integer"},
    "int32": {"type": "integer"},
    "uint32": {"type": "integer"},
    "int64": {"type": "integer"},
    "uint64": {"type": "integer"},
    "float": {"type": "number"},
    "double": {"type": "number"},
    "date": {"type": "number", "description": "An OLE automation date."},
    # A variant takes text, a number or a truth value here.
    "variant": {"type": ["string", "number", "boolean"]},
}


def words(name):
    return [word.lower() for word in re.findall(r"[A-Z]?[a-z]+|[A-Z]+(?![a-z])|\d+", name or "")]


def consequential(name):
    return any(word.startswith(stem) for word in words(name) for stem in CONSEQUENTIAL)


def schema_of(type_name):
    """The JSON schema of a parameter type, or None for one a tool cannot
    carry (an object, a pointer to a structure, an array)."""
    base = (type_name or "").rstrip("*").strip()
    if base in SIMPLE_TYPES:
        return dict(SIMPLE_TYPES[base])
    return None


def tools_from_typeinfo(info):
    library = info.get("library") or "COM"
    interface = info.get("interface") or "object"
    tools = []
    left_out = {"restricted_or_hidden": 0, "takes_or_needs_an_object": 0, "out_parameters": 0}
    by_class = {0: 0, 1: 0, 2: 0}
    for member in info.get("members", []):
        if member.get("restricted") or member.get("hidden"):
            left_out["restricted_or_hidden"] += 1
            continue
        properties = {}
        required = []
        usable = True
        for index, parameter in enumerate(member.get("params", [])):
            if parameter.get("retval"):
                continue
            if parameter.get("out") and not parameter.get("in"):
                left_out["out_parameters"] += 1
                usable = False
                break
            schema = schema_of(parameter.get("type"))
            if schema is None:
                left_out["takes_or_needs_an_object"] += 1
                usable = False
                break
            name = parameter.get("name") or f"argument{index + 1}"
            properties[name] = schema
            if not parameter.get("optional"):
                required.append(name)
        if not usable:
            continue
        kind = member.get("kind")
        if kind == "get":
            proposed = 0
        elif kind in ("put", "putref") or consequential(member.get("name")):
            proposed = 2
        else:
            proposed = 1
        by_class[proposed] += 1
        suffix = {"get": "_get", "put": "_set", "putref": "_set"}.get(kind, "")
        tools.append(
            {
                "name": f"com_{library}_{interface}_{member.get('name')}{suffix}".replace(".", "_"),
                "description": (member.get("doc") or f"{interface}.{member.get('name')} of the {library} automation library.")[:300],
                "class": proposed,
                "method": member.get("name"),
                "invoke": {"dispid": member.get("memid"), "kind": kind},
                "returns": member.get("returns"),
                "input_schema": {"type": "object", "properties": properties, "required": required, "additionalProperties": False},
            }
        )
    return {
        "schema_version": 1,
        "service": f"com:{info.get('progid')}",
        "note": "Proposed classes come from the member's kind and name only; a type library does not say what a call does.",
        "tools": tools,
        "left_out": left_out,
        "by_class": {str(key): value for key, value in by_class.items()},
    }
