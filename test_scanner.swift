import Foundation

func makeTempDir() -> URL {
    let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("treesize-test-\(UUID().uuidString)")
    try! FileManager.default.createDirectory(at: tmp, withIntermediateDirectories: true)
    return tmp
}

func writeFile(at url: URL, size: Int) {
    let data = Data(count: size)
    try! data.write(to: url)
}

func createFixture() -> (root: URL, expected: [String: Int64]) {
    let root = makeTempDir()
    let bigdir = root.appendingPathComponent("bigdir")
    try! FileManager.default.createDirectory(at: bigdir, withIntermediateDirectories: true)
    writeFile(at: bigdir.appendingPathComponent("f1.dat"), size: 5000)

    let sub = bigdir.appendingPathComponent("sub")
    try! FileManager.default.createDirectory(at: sub, withIntermediateDirectories: true)
    writeFile(at: sub.appendingPathComponent("f2.dat"), size: 3000)

    let smalldir = root.appendingPathComponent("smalldir")
    try! FileManager.default.createDirectory(at: smalldir, withIntermediateDirectories: true)
    writeFile(at: smalldir.appendingPathComponent("f3.dat"), size: 100)

    writeFile(at: root.appendingPathComponent("fileA.bin"), size: 2000)
    writeFile(at: root.appendingPathComponent("fileB.bin"), size: 800)

    let expected: [String: Int64] = [
        "root": 10900,
        "bigdir": 8000,
        "sub": 3000,
        "smalldir": 100,
        "fileA.bin": 2000,
        "fileB.bin": 800,
        "f1.dat": 5000,
        "f2.dat": 3000,
        "f3.dat": 100
    ]
    return (root, expected)
}

func runTests() -> Bool {
    print("=== Scanner Unit Tests (real FS temp trees) ===")
    let (rootURL, expected) = createFixture()
    defer { try? FileManager.default.removeItem(at: rootURL) }

    let rootNode = scanDirectory(at: rootURL)

    var pass = true

    if rootNode.size != expected["root"]! {
        print("FAIL: root size \(rootNode.size) != expected \(expected["root"]!)")
        pass = false
    } else {
        print("PASS: root aggregate size = \(rootNode.size)")
    }

    func find(_ node: TreeNode, name: String) -> TreeNode? {
        if node.name == name { return node }
        for c in node.children { if let m = find(c, name: name) { return m } }
        return nil
    }

    let big = find(rootNode, name: "bigdir")!
    if big.size != expected["bigdir"]! {
        print("FAIL: bigdir size")
        pass = false
    } else { print("PASS: bigdir size = \(big.size)") }

    let rootKids = rootNode.children
    if rootKids.count >= 2 {
        if rootKids[0].size < rootKids[1].size {
            print("FAIL: root children not sorted descending")
            pass = false
        } else {
            print("PASS: root children sorted by size desc: \(rootKids.map { $0.name + ":" + String($0.size) })")
        }
    }

    let f1 = find(rootNode, name: "f1.dat")!
    if f1.size != 5000 || f1.isDirectory {
        print("FAIL: f1.dat leaf size or type")
        pass = false
    } else { print("PASS: f1.dat size correct and is file") }

    let subn = find(rootNode, name: "sub")!
    if subn.size != 3000 {
        print("FAIL: sub size")
        pass = false
    } else { print("PASS: sub aggregate correct") }

    let hs = humanSize(10900)
    print("Human size for 10900B: \(hs)")

    let bar = sizeBar(size: 5000, maxSize: 10900, width: 8)
    print("Bar example: \(bar)")

    if pass {
        print("=== ALL SCANNER TESTS PASSED ===")
    } else {
        print("=== SOME TESTS FAILED ===")
    }
    return pass
}

@main
struct TestRunner {
    static func main() {
        if !runTests() {
            exit(1)
        }
        exit(0)
    }
}
