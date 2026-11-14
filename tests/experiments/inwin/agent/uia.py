# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""UI Automation on real Windows, through its COM client (comtypes), in the
node model the winsee bridge uses (docs/reports/exp-winsee-1.md), so that
one scoring function serves both. The whole tree of a window is read in one
request with a cache; acting uses the control patterns.

Experiment code (docs/reports/exp-inwin-1.md). The product's guest agent
would be a native program; what UI Automation exposes does not depend on
the client's language.
"""

import sys
import time

sys.coinit_flags = 0  # multithreaded apartment: no message pump needed

import comtypes  # noqa: E402
import comtypes.client  # noqa: E402

comtypes.client.GetModule("UIAutomationCore.dll")
from comtypes.gen import UIAutomationClient as U  # noqa: E402

CONTROL_TYPES = {
    50000: "push button", 50001: "calendar", 50002: "check box", 50003: "combo box", 50004: "text",
    50005: "link", 50006: "image", 50007: "list item", 50008: "list", 50009: "menu popup", 50010: "menu bar",
    50011: "menu item", 50012: "progress bar", 50013: "radio button", 50014: "scroll bar", 50015: "slider",
    50016: "spin button", 50017: "status bar", 50018: "page tab list", 50019: "page tab", 50020: "label",
    50021: "tool bar", 50022: "tool tip", 50023: "tree", 50024: "tree item", 50025: "custom", 50026: "group",
    50027: "thumb", 50028: "data grid", 50029: "list item", 50030: "text", 50031: "push button",
    50032: "window", 50033: "pane", 50034: "header", 50035: "table column header", 50036: "table",
    50037: "title bar", 50038: "separator", 50039: "semantic zoom", 50040: "app bar",
}

# Property identifiers (UIAutomationClient.h).
P_RECT, P_TYPE, P_NAME, P_ENABLED, P_AUTOID, P_CLASS, P_HWND = 30001, 30003, 30005, 30010, 30011, 30012, 30020
P_PASSWORD, P_OFFSCREEN, P_FRAMEWORK, P_FOCUSABLE, P_PID = 30019, 30022, 30024, 30009, 30002
P_HAS = {"press": 30031, "type": 30043, "select": 30036, "expand": 30028, "toggle": 30041, "legacy": 30090}
PATTERN = {"invoke": 10000, "value": 10002, "expand": 10005, "select": 10010, "toggle": 10015, "legacy": 10018, "text": 10014}


class Automation:
    def __init__(self):
        self.auto = comtypes.CoCreateInstance(U.CUIAutomation._reg_clsid_, interface=U.IUIAutomation, clsctx=comtypes.CLSCTX_INPROC_SERVER)
        self.live = {}
        self.reads = 0

    def _request(self, view):
        request = self.auto.CreateCacheRequest()
        for prop in [P_RECT, P_TYPE, P_NAME, P_ENABLED, P_AUTOID, P_CLASS, P_HWND, P_PASSWORD, P_OFFSCREEN, P_FRAMEWORK, P_FOCUSABLE] + list(P_HAS.values()):
            request.AddProperty(prop)
        request.TreeScope = 7  # the element, its children and descendants
        request.TreeFilter = self.auto.RawViewCondition if view == "raw" else self.auto.ControlViewCondition
        return request

    def read(self, hwnd, view="control", max_nodes=6000):
        """One window as {"elements": [...], "milliseconds": n}. Elements
        keep a live reference in self.live under their id, for acting."""
        began = time.perf_counter()
        # Identifiers are unique across reads, so one from an earlier read
        # still names the element it named then (or fails, if it is gone).
        self.reads += 1
        if len(self.live) > 60000:
            self.live = {}
        elements = []
        note = {}
        root = self.auto.ElementFromHandleBuildCache(hwnd, self._request(view))
        stack = [(root, 0, None)]
        while stack and len(elements) < max_nodes:
            element, depth, parent = stack.pop()
            identifier = f"u{self.reads}.{len(elements)}"
            node = self._node(element, identifier, depth, parent)
            elements.append(node)
            self.live[identifier] = element
            try:
                kids = element.GetCachedChildren()
            except comtypes.COMError:
                kids = None
            if kids:
                count = kids.Length
                for index in range(count - 1, -1, -1):
                    stack.append((kids.GetElement(index), depth + 1, identifier))
        if stack:
            note["truncated"] = True
        result = {"tier": "uia-" + view, "elements": elements, "milliseconds": round((time.perf_counter() - began) * 1000, 1)}
        result.update(note)
        return result

    def _node(self, element, identifier, depth, parent):
        def cached(prop, default=None):
            try:
                value = element.GetCachedPropertyValue(prop)
            except comtypes.COMError:
                return default
            return default if value is None else value

        kind = cached(P_TYPE, 0)
        node = {"id": identifier, "depth": depth, "role": CONTROL_TYPES.get(kind, f"type {kind}"), "name": cached(P_NAME, "") or ""}
        if parent is not None:
            node["parent"] = parent
        try:
            rect = element.CachedBoundingRectangle
            if rect.right > rect.left and rect.bottom > rect.top:
                node["screen"] = [rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top]
        except comtypes.COMError:
            pass
        states = []
        if cached(P_OFFSCREEN, False):
            states.append("offscreen")
        if not cached(P_ENABLED, True):
            states.append("disabled")
        if cached(P_PASSWORD, False):
            states.append("password")
            node["role"] = "password text"
        node["states"] = states
        node["actions"] = [name for name, prop in P_HAS.items() if cached(prop, False)]
        for key, prop in (("class", P_CLASS), ("automation_id", P_AUTOID), ("framework", P_FRAMEWORK)):
            value = cached(prop, "")
            if value:
                node[key] = value
        handle = cached(P_HWND, 0)
        if handle:
            node["hwnd"] = int(handle)
        return node

    # -- acting ---------------------------------------------------------

    def _pattern(self, identifier, name, interface):
        element = self.live[identifier]
        unknown = element.GetCurrentPattern(PATTERN[name])
        if not unknown:
            raise LookupError(f"the element offers no {name} pattern")
        return unknown.QueryInterface(interface)

    def press(self, identifier):
        try:
            self._pattern(identifier, "invoke", U.IUIAutomationInvokePattern).Invoke()
            return "invoke"
        except (LookupError, comtypes.COMError):
            self._pattern(identifier, "legacy", U.IUIAutomationLegacyIAccessiblePattern).DoDefaultAction()
            return "legacy default action"

    def set_value(self, identifier, text):
        self._pattern(identifier, "value", U.IUIAutomationValuePattern).SetValue(text)
        return "value"

    def value(self, identifier):
        try:
            return self._pattern(identifier, "value", U.IUIAutomationValuePattern).CurrentValue
        except (LookupError, comtypes.COMError):
            document = self._pattern(identifier, "text", U.IUIAutomationTextPattern).DocumentRange
            return document.GetText(-1)

    def select(self, identifier):
        self._pattern(identifier, "select", U.IUIAutomationSelectionItemPattern).Select()
        return "select"

    def selected(self, identifier):
        return bool(self._pattern(identifier, "select", U.IUIAutomationSelectionItemPattern).CurrentIsSelected)

    def toggle(self, identifier):
        pattern = self._pattern(identifier, "toggle", U.IUIAutomationTogglePattern)
        pattern.Toggle()
        return pattern.CurrentToggleState

    def expand(self, identifier):
        self._pattern(identifier, "expand", U.IUIAutomationExpandCollapsePattern).Expand()
        return "expand"

    def collapse(self, identifier):
        self._pattern(identifier, "expand", U.IUIAutomationExpandCollapsePattern).Collapse()
        return "collapse"

    def focus(self, identifier):
        self.live[identifier].SetFocus()
        return "focus"

    def element_at(self, x, y):
        """What UI Automation says is at a screen point: name, role, process."""
        point = U.tagPOINT(x, y)
        element = self.auto.ElementFromPoint(point)
        return {
            "name": element.CurrentName or "",
            "role": CONTROL_TYPES.get(element.CurrentControlType, "?"),
            "pid": element.CurrentProcessId,
        }
