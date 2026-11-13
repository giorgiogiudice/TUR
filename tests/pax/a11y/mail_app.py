#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""A GTK 3 window that looks like a mail being written, for the accessibility
test of P@x (tests/pax/a11y/harness.py).

It has the controls the test needs: a "To" and a "Subject" field, a
"Message" text area, and three buttons: "Bold" (harmless), "Send" and
"Delete draft" (consequential: P@x must be refused them). It is a test
program; it sends nothing and deletes nothing.

Everything that happens to it is written, one line each, to the log file
named on the command line, so that the test can see what happened without
the accessibility bus:

    clicked:<label>          a button was pressed
    text:<field>:<content>   a field's text changed (newlines as \\n)

    mail_app.py LOG
"""

import sys

import gi

gi.require_version("Gtk", "3.0")
from gi.repository import GLib, Gtk  # noqa: E402

TITLE = "Compose — TUR test mail"


def main():
    log_path = sys.argv[1]
    log = open(log_path, "a", encoding="utf-8", buffering=1)

    def write(line):
        log.write(line.replace("\n", "\\n") + "\n")

    GLib.set_prgname("tur-a11y-mail")
    GLib.set_application_name("TUR test mail")

    window = Gtk.Window(title=TITLE)
    window.set_default_size(720, 480)
    window.connect("destroy", Gtk.main_quit)

    grid = Gtk.Grid(column_spacing=12, row_spacing=8, margin=16)
    window.add(grid)

    def field(row, label_text):
        label = Gtk.Label(label=label_text, xalign=0)
        entry = Gtk.Entry(hexpand=True)
        label.set_mnemonic_widget(entry)
        entry.get_accessible().set_name(label_text)
        entry.connect("changed", lambda widget: write(f"text:{label_text}:{widget.get_text()}"))
        grid.attach(label, 0, row, 1, 1)
        grid.attach(entry, 1, row, 3, 1)
        return entry

    field(0, "To")
    field(1, "Subject")

    message_label = Gtk.Label(label="Message", xalign=0, yalign=0)
    view = Gtk.TextView(hexpand=True, vexpand=True, wrap_mode=Gtk.WrapMode.WORD)
    view.get_accessible().set_name("Message")
    buffer = view.get_buffer()
    buffer.connect(
        "changed",
        lambda changed: write(
            "text:Message:" + changed.get_text(changed.get_start_iter(), changed.get_end_iter(), True)
        ),
    )
    scroller = Gtk.ScrolledWindow(hexpand=True, vexpand=True)
    scroller.add(view)
    grid.attach(message_label, 0, 2, 1, 1)
    grid.attach(scroller, 1, 2, 3, 1)

    status = Gtk.Label(label="Nothing has been sent.", xalign=0)
    grid.attach(status, 1, 4, 3, 1)

    def button(column, label_text):
        widget = Gtk.Button(label=label_text)

        def clicked(_widget):
            write(f"clicked:{label_text}")
            status.set_text(f"{label_text} was pressed.")

        widget.connect("clicked", clicked)
        grid.attach(widget, column, 3, 1, 1)
        return widget

    button(1, "Bold")
    button(2, "Send")
    button(3, "Delete draft")

    window.show_all()
    write("ready")
    Gtk.main()


if __name__ == "__main__":
    sys.exit(main())
