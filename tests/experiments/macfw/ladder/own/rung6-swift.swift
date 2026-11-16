// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// macfw ladder rung 6: rung 1 again, in Swift, as Xcode builds it today.
// The AppKit surface is the same; what this rung adds is the Swift API that
// lives inside Foundation and AppKit on current macOS (Darling issue 2059).
// Standard output as in rung 1: macfw-ready, macfw-click N text=...

import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    let field = NSTextField(string: "")
    let status = NSTextField(labelWithString: "Not clicked yet")
    var clicks = 0

    func applicationDidFinishLaunching(_ notification: Notification) {
        let main = NSMenu()
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "Quit Rung 6", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        main.addItem(withTitle: "", action: nil, keyEquivalent: "").submenu = appMenu
        NSApp.mainMenu = main

        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 480, height: 220),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable],
                          backing: .buffered, defer: false)
        window.title = "TUR macfw rung 6"
        let hello = NSTextField(labelWithString: "Hello from Swift")
        hello.font = .systemFont(ofSize: 20)
        field.placeholderString = "Type here"
        let button = NSButton(title: "Count", target: self, action: #selector(count(_:)))
        button.keyEquivalent = "\r"
        let stack = NSStackView(views: [hello, field, button, status])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        stack.edgeInsets = NSEdgeInsets(top: 20, left: 20, bottom: 20, right: 20)
        field.widthAnchor.constraint(equalToConstant: 300).isActive = true
        window.contentView = stack
        window.center()
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(field)
        NSApp.activate(ignoringOtherApps: true)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [self] in
            print("macfw-ready visible=\(window.isVisible ? 1 : 0) key=\(window.isKeyWindow ? 1 : 0)")
            fflush(stdout)
        }
        if let q = ProcessInfo.processInfo.environment["MACFW_QUIT_AFTER"], let s = Double(q) {
            DispatchQueue.main.asyncAfter(deadline: .now() + s) { NSApp.terminate(nil) }
        }
    }

    @objc func count(_ sender: Any?) {
        clicks += 1
        status.stringValue = "Clicked \(clicks) times"
        print("macfw-click \(clicks) text=\(field.stringValue)")
        fflush(stdout)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

MainActor.assumeIsolated {
    let app = NSApplication.shared
    let delegate = AppDelegate()
    app.delegate = delegate
    _ = app.setActivationPolicy(.regular)
    app.run()
}
