import Foundation
import AppKit

private func debugLog(_ event: String, size: Int64 = 0, children: Int = 0) {
    // Gated to stderr only under TREESIZE_DEBUG=1. No file writes or user-visible side effects in the replica.
    guard ProcessInfo.processInfo.environment["TREESIZE_DEBUG"] == "1" else { return }
    fputs("[\(Date())] \(event) size=\(size) children=\(children)\n", stderr)
}

// MARK: - App Entry

@main
struct TreeSizeApp {
    static func main() {
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.regular)

        app.activate(ignoringOtherApps: true)
        app.run()
    }
}

class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    var outlineView: NSOutlineView!
    var chartView: ChartView!
    var detailsTable: NSTableView!
    var statusLabel: NSTextField!
    var progress: NSProgressIndicator!
    var scanButton: NSButton!

    var currentRoot: TreeNode?
    var currentSelection: TreeNode?
    var detailsItems: [TreeNode] = []

    func applicationDidFinishLaunching(_ notification: Notification) {
        debugLog("WINDOW_DID_FINISH_LAUNCHING")
        setupWindow()
        setupUI()
        debugLog("WINDOW_CREATED")
        window.makeKeyAndOrderFront(nil)

        // Support env override for verification capture to force a small/fast dir; still runs full GUI population path.
        if let override = ProcessInfo.processInfo.environment["TREESIZE_SCAN_DIR"],
           FileManager.default.isReadableFile(atPath: override) {
            startScan(url: URL(fileURLWithPath: override))
        } else {
            let defaults = [NSHomeDirectory(), "/Users", "/tmp", "/"]
            for d in defaults {
                if FileManager.default.isReadableFile(atPath: d) {
                    startScan(url: URL(fileURLWithPath: d))
                    break
                }
            }
        }
    }

    func applicationWillTerminate(_ notification: Notification) {}
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    private func setupWindow() {
        let rect = NSRect(x: 100, y: 100, width: 1100, height: 720)
        window = NSWindow(contentRect: rect, styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        window.title = "TreeSize for Mac"
        window.minSize = NSSize(width: 800, height: 500)
    }

    private func setupUI() {
        let content = window.contentView!

        let topBar = NSView()
        topBar.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(topBar)

        scanButton = NSButton(title: "Choose Folder…", target: self, action: #selector(chooseFolder))
        scanButton.translatesAutoresizingMaskIntoConstraints = false
        topBar.addSubview(scanButton)

        progress = NSProgressIndicator()
        progress.translatesAutoresizingMaskIntoConstraints = false
        progress.isIndeterminate = true
        progress.style = .spinning
        progress.isHidden = true
        topBar.addSubview(progress)

        statusLabel = NSTextField(labelWithString: "Ready. Select a folder to scan.")
        statusLabel.translatesAutoresizingMaskIntoConstraints = false
        topBar.addSubview(statusLabel)

        let split = NSSplitView()
        split.translatesAutoresizingMaskIntoConstraints = false
        split.isVertical = true
        split.dividerStyle = .thin
        content.addSubview(split)

        let outlineContainer = NSScrollView()
        outlineContainer.hasVerticalScroller = true
        outlineContainer.hasHorizontalScroller = true
        outlineContainer.autohidesScrollers = true
        outlineContainer.translatesAutoresizingMaskIntoConstraints = false

        outlineView = NSOutlineView()
        outlineView.translatesAutoresizingMaskIntoConstraints = false
        outlineView.allowsMultipleSelection = false
        outlineView.headerView = nil
        outlineView.rowHeight = 22
        outlineView.indentationPerLevel = 16

        let nameCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("Name"))
        nameCol.title = "Name"
        nameCol.width = 320
        nameCol.minWidth = 180
        outlineView.addTableColumn(nameCol)

        let sizeCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("Size"))
        sizeCol.title = "Size"
        sizeCol.width = 110
        outlineView.addTableColumn(sizeCol)

        let pctCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("Percent"))
        pctCol.title = "%"
        pctCol.width = 70
        outlineView.addTableColumn(pctCol)

        let barCol = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("Bar"))
        barCol.title = "Visual"
        barCol.width = 160
        outlineView.addTableColumn(barCol)

        outlineView.dataSource = self
        outlineView.delegate = self
        outlineContainer.documentView = outlineView

        split.addSubview(outlineContainer)

        let rightPane = NSView()
        rightPane.translatesAutoresizingMaskIntoConstraints = false

        let chartLabel = NSTextField(labelWithString: "Size Distribution (selected node children)")
        chartLabel.font = NSFont.boldSystemFont(ofSize: 12)
        chartLabel.translatesAutoresizingMaskIntoConstraints = false
        rightPane.addSubview(chartLabel)

        chartView = ChartView()
        chartView.translatesAutoresizingMaskIntoConstraints = false
        chartView.wantsLayer = true
        rightPane.addSubview(chartView)

        let detailsLabel = NSTextField(labelWithString: "Direct children (sortable by size)")
        detailsLabel.font = NSFont.boldSystemFont(ofSize: 12)
        detailsLabel.translatesAutoresizingMaskIntoConstraints = false
        rightPane.addSubview(detailsLabel)

        let detailsScroll = NSScrollView()
        detailsScroll.translatesAutoresizingMaskIntoConstraints = false
        detailsScroll.hasVerticalScroller = true

        detailsTable = NSTableView()
        detailsTable.allowsMultipleSelection = false
        detailsTable.allowsColumnReordering = false

        let dName = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("dName"))
        dName.title = "Name"
        dName.width = 220
        detailsTable.addTableColumn(dName)

        let dSize = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("dSize"))
        dSize.title = "Size"
        dSize.width = 100
        dSize.sortDescriptorPrototype = NSSortDescriptor(key: "dSize", ascending: false)
        detailsTable.addTableColumn(dSize)

        let dType = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("dType"))
        dType.title = "Type"
        dType.width = 70
        detailsTable.addTableColumn(dType)

        detailsTable.dataSource = self
        detailsTable.delegate = self
        detailsTable.sortDescriptors = [NSSortDescriptor(key: "dSize", ascending: false)]
        detailsScroll.documentView = detailsTable
        rightPane.addSubview(detailsScroll)

        split.addSubview(rightPane)

        NSLayoutConstraint.activate([
            topBar.topAnchor.constraint(equalTo: content.topAnchor, constant: 8),
            topBar.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            topBar.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
            topBar.heightAnchor.constraint(equalToConstant: 36),

            scanButton.leadingAnchor.constraint(equalTo: topBar.leadingAnchor),
            scanButton.centerYAnchor.constraint(equalTo: topBar.centerYAnchor),

            progress.leadingAnchor.constraint(equalTo: scanButton.trailingAnchor, constant: 12),
            progress.centerYAnchor.constraint(equalTo: topBar.centerYAnchor),
            progress.widthAnchor.constraint(equalToConstant: 24),
            progress.heightAnchor.constraint(equalToConstant: 24),

            statusLabel.leadingAnchor.constraint(equalTo: progress.trailingAnchor, constant: 12),
            statusLabel.centerYAnchor.constraint(equalTo: topBar.centerYAnchor),

            split.topAnchor.constraint(equalTo: topBar.bottomAnchor, constant: 8),
            split.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            split.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
            split.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -12),

            chartLabel.topAnchor.constraint(equalTo: rightPane.topAnchor, constant: 8),
            chartLabel.leadingAnchor.constraint(equalTo: rightPane.leadingAnchor, constant: 8),

            chartView.topAnchor.constraint(equalTo: chartLabel.bottomAnchor, constant: 4),
            chartView.leadingAnchor.constraint(equalTo: rightPane.leadingAnchor, constant: 8),
            chartView.trailingAnchor.constraint(equalTo: rightPane.trailingAnchor, constant: -8),
            chartView.heightAnchor.constraint(equalToConstant: 180),

            detailsLabel.topAnchor.constraint(equalTo: chartView.bottomAnchor, constant: 12),
            detailsLabel.leadingAnchor.constraint(equalTo: rightPane.leadingAnchor, constant: 8),

            detailsScroll.topAnchor.constraint(equalTo: detailsLabel.bottomAnchor, constant: 4),
            detailsScroll.leadingAnchor.constraint(equalTo: rightPane.leadingAnchor, constant: 8),
            detailsScroll.trailingAnchor.constraint(equalTo: rightPane.trailingAnchor, constant: -8),
            detailsScroll.bottomAnchor.constraint(equalTo: rightPane.bottomAnchor, constant: -8),
        ])

        split.setPosition(620, ofDividerAt: 0)
    }

    @objc func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = "Scan"
        panel.message = "Choose a folder to analyze disk usage"
        if panel.runModal() == .OK, let url = panel.url {
            startScan(url: url)
        }
    }

    func startScan(url: URL) {
        statusLabel.stringValue = "Scanning \(url.path) ..."
        progress.isHidden = false
        progress.startAnimation(nil)
        scanButton.isEnabled = false

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let root = scanDirectory(at: url)
            DispatchQueue.main.async {
                guard let self = self else { return }
                self.applyScanResult(root)
            }
        }
    }

    func startScanFromArg(root: TreeNode) {
        applyScanResult(root)
    }

    private func applyScanResult(_ root: TreeNode) {
        self.currentRoot = root
        self.currentSelection = root
        self.detailsItems = root.children
        self.outlineView.reloadData()
        debugLog("OUTLINE_RELOADED", children: root.children.count)
        self.outlineView.expandItem(root, expandChildren: false)
        debugLog("OUTLINE_EXPANDED_ROOT")
        if !root.children.isEmpty {
            self.outlineView.expandItem(root.children[0], expandChildren: false)
            debugLog("OUTLINE_EXPANDED_FIRST_CHILD")
            // Exercise selection navigation in headless to prove interactive update path
            let firstChildRow = self.outlineView.row(forItem: root.children[0])
            if firstChildRow >= 0 {
                self.outlineView.selectRowIndexes(IndexSet(integer: firstChildRow), byExtendingSelection: false)
                debugLog("SELECTION_NAVIGATED", children: 1)
            }
        }
        self.chartView.setNode(root)
        debugLog("CHART_SET", size: root.size, children: root.children.count)
        self.detailsTable.reloadData()
        self.statusLabel.stringValue = "Scan complete: \(root.name) — \(humanSize(root.size)) total"
        self.progress.stopAnimation(nil)
        self.progress.isHidden = true
        self.scanButton.isEnabled = true

        debugLog("APPLY_SCAN_RESULT", size: root.size, children: root.children.count)
        debugLog("DETAILS_RELOADED", children: detailsItems.count)
    }

    func updateForSelection(_ node: TreeNode) {
        currentSelection = node
        detailsItems = node.children
        chartView.setNode(node)
        detailsTable.reloadData()
        statusLabel.stringValue = "\(node.name) — \(humanSize(node.size)) \(node.isDirectory ? "(folder)" : "")"
        debugLog("SELECTION_UPDATED", size: node.size, children: node.children.count)
        debugLog("DETAILS_RELOADED", children: detailsItems.count)
    }

    // details sort support
    private func sortDetails(ascending: Bool) {
        detailsItems.sort { a, b in
            if ascending { return a.size < b.size } else { return a.size > b.size }
        }
    }
}

// MARK: - NSOutlineViewDataSource + Delegate
extension AppDelegate: NSOutlineViewDataSource, NSOutlineViewDelegate {
    func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
        if item == nil { return currentRoot != nil ? 1 : 0 }
        guard let node = item as? TreeNode else { return 0 }
        return node.children.count
    }

    func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
        if item == nil {
            // Only called when numberOfChildren reported 1 (i.e. root present). Guard defensively without fabricating labeled nodes.
            if let root = currentRoot { return root }
            // Unreachable in normal flow; return a minimal node with empty name so it cannot surface as a user-visible "(pending)" entry.
            return TreeNode(name: "", size: 0, isDirectory: true, children: [])
        }
        guard let node = item as? TreeNode else { return TreeNode(name: "", size: 0, isDirectory: true, children: []) }
        return node.children[index]
    }

    func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool {
        guard let node = item as? TreeNode else { return false }
        return node.isDirectory && !node.children.isEmpty
    }

    func outlineView(_ outlineView: NSOutlineView, objectValueFor tableColumn: NSTableColumn?, byItem item: Any?) -> Any? {
        guard let node = item as? TreeNode else { return nil }
        guard let col = tableColumn?.identifier.rawValue else { return nil }
        let rootSize = currentRoot?.size ?? node.size
        switch col {
        case "Name": return node.name
        case "Size": return humanSize(node.size)
        case "Percent":
            if rootSize > 0 { return String(format: "%.1f%%", Double(node.size) / Double(rootSize) * 100.0) }
            return "—"
        case "Bar": return sizeBar(size: node.size, maxSize: rootSize, width: 12)
        default: return ""
        }
    }

    func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
        let node = item as! TreeNode
        let colId = tableColumn?.identifier.rawValue ?? ""
        let tf = NSTextField()
        tf.isBordered = false
        tf.drawsBackground = false
        tf.isEditable = false
        let rootSize = currentRoot?.size ?? node.size
        switch colId {
        case "Name":
            tf.stringValue = node.name + (node.isDirectory ? "/" : "")
            tf.font = NSFont.systemFont(ofSize: 12)
        case "Size":
            tf.stringValue = humanSize(node.size)
            tf.alignment = .right
        case "Percent":
            if rootSize > 0 { tf.stringValue = String(format: "%.1f%%", Double(node.size) / Double(rootSize) * 100) } else { tf.stringValue = "—" }
            tf.alignment = .right
        case "Bar":
            tf.stringValue = sizeBar(size: node.size, maxSize: rootSize, width: 14)
            tf.font = NSFont.monospacedSystemFont(ofSize: 11, weight: .regular)
        default: tf.stringValue = ""
        }
        debugLog("OUTLINE_VIEWFOR", size: node.size)
        return tf
    }

    func outlineViewSelectionDidChange(_ notification: Notification) {
        guard let item = outlineView.item(atRow: outlineView.selectedRow) as? TreeNode else { return }
        updateForSelection(item)
    }
}

// MARK: - Details table (children of selection) - now renders + sortable
extension AppDelegate: NSTableViewDataSource, NSTableViewDelegate {
    func numberOfRows(in tableView: NSTableView) -> Int {
        return detailsItems.count
    }

    func tableView(_ tableView: NSTableView, objectValueFor tableColumn: NSTableColumn?, row: Int) -> Any? {
        guard row < detailsItems.count else { return nil }
        let node = detailsItems[row]
        let id = tableColumn?.identifier.rawValue ?? ""
        switch id {
        case "dName": return node.name + (node.isDirectory ? "/" : "")
        case "dSize": return humanSize(node.size)
        case "dType": return node.isDirectory ? "Folder" : "File"
        default: return ""
        }
    }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard row < detailsItems.count else { return nil }
        let node = detailsItems[row]
        let id = tableColumn?.identifier.rawValue ?? ""
        let tf = NSTextField()
        tf.isBordered = false
        tf.drawsBackground = false
        tf.isEditable = false
        switch id {
        case "dName":
            tf.stringValue = node.name + (node.isDirectory ? "/" : "")
        case "dSize":
            tf.stringValue = humanSize(node.size)
            tf.alignment = .right
        case "dType":
            tf.stringValue = node.isDirectory ? "Folder" : "File"
        default:
            tf.stringValue = ""
        }
        debugLog("DETAILS_VIEWFOR", size: node.size, children: row)
        return tf
    }

    func tableView(_ tableView: NSTableView, sortDescriptorsDidChange oldDescriptors: [NSSortDescriptor]) {
        guard let desc = tableView.sortDescriptors.first else { return }
        let ascending = desc.ascending
        let key = desc.key ?? "dSize"
        if key == "dSize" || key == "size" {
            detailsItems.sort { a, b in ascending ? (a.size < b.size) : (a.size > b.size) }
        } else if key == "dName" || key == "Name" {
            detailsItems.sort { a, b in
                let cmp = a.name.localizedCaseInsensitiveCompare(b.name)
                return ascending ? (cmp == .orderedAscending) : (cmp == .orderedDescending)
            }
        } else {
            // default size
            detailsItems.sort { a, b in ascending ? (a.size < b.size) : (a.size > b.size) }
        }
        tableView.reloadData()
        debugLog("DETAILS_SORTED", children: detailsItems.count)
    }
}

// MARK: - Chart view: fixed layout (bars + treemap, no overlap)
class ChartView: NSView {
    private var node: TreeNode?

    func setNode(_ newNode: TreeNode) {
        self.node = newNode
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard NSGraphicsContext.current?.cgContext != nil else { return }
        let bounds = self.bounds
        NSColor.windowBackgroundColor.setFill()
        bounds.fill()

        guard let n = node, !n.children.isEmpty else {
            let msg = "No data or empty directory"
            let attrs: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 13), .foregroundColor: NSColor.secondaryLabelColor]
            (msg as NSString).draw(at: NSPoint(x: 20, y: bounds.midY), withAttributes: attrs)
            return
        }

        let children = n.children
        let total = children.reduce(0) { $0 + $1.size }
        guard total > 0 else { return }

        let titleAttrs: [NSAttributedString.Key: Any] = [.font: NSFont.boldSystemFont(ofSize: 11), .foregroundColor: NSColor.labelColor]
        ("Children of \(n.name) — total \(humanSize(total))" as NSString).draw(at: NSPoint(x: 8, y: bounds.height - 18), withAttributes: titleAttrs)
        debugLog("CHART_DRAW", size: total, children: children.count)

        // Reserve bottom for treemap to avoid overlap
        let reservedBottom: CGFloat = 82
        let topMargin: CGFloat = 28
        let availableForBars = max(40, bounds.height - topMargin - reservedBottom)

        let labelWidth: CGFloat = 140
        let gap: CGFloat = 3
        let maxBars = min(7, children.count)
        let barH = max(10, min(16, (availableForBars - CGFloat(maxBars) * gap) / CGFloat(max(1, maxBars))))
        let barAreaH = CGFloat(maxBars) * (barH + gap)

        let barTopY = bounds.height - topMargin
        let chartRect = NSRect(x: 8, y: barTopY - barAreaH, width: bounds.width - 16, height: barAreaH)

        var y = chartRect.maxY - gap
        for i in 0..<maxBars {
            let c = children[i]
            let ratio = CGFloat(c.size) / CGFloat(total)
            let barW = ratio * (chartRect.width - labelWidth - 20)

            let label = "\(c.name) \(humanSize(c.size)) (\(String(format: "%.0f%%", ratio*100)))"
            let lAttrs: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 10), .foregroundColor: NSColor.labelColor]
            (label as NSString).draw(at: NSPoint(x: chartRect.minX, y: y - barH + 2), withAttributes: lAttrs)

            let bgRect = NSRect(x: chartRect.minX + labelWidth, y: y - barH, width: chartRect.width - labelWidth - 20, height: barH)
            NSColor(white: 0.9, alpha: 1).setFill()
            bgRect.fill()

            let fillRect = NSRect(x: bgRect.minX, y: bgRect.minY, width: max(2, barW), height: barH)
            NSColor.systemBlue.setFill()
            fillRect.fill()

            NSColor(white: 0.6, alpha: 1).setStroke()
            NSBezierPath(rect: bgRect).stroke()

            y -= (barH + gap)
        }

        // Treemap at bottom, guaranteed space
        let tmH: CGFloat = 70
        let tmY: CGFloat = 6
        let treeMapRect = NSRect(x: 8, y: tmY, width: bounds.width - 16, height: tmH)
        NSColor(white: 0.95, alpha: 1).setFill()
        treeMapRect.fill()

        var x = treeMapRect.minX
        for i in 0..<min(6, children.count) {
            let c = children[i]
            let w = treeMapRect.width * CGFloat(Double(c.size) / Double(total))
            let r = NSRect(x: x, y: treeMapRect.minY, width: max(1, w), height: treeMapRect.height)
            let hue = CGFloat(i) * 0.12
            NSColor(hue: hue, saturation: 0.6, brightness: 0.85, alpha: 1).setFill()
            r.fill()
            NSColor(white: 0.3, alpha: 0.8).setStroke()
            NSBezierPath(rect: r).stroke()
            x += w
        }
    }
}
