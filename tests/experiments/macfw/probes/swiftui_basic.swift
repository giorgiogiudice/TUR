// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// macfw probe `sui`: SwiftUI layout and rendering, measured without fonts.
// Every view here is made of colours with fixed frames, so its size is
// arithmetic the documentation defines (stack spacing, padding, frames,
// alignment), and its pixels are colours the probe chose. Text appears only
// in `metric` checks.
//
// Darling has no SwiftUI today (issue 2059: "SwiftUI last"); this probe is
// here so that the first SwiftUI symbol Darling gains is measured, and so the
// SwiftUI rungs of the ladder have a font-free baseline. Written from Apple's
// public documentation only.

import AppKit
import SwiftUI

@MainActor
func fitting<V: View>(_ v: V) -> String {
    let h = NSHostingView(rootView: v)
    return MF.size(h.fittingSize)
}

@MainActor
func pixels<V: View>(_ v: V, _ points: [(Int, Int)]) -> String {
    let r = ImageRenderer(content: v)
    r.scale = 1
    guard let cg = r.cgImage else { return "(no image)" }
    let rep = NSBitmapImageRep(cgImage: cg)
    return points.map { p in
        guard let c = rep.colorAt(x: p.0, y: p.1)?.usingColorSpace(.sRGB) else { return "?" }
        return String(format: "%d,%d,%d", Int(c.redComponent * 255 + 0.5), Int(c.greenComponent * 255 + 0.5),
                      Int(c.blueComponent * 255 + 0.5))
    }.joined(separator: " ")
}

@MainActor
func gLayout() {
    let red = Color(.sRGB, red: 1, green: 0, blue: 0)
    let blue = Color(.sRGB, red: 0, green: 0, blue: 1)
    MF.check("vstack0") { fitting(VStack(spacing: 0) { red.frame(width: 100, height: 40); blue.frame(width: 60, height: 60) }) }
    MF.check("hstack10") { fitting(HStack(spacing: 10) { red.frame(width: 100, height: 40); blue.frame(width: 60, height: 60) }) }
    MF.check("padding8") { fitting(red.frame(width: 10, height: 10).padding(8)) }
    MF.check("padding_edges") { fitting(red.frame(width: 10, height: 10).padding(.leading, 5).padding(.top, 3)) }
    MF.check("zstack") { fitting(ZStack { red.frame(width: 30, height: 70); blue.frame(width: 50, height: 20) }) }
    MF.check("frame_min_max") { fitting(red.frame(minWidth: 40, maxWidth: 80, minHeight: 10, maxHeight: 10)) }
    MF.check("fixed_size_spacer") { fitting(HStack(spacing: 0) { red.frame(width: 10, height: 10); Spacer(minLength: 25); blue.frame(width: 10, height: 10) }) }
    MF.check("grid") {
        fitting(Grid(horizontalSpacing: 2, verticalSpacing: 3) {
            GridRow { red.frame(width: 10, height: 10); blue.frame(width: 20, height: 5) }
            GridRow { blue.frame(width: 15, height: 8); red.frame(width: 5, height: 5) }
        })
    }
    MF.check("default_stack_spacing_metric", "metric") { fitting(VStack { red.frame(width: 10, height: 10); red.frame(width: 10, height: 10) }) }
    MF.check("text_metric", "metric") { fitting(Text("Hello")) }
}

@MainActor
func gRender() {
    let red = Color(.sRGB, red: 1, green: 0, blue: 0)
    let blue = Color(.sRGB, red: 0, green: 0, blue: 1)
    MF.check("vstack_pixels", "num:2") {
        pixels(VStack(spacing: 0) { red.frame(width: 40, height: 20); blue.frame(width: 40, height: 20) }, [(20, 5), (20, 35)])
    }
    MF.check("overlay_alignment", "num:2") {
        pixels(red.frame(width: 40, height: 40).overlay(alignment: .bottomTrailing) { blue.frame(width: 10, height: 10) },
               [(5, 5), (35, 35)])
    }
    MF.check("opacity", "num:3") { pixels(ZStack { Color.white; red.opacity(0.5) }.frame(width: 10, height: 10), [(5, 5)]) }
}

@MainActor
final class Counter: ObservableObject {
    @Published var n = 0
}

struct CounterView: View {
    @ObservedObject var c: Counter
    var body: some View {
        HStack(spacing: 0) {
            ForEach(0..<c.n, id: \.self) { _ in Color(.sRGB, red: 0, green: 1, blue: 0).frame(width: 10, height: 10) }
        }
    }
}

@MainActor
func gState() {
    let c = Counter()
    let host = NSHostingView(rootView: CounterView(c: c))
    MF.check("initial") { MF.size(host.fittingSize) }
    c.n = 3
    MF.spin(0.1)
    MF.check("after_publish") { MF.size(host.fittingSize) }
    let app = NSApplication.shared
    _ = app.setActivationPolicy(.regular)
    app.finishLaunching()
    let w = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 200, height: 100), styleMask: [.titled],
                     backing: .buffered, defer: false)
    w.isReleasedWhenClosed = false
    w.contentView = host
    w.makeKeyAndOrderFront(nil)
    MF.spin(0.3)
    MF.check("hosted_in_window") { MF.b(host.window === w && w.isVisible) }
    MF.check("hosting_controller_class_is_view") { MF.b(NSHostingController(rootView: Text("x")).view is NSView) }
    w.close()
}

exit(MainActor.assumeIsolated {
    MF.main("sui", [("layout", gLayout), ("render", gRender), ("state", gState)])
})
