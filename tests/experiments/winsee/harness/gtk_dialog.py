#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""A plain, well-labelled GTK 3 dialog for the vision test of the winsee
experiment (docs/reports/exp-winsee-1.md): fields, check boxes, radio
buttons, a drop-down list and buttons, each with a name the accessibility
tree reports. It does nothing; linux_labels.py photographs it and reads
where its controls are.
"""

import sys

import gi

gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

TITLE = "Network connection"


def main():
    GLib.set_prgname("tur-winsee-gtk")
    GLib.set_application_name("TUR winsee GTK dialog")
    window = Gtk.Window(title=TITLE)
    window.set_default_size(560, 420)
    window.connect("destroy", Gtk.main_quit)
    grid = Gtk.Grid(column_spacing=12, row_spacing=10, margin=18)
    window.add(grid)

    def field(row, text, value):
        label = Gtk.Label(label=text, xalign=0)
        entry = Gtk.Entry(hexpand=True, text=value)
        label.set_mnemonic_widget(entry)
        entry.get_accessible().set_name(text)
        grid.attach(label, 0, row, 1, 1)
        grid.attach(entry, 1, row, 3, 1)

    field(0, "Connection name", "Office")
    field(1, "Server address", "192.0.2.10")

    automatic = Gtk.CheckButton(label="Connect automatically", active=True)
    proxy = Gtk.CheckButton(label="Use a proxy")
    grid.attach(automatic, 1, 2, 3, 1)
    grid.attach(proxy, 1, 3, 3, 1)

    grid.attach(Gtk.Label(label="Addresses", xalign=0), 0, 4, 1, 1)
    dynamic = Gtk.RadioButton.new_with_label_from_widget(None, "Automatic")
    manual = Gtk.RadioButton.new_with_label_from_widget(dynamic, "Manual")
    grid.attach(dynamic, 1, 4, 1, 1)
    grid.attach(manual, 2, 4, 1, 1)

    security_label = Gtk.Label(label="Security", xalign=0)
    security = Gtk.ComboBoxText()
    for choice in ("None", "WPA2", "WPA3"):
        security.append_text(choice)
    security.set_active(1)
    security.get_accessible().set_name("Security")
    security_label.set_mnemonic_widget(security)
    grid.attach(security_label, 0, 5, 1, 1)
    grid.attach(security, 1, 5, 2, 1)

    metered_label = Gtk.Label(label="Metered connection", xalign=0)
    metered = Gtk.Switch(halign=Gtk.Align.START)
    metered.get_accessible().set_name("Metered connection")
    grid.attach(metered_label, 0, 6, 1, 1)
    grid.attach(metered, 1, 6, 1, 1)

    row = Gtk.Box(spacing=10, halign=Gtk.Align.END)
    for text in ("Test connection", "Advanced", "Help", "Cancel", "Apply"):
        row.pack_start(Gtk.Button(label=text), False, False, 0)
    grid.attach(row, 0, 8, 4, 1)

    window.show_all()
    Gtk.main()


if __name__ == "__main__":
    sys.exit(main())
