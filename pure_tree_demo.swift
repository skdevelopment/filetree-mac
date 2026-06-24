import Foundation

// Pure driver for verification step 5 (scan-demo.log).
// Compiles against Scanner.swift only; no AppKit, no GUI, no bypass.

@main
struct PureDemo {
    static func main() {
        let fm = FileManager.default
        let fix = fm.temporaryDirectory.appendingPathComponent("pure-demo-fixture-\(UUID().uuidString)")
        try! fm.createDirectory(at: fix, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: fix) }

        // Controlled small tree
        let a = fix.appendingPathComponent("a")
        try! fm.createDirectory(at: a, withIntermediateDirectories: true)
        try! Data(count: 3000).write(to: a.appendingPathComponent("big.bin"))

        let b = a.appendingPathComponent("b")
        try! fm.createDirectory(at: b, withIntermediateDirectories: true)
        try! Data(count: 1000).write(to: b.appendingPathComponent("med.bin"))

        let c = fix.appendingPathComponent("c")
        try! fm.createDirectory(at: c, withIntermediateDirectories: true)
        try! Data(count: 256).write(to: c.appendingPathComponent("small.bin"))

        let root = scanDirectory(at: fix)
        print("TreeSize pure demo scan: \(fix.lastPathComponent)")
        print(renderTree(root))
        print("Total: \(humanSize(root.size))")
    }
}
