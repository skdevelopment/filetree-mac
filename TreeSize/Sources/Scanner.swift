import Foundation

/// Pure data model for the file tree. No UI, no side effects beyond filesystem read.
public struct TreeNode: Equatable, Hashable, Sendable {
    public let name: String
    public let size: Int64
    public let isDirectory: Bool
    public let children: [TreeNode]

    public init(name: String, size: Int64, isDirectory: Bool, children: [TreeNode]) {
        self.name = name
        self.size = size
        self.isDirectory = isDirectory
        self.children = children
    }

    public func hash(into hasher: inout Hasher) {
        hasher.combine(name)
        hasher.combine(size)
        hasher.combine(isDirectory)
        // children not included for hash stability in UI selection
    }
}

/// Recursively scans the directory at the given URL.
/// Returns a TreeNode with size = sum of all contained file sizes (logical size).
/// Children (files + dirs) are sorted descending by size.
/// Errors (permission etc) result in skipped entries (size contribution 0 for them).
public func scanDirectory(at url: URL) -> TreeNode {
    let fm = FileManager.default
    var totalSize: Int64 = 0
    var childNodes: [TreeNode] = []

    do {
        let keys: [URLResourceKey] = [.isDirectoryKey, .fileSizeKey]
        let items = try fm.contentsOfDirectory(
            at: url,
            includingPropertiesForKeys: keys,
            options: [.skipsHiddenFiles, .skipsPackageDescendants]
        )

        for item in items {
            do {
                let values = try item.resourceValues(forKeys: Set(keys))
                let isDir = values.isDirectory ?? false

                if isDir {
                    let sub = scanDirectory(at: item)
                    totalSize += sub.size
                    childNodes.append(sub)
                } else {
                    let fsize = Int64(values.fileSize ?? 0)
                    totalSize += fsize
                    childNodes.append(TreeNode(
                        name: item.lastPathComponent,
                        size: fsize,
                        isDirectory: false,
                        children: []
                    ))
                }
            } catch {
                continue
            }
        }
    } catch {
    }

    childNodes.sort { $0.size > $1.size }

    let displayName = url.lastPathComponent.isEmpty ? url.path : url.lastPathComponent
    return TreeNode(
        name: displayName,
        size: totalSize,
        isDirectory: true,
        children: childNodes
    )
}

/// Human readable size using 1024 base
public func humanSize(_ bytes: Int64) -> String {
    if bytes < 1024 { return "\(bytes) B" }
    let units = ["KB", "MB", "GB", "TB", "PB"]
    var v = Double(bytes) / 1024.0
    var idx = 0
    while v >= 1024.0 && idx < units.count - 1 {
        v /= 1024.0
        idx += 1
    }
    return String(format: "%.1f %@", v, units[idx])
}

/// Proportional bar
public func sizeBar(size: Int64, maxSize: Int64, width: Int = 12) -> String {
    guard maxSize > 0 else { return "[" + String(repeating: " ", count: width) + "]" }
    let ratio = Double(size) / Double(maxSize)
    let filled = max(0, min(width, Int((ratio * Double(width)).rounded())))
    let bar = String(repeating: "█", count: filled) + String(repeating: "░", count: width - filled)
    return "[\(bar)]"
}

/// Text tree renderer with bars and %
public func renderTree(_ node: TreeNode, maxSize: Int64? = nil, prefix: String = "", isLast: Bool = true) -> String {
    let total = maxSize ?? node.size
    let bar = sizeBar(size: node.size, maxSize: total, width: 10)
    let pct = total > 0 ? String(format: " %6.1f%%", Double(node.size) / Double(total) * 100) : ""
    let connector = prefix.isEmpty ? "" : (isLast ? "└── " : "├── ")
    let line = "\(prefix)\(connector)\(node.name)\(node.isDirectory ? "/" : "")  \(humanSize(node.size))\(pct)  \(bar)\n"
    var out = line
    let childPrefix = prefix + (isLast ? "    " : "│   ")
    for (i, child) in node.children.enumerated() {
        let last = i == node.children.count - 1
        out += renderTree(child, maxSize: total, prefix: childPrefix, isLast: last)
    }
    return out
}
