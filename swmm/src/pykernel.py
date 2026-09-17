# SPDX-License-Identifier: GPL-3.0-or-later
"""StormSewer Python kernel.

Runs as a child process of the app and executes blocks of code in one
persistent namespace, framing every reply with sentinels so the caller never
has to work out where the output ended. A prompt-scraping REPL would force
exactly that guess -- which stream CPython writes ">>> " to when stdin is not
a terminal, and whether a continuation prompt is pending -- so the protocol is
made explicit here instead.

Protocol, UTF-8 over stdin/stdout:

    caller -> kernel    one or more lines of source, then a line that is
                        exactly END_OF_BLOCK
    kernel -> caller    whatever the code wrote, then a line that is exactly
                        DONE followed by " ok" or " error"

stderr is folded into stdout so ordering is preserved and the caller has a
single pipe to read. The kernel writes one DONE line at startup too, so the
caller can wait for a ready signal the same way it waits for a result.
"""

import ast
import contextlib
import os
import sys
import traceback

END_OF_BLOCK = "<<<STORMSEWER-EXEC>>>"
DONE = "<<<STORMSEWER-DONE>>>"


def _reconfigure_streams():
    """Force UTF-8. Windows still defaults these to a legacy code page, and a
    single em dash in a model name would otherwise take the kernel down."""
    # stdin uses utf-8-sig so a byte-order mark at the head of the stream is
    # consumed rather than parsed: several callers emit one, and Python treats
    # U+FEFF in source as an invalid non-printable character.
    try:
        sys.stdin.reconfigure(encoding="utf-8-sig", errors="replace")
    except (AttributeError, ValueError):
        pass
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass


def _bootstrap():
    """Build the namespace the session starts with.

    Paths arrive by environment rather than as generated source, so a model
    path containing quotes or backslashes cannot turn into broken code.
    """
    alr_dir = os.environ.get("STORMSEWER_ALR_DIR")
    if alr_dir and alr_dir not in sys.path:
        sys.path.insert(0, alr_dir)

    namespace = {"__name__": "__stormsewer__"}
    for name, var in (
        ("model", "STORMSEWER_PY_MODEL"),
        ("out", "STORMSEWER_PY_OUT"),
        ("rpt", "STORMSEWER_PY_RPT"),
    ):
        value = os.environ.get(var)
        namespace[name] = value or None
    return namespace


def _print_banner(namespace):
    print(f"StormSewer Python kernel — {sys.version.split()[0]}")
    bound = [name for name in ("model", "out", "rpt") if namespace.get(name)]
    if bound:
        for name in bound:
            print(f"  {name} = {namespace[name]!r}")
    else:
        print("  no model loaded yet — run one from the SWMM tab")


def _run_block(source, namespace):
    """Execute one block, echoing a trailing expression the way a prompt does.

    Returns True when the block completed without raising.
    """
    # A mark can also ride in on pasted code, mid-stream, where utf-8-sig on
    # the stream no longer helps.
    source = source.lstrip("﻿")
    try:
        tree = ast.parse(source, filename="<input>", mode="exec")
    except SyntaxError:
        # Syntax errors carry their own location; no kernel frames to strip.
        traceback.print_exc(file=sys.stdout)
        return False

    if not tree.body:
        return True

    last = tree.body[-1]
    try:
        if isinstance(last, ast.Expr):
            head = ast.Module(body=tree.body[:-1], type_ignores=[])
            exec(compile(head, "<input>", "exec"), namespace)
            value = eval(
                compile(ast.Expression(last.value), "<input>", "eval"), namespace
            )
            if value is not None:
                namespace["_"] = value
                print(repr(value))
        else:
            exec(compile(tree, "<input>", "exec"), namespace)
        return True
    except BaseException:  # noqa: BLE001 - a REPL reports everything, including SystemExit
        exc_type, exc, tb = sys.exc_info()
        # Drop this frame so the traceback starts in the user's own code.
        traceback.print_exception(exc_type, exc, tb.tb_next if tb else None,
                                  file=sys.stdout)
        return False


def main():
    _reconfigure_streams()
    namespace = _bootstrap()
    _print_banner(namespace)
    print(f"{DONE} ok", flush=True)

    lines = []
    while True:
        raw = sys.stdin.readline()
        if not raw:
            break  # the app closed the pipe
        line = raw.rstrip("\r\n")
        if line != END_OF_BLOCK:
            lines.append(line)
            continue

        source = "\n".join(lines)
        lines = []
        with contextlib.redirect_stderr(sys.stdout):
            ok = _run_block(source, namespace)
        print(f"{DONE} {'ok' if ok else 'error'}", flush=True)


if __name__ == "__main__":
    main()
