#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The guest agent without administrator rights, against a window of a
program that has them (docs/reports/exp-inwin-1.md, section 3): what it
can still read, and whether its actions arrive. Started by seer.py through
runas /trustlevel:0x20000.

    lower.py HWND OUT.json
"""

import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE))


def main():
    hwnd = int(sys.argv[1])
    out = pathlib.Path(sys.argv[2])
    findings = {}
    result = {"findings": findings}
    try:
        from probe import token_facts

        result["rights"] = token_facts()
        import win

        findings["title_read"] = bool(win.text_of(hwnd))
        findings["child_windows"] = len(win.children(hwnd))
        editor = next((c for c in win.children(hwnd) if win.class_of(c) in ("Edit", "RichEditD2DPT", "RICHEDIT50W")), None)
        findings["editor_window_found"] = editor is not None
        import uia

        auto = uia.Automation()
        try:
            read = auto.read(hwnd)
            findings["uia_elements"] = len(read["elements"])
            findings["uia_named"] = sum(1 for e in read["elements"] if e.get("name"))
            text = next((e for e in read["elements"] if e["role"] == "text"), None)
            if text is not None:
                try:
                    auto.set_value(text["id"], "from a lower agent")
                    findings["uia_set_value"] = "no error"
                except Exception as error:
                    findings["uia_set_value"] = f"{type(error).__name__}: {str(error)[:160]}"
                try:
                    findings["uia_value_after"] = (auto.value(text["id"]) or "")[:40]
                except Exception as error:
                    findings["uia_value_after"] = f"{type(error).__name__}: {str(error)[:160]}"
        except Exception as error:
            findings["uia_read"] = f"{type(error).__name__}: {str(error)[:200]}"
        if editor is not None:
            import ctypes

            findings["wm_gettext_before"] = win.remote_text(editor)[:40]
            done = ctypes.c_size_t(0)
            buffer = ctypes.create_unicode_buffer("typed by WM_SETTEXT")
            sent = win.SendMessageTimeoutW(editor, 0x0C, 0, ctypes.cast(buffer, ctypes.c_void_p), 2, 2000, ctypes.byref(done))
            findings["wm_settext_sent"] = bool(sent)
            findings["wm_settext_error"] = ctypes.get_last_error()
            findings["wm_gettext_after"] = win.remote_text(editor)[:40]
        findings["post_close"] = bool(win.PostMessageW(hwnd, 0x10, 0, 0))
    except Exception as error:
        result["error"] = f"{type(error).__name__}: {str(error)[:300]}"
    out.write_text(json.dumps(result, indent=1), encoding="utf-8")


if __name__ == "__main__":
    main()
