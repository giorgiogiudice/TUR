// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// macfw ladder rung 8: the smallest SwiftUI application, written by TUR: an
// `App` with one `WindowGroup`, a text, a text field, a button and a list.
// It separates "SwiftUI is missing" from the failures of the real SwiftUI
// applications above it on the ladder.
// Standard output: macfw-ready, macfw-click N text=..., macfw-select ITEM.

import SwiftUI

@main
struct Rung8App: App {
    init() {
        if let q = ProcessInfo.processInfo.environment["MACFW_QUIT_AFTER"], let s = Double(q) {
            DispatchQueue.main.asyncAfter(deadline: .now() + s) { NSApplication.shared.terminate(nil) }
        }
    }

    var body: some Scene {
        WindowGroup("TUR macfw rung 8") {
            ContentView()
                .frame(minWidth: 420, minHeight: 300)
        }
    }
}

struct ContentView: View {
    @State private var text = ""
    @State private var clicks = 0
    @State private var selection: String?
    private let items = ["Alpha", "Bravo", "Charlie", "Delta"]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Hello from SwiftUI").font(.title)
            TextField("Type here", text: $text).frame(width: 300)
            Button("Count") {
                clicks += 1
                print("macfw-click \(clicks) text=\(text)")
                fflush(stdout)
            }
            .keyboardShortcut(.defaultAction)
            Text(clicks == 0 ? "Not clicked yet" : "Clicked \(clicks) times")
            List(items, id: \.self, selection: $selection) { Text($0) }
                .onChange(of: selection) { _, new in
                    print("macfw-select \(new ?? "-")")
                    fflush(stdout)
                }
        }
        .padding(20)
        .onAppear {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                let w = NSApplication.shared.windows.first { $0.isVisible }
                print("macfw-ready visible=\(w != nil ? 1 : 0) key=\(w?.isKeyWindow == true ? 1 : 0)")
                fflush(stdout)
            }
        }
    }
}
