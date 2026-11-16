// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// macfw: the Swift side of the probe framework (see mf.h for the protocol).
// The same JSON lines, the same "k" kinds; compiled into each Swift probe.
//
// Swift has no @try: an Objective-C exception inside a Swift check ends the
// process, and the runner records the crash and the last check that started
// (each check prints a "begin" marker on standard error first).
//
// Everything runs on the main thread, on the main actor: the probes call
// AppKit, which is main-actor isolated. A probe's top level is
//     exit(MainActor.assumeIsolated { MF.main("prog", [...]) })
// (macOS 14 and later).

import Foundation

@MainActor
enum MF {
    static var prog = "?"
    static var group = "?"
    static var log: [String] = []

    nonisolated static func escape(_ s: String) -> String {
        var o = ""
        for u in s.unicodeScalars {
            switch u {
            case "\"": o += "\\\""
            case "\\": o += "\\\\"
            default:
                if u.value < 0x20 || u.value == 0x7f { o += String(format: "\\u%04x", u.value) } else { o.unicodeScalars.append(u) }
            }
        }
        return o
    }

    static func emit(_ check: String, _ kind: String, _ value: String) {
        print("{\"id\":\"\(prog).\(group).\(check)\",\"k\":\"\(kind)\",\"v\":\"\(escape(value))\"}")
        fflush(stdout)
    }

    static func check(_ name: String, _ kind: String = "exact", _ body: () -> String) {
        FileHandle.standardError.write("begin \(prog).\(group).\(name)\n".data(using: .utf8)!)
        emit(name, kind, body())
    }

    static func f(_ d: Double) -> String { String(format: "%.1f", d) }
    static func size(_ s: CGSize) -> String { String(format: "%.1f,%.1f", s.width, s.height) }
    static func rect(_ r: CGRect) -> String {
        String(format: "%.1f,%.1f,%.1f,%.1f", r.origin.x, r.origin.y, r.size.width, r.size.height)
    }
    static func b(_ v: Bool) -> String { v ? "YES" : "NO" }

    static func spin(_ seconds: Double) {
        let until = Date(timeIntervalSinceNow: seconds)
        while until.timeIntervalSinceNow > 0 {
            autoreleasepool {
                _ = RunLoop.current.run(mode: .default, before: Date(timeIntervalSinceNow: 0.02))
            }
        }
    }

    static func main(_ prog: String, _ groups: [(String, @MainActor () -> Void)]) -> Int32 {
        MF.prog = prog
        let args = CommandLine.arguments
        if args.count == 2 && args[1] == "--list" {
            for g in groups { print(g.0) }
            return 0
        }
        if args.count == 3 && args[1] == "--run", let g = groups.first(where: { $0.0 == args[2] }) {
            MF.group = g.0
            g.1()
            print("{\"done\":\"\(prog).\(g.0)\"}")
            fflush(stdout)
            return 0
        }
        FileHandle.standardError.write("usage: \(prog) --list | --run GROUP\n".data(using: .utf8)!)
        return 2
    }
}
