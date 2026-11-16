// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// macfw probe `swak`: AppKit and Foundation as a Swift application built
// with today's Xcode reaches them. The interesting part is not the values
// but whether the program loads at all: a Swift binary binds the Swift API
// that, on current macOS, lives inside Foundation and AppKit themselves
// (Darling issue 2059). The `load` group therefore touches one Swift-only
// API of each kind before anything else. Written from Apple's public
// documentation only.

import AppKit
import Foundation

@MainActor
func gLoad() {
    MF.check("swift_string") { "héllo".uppercased() + String("abc".reversed()) }
    MF.check("foundation_data") { Data([1, 2, 3]).base64EncodedString() }
    MF.check("foundation_url") { URL(fileURLWithPath: "/tmp/a b").appendingPathComponent("c").path }
    MF.check("foundation_json") {
        struct P: Codable { var a: Int; var b: [String] }
        let enc = JSONEncoder()
        enc.outputFormatting = .sortedKeys
        return String(data: try! enc.encode(P(a: 1, b: ["x"])), encoding: .utf8)!
    }
    MF.check("foundation_date_format") {
        Date(timeIntervalSince1970: 0).formatted(.iso8601)
    }
    MF.check("foundation_measurement") {
        let m = Measurement(value: 1500, unit: UnitLength.meters)
        return MF.f(m.converted(to: .kilometers).value)
    }
    MF.check("appkit_color") {
        let c = NSColor(srgbRed: 0.5, green: 0.25, blue: 1, alpha: 1)
        return String(format: "%.2f,%.2f,%.2f", c.redComponent, c.greenComponent, c.blueComponent)
    }
    MF.check("appkit_enum_names") { NSWindow.StyleMask.titled.union(.closable).rawValue.description }
    MF.check("concurrency") {
        final class Box: @unchecked Sendable { var v = "" }
        let box = Box()
        let sem = DispatchSemaphore(value: 0)
        Task.detached {
            async let a = Task { 20 }.value
            async let b = Task { 22 }.value
            box.v = String(await a + b)
            sem.signal()
        }
        sem.wait()
        return box.v
    }
}

final class Delegate: NSObject, NSWindowDelegate {
    func windowDidBecomeKey(_ n: Notification) { MF.log.append("didBecomeKey") }
    func windowWillClose(_ n: Notification) { MF.log.append("willClose") }
}

@MainActor
func gWindow() {
    let app = NSApplication.shared
    _ = app.setActivationPolicy(.regular)
    app.finishLaunching()
    let w = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 320, height: 200),
                     styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
    w.isReleasedWhenClosed = false
    let d = Delegate()
    w.delegate = d
    let stack = NSStackView(views: [NSTextField(labelWithString: "Swift"), NSButton(title: "OK", target: nil, action: nil)])
    stack.orientation = .vertical
    w.contentView = stack
    MF.log = []
    w.makeKeyAndOrderFront(nil)
    app.activate(ignoringOtherApps: true)
    MF.spin(0.5)
    MF.check("visible") { MF.b(w.isVisible) }
    MF.check("sequence") { MF.log.joined(separator: " ") }
    MF.check("content_size") { MF.size(w.contentView!.frame.size) }
    MF.check("arranged") { String(stack.arrangedSubviews.count) }
    MF.check("button_title") { (stack.arrangedSubviews[1] as! NSButton).title }
    w.performClose(nil)
    MF.spin(0.2)
    MF.check("close_sequence") { MF.log.joined(separator: " ") }
}

@MainActor
func gObservation() {
    // KVO through the Swift key-path API, implemented in Foundation's Swift
    // part on current macOS.
    final class Model: NSObject {
        @objc dynamic var count = 0
    }
    let m = Model()
    var seen: [Int] = []
    let obs = m.observe(\.count, options: [.initial, .new]) { _, ch in seen.append(ch.newValue ?? -1) }
    m.count = 3
    m.count = 4
    obs.invalidate()
    m.count = 5
    MF.check("kvo") { seen.map(String.init).joined(separator: ",") }
    var got = ""
    let tok = NotificationCenter.default.addObserver(forName: Notification.Name("org.tur.macfw"), object: nil,
                                                     queue: nil) { n in got = n.userInfo?["k"] as? String ?? "?" }
    NotificationCenter.default.post(name: Notification.Name("org.tur.macfw"), object: nil, userInfo: ["k": "v"])
    NotificationCenter.default.removeObserver(tok)
    MF.check("notification") { got }
}

exit(MainActor.assumeIsolated {
    MF.main("swak", [("load", gLoad), ("window", gWindow), ("observation", gObservation)])
})
