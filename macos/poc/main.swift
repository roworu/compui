// poc host: listens on /tmp/compui-<uid>.sock for compui-pty messages, asks `compcore serve` for candidates,
// finds the cursor of the frontmost terminal via accessibility and shows a non-activating popup under the
// typed word. lives in the menu bar.
import Cocoa
import ApplicationServices


/// pid and its parents, to tell whether the frontmost app is the terminal a session runs in
func ancestors(_ pid: Int32) -> Set<Int32> {
    var out = Set<Int32>(), p = pid
    while p > 1 && !out.contains(p) {
        out.insert(p)
        var info = proc_bsdinfo()
        guard proc_pidinfo(p, PROC_PIDTBSDINFO, 0, &info, Int32(MemoryLayout<proc_bsdinfo>.size)) > 0 else { break }
        p = Int32(info.pbi_ppid)
    }
    return out
}

func attr(_ el: AXUIElement, _ name: String) -> CFTypeRef? {
    var v: CFTypeRef?
    return AXUIElementCopyAttributeValue(el, name as CFString, &v) == .success ? v : nil
}

func rect(of v: CFTypeRef?) -> CGRect? {
    var r = CGRect.zero
    guard let v = v, AXValueGetValue(v as! AXValue, .cgRect, &r) else { return nil }
    return r
}

func firstTextArea(_ el: AXUIElement, depth: Int = 0) -> AXUIElement? {
    if attr(el, kAXRoleAttribute) as? String == kAXTextAreaRole { return el }
    guard depth < 6, let kids = attr(el, kAXChildrenAttribute) as? [AXUIElement] else { return nil }
    for k in kids { if let t = firstTextArea(k, depth: depth + 1) { return t } }
    return nil
}

/// cursor rect in ax coordinates (points, top-left origin), or nil
func locate(_ win: AXUIElement, _ args: [Int]) -> CGRect? {
    // exact: bounds of the selected range (terminal.app, iterm2)
    let ta = firstTextArea(win)
    if let ta = ta, let sel = attr(ta, kAXSelectedTextRangeAttribute) {
        var range = CFRange()
        AXValueGetValue(sel as! AXValue, .cfRange, &range)
        range.length = 1
        let param = AXValueCreate(.cfRange, &range)!
        var out: CFTypeRef?
        if AXUIElementCopyParameterizedAttributeValue(ta, kAXBoundsForRangeParameterizedAttribute as CFString, param, &out) == .success,
           let r = rect(of: out), r.width > 0 { return r }
    }
    // fallback: cell size from the pty's pixel size (points = px / scale), grid centred horizontally
    // and anchored to the bottom with the same padding; without pixels, window frame / grid
    guard args.count == 6, let pos = attr(win, kAXPositionAttribute), let size = attr(win, kAXSizeAttribute) else { return nil }
    var p = CGPoint.zero, s = CGSize.zero
    AXValueGetValue(pos as! AXValue, .cgPoint, &p)
    AXValueGetValue(size as! AXValue, .cgSize, &s)
    let (row, col, rows, cols, xpx, ypx) = (args[0], args[1], args[2], args[3], args[4], args[5])
    var cw = s.width / CGFloat(cols), ch = s.height / CGFloat(rows)
    var x0 = p.x, y0 = p.y
    if xpx > 0 && ypx > 0 {
        let scale = NSScreen.main?.backingScaleFactor ?? 2
        cw = CGFloat(xpx) / CGFloat(cols) / scale
        ch = CGFloat(ypx) / CGFloat(rows) / scale
        let pad = max((s.width - cw * CGFloat(cols)) / 2, 0)
        x0 = p.x + pad
        y0 = p.y + s.height - ch * CGFloat(rows) - pad
        // the text area is the terminal surface: the grid starts at its top-left plus the padding
        if let ta = ta, let tp = attr(ta, kAXPositionAttribute) {
            var t = CGPoint.zero
            AXValueGetValue(tp as! AXValue, .cgPoint, &t)
            x0 = t.x + 2; y0 = t.y + 2 // shortcut: ghostty's default window-padding of 2, read the config to be exact
        }
    }
    log("window \(p) \(s) cell \(cw)x\(ch) origin \(x0),\(y0)")
    return CGRect(x: x0 + CGFloat(col) * cw, y: y0 + CGFloat(row) * ch, width: cw, height: ch)
}

func log(_ m: String) { if ProcessInfo.processInfo.environment["COMPUI_DEBUG"] != nil { FileHandle.standardError.write(Data((m + "\n").utf8)) } }

guard AXIsProcessTrustedWithOptions([kAXTrustedCheckOptionPrompt.takeUnretainedValue(): true] as CFDictionary) else {
    print("grant accessibility to this binary's terminal, then rerun"); exit(1)
}

let nsApp = NSApplication.shared
nsApp.setActivationPolicy(.accessory)

// tray: stays while the host runs
let tray = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
tray.button?.title = "compUI"
let menu = NSMenu()
var enabled = true
final class Toggle: NSObject {
    @objc func flip(_ item: NSMenuItem) { enabled.toggle(); item.state = enabled ? .on : .off; if !enabled { hide() } }
}
let toggle = Toggle()
let toggleItem = NSMenuItem(title: "Popup enabled", action: #selector(Toggle.flip(_:)), keyEquivalent: "")
toggleItem.target = toggle
toggleItem.state = .on
menu.addItem(toggleItem)
menu.addItem(withTitle: "Quit", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
tray.menu = menu

struct Cand { let label: String, insert: String, desc: String, kind: String }
struct Ctx { let fd: Int32, session: Int, seq: Int, cell: [String: Int], grid: [String: Int], cursor: Int }

let label = NSTextField(labelWithString: "")
let pad: CGFloat = 6
let panel = NSPanel(contentRect: .zero, styleMask: [.nonactivatingPanel, .borderless], backing: .buffered, defer: false)
panel.level = .popUpMenu
panel.hidesOnDeactivate = false
panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
panel.ignoresMouseEvents = true
panel.backgroundColor = NSColor(white: 0.1, alpha: 0.95)
panel.contentView?.addSubview(label)

let kindColor: [String: NSColor] = ["subcommand": .magenta, "suggestion": .cyan, "option": .yellow,
                                    "command": .green, "folder": .blue, "file": .white]
let maxRows = 8

var cands: [Cand] = []
var selected = 0
var ctx: Ctx?
var wordStart = 0, wordEnd = 0
var suppressed: (fd: Int32, start: Int)?  // word the user accepted or dismissed, no popup until the next one
var shownFd: Int32?
var latestId = 0
var requests: [Int: Ctx] = [:]
var compcore: Process!
var toCompcore: FileHandle!

func reply(_ fd: Int32, _ obj: [String: Any]) {
    var o = obj; o["v"] = 1
    if var d = try? JSONSerialization.data(withJSONObject: o) { d.append(10); d.withUnsafeBytes { _ = write(fd, $0.baseAddress, d.count) } }
}

func hide() {
    panel.orderOut(nil)
    if let fd = shownFd, let c = ctx { reply(fd, ["type": "hidden", "session": c.session]) }
    shownFd = nil
}

func render() {
    let top = min(max(selected - maxRows + 1, 0), max(cands.count - maxRows, 0))
    let text = NSMutableAttributedString()
    let font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
    for (i, c) in cands[top..<min(top + maxRows, cands.count)].enumerated() {
        var attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: kindColor[c.kind] ?? .white]
        if top + i == selected { attrs[.backgroundColor] = NSColor(white: 0.3, alpha: 1) }
        text.append(NSAttributedString(string: c.label + (c.desc.isEmpty ? "" : "  " + c.desc) + (i < min(maxRows, cands.count - top) - 1 ? "\n" : ""), attributes: attrs))
    }
    label.attributedStringValue = text
    let size = label.fittingSize
    label.frame = NSRect(x: pad, y: pad, width: size.width, height: size.height)
    panel.setContentSize(NSSize(width: size.width + 2 * pad, height: size.height + 2 * pad))
}

func show(_ cursor: CGRect) {
    render()
    // ax is top-left origin on the primary screen, appkit is bottom-left
    let primaryH = NSScreen.screens[0].frame.height
    var origin = NSPoint(x: cursor.minX - pad, y: primaryH - cursor.maxY - panel.frame.height)
    let visible = (NSScreen.screens.first { $0.frame.contains(NSPoint(x: cursor.midX, y: primaryH - cursor.midY)) } ?? NSScreen.screens[0]).visibleFrame
    if origin.y < visible.minY { origin.y = primaryH - cursor.minY } // flip above
    origin.x = min(max(origin.x, visible.minX), visible.maxX - panel.frame.width)
    panel.setFrameOrigin(origin)
    panel.orderFrontRegardless()
}

/// a reply from compcore: seq, start, end, then candidates
func onCandidates(_ record: String) {
    let head = record.split(separator: "\u{1e}", omittingEmptySubsequences: false)
    let f = head[0].split(separator: "\u{1f}", omittingEmptySubsequences: false)
    guard f.count == 3, let id = Int(f[0]), id == latestId, let c = requests[id], let s = Int(f[1]), let e = Int(f[2]) else { return }
    requests = [:]
    cands = head.dropFirst().map {
        let p = $0.split(separator: "\u{1f}", omittingEmptySubsequences: false).map(String.init)
        return Cand(label: p[0], insert: p[1], desc: p.count > 2 ? p[2] : "", kind: p.count > 3 ? p[3] : "")
    }
    if cands.isEmpty || suppressed.map({ $0.fd == c.fd && $0.start == s }) == true { ctx = c; hide(); return }
    // shortcut: the word's first cell is the cursor cell minus its chars, no wrapping or wide chars
    let col = max((c.cell["col"] ?? 0) - (c.cursor - s), 0)
    guard let app = NSWorkspace.shared.frontmostApplication,
          let win = attr(AXUIElementCreateApplication(app.processIdentifier), kAXFocusedWindowAttribute),
          let rect = locate(win as! AXUIElement, [c.cell["row"] ?? 0, col, c.grid["rows"] ?? 1, c.grid["cols"] ?? 1, c.grid["xpx"] ?? 0, c.grid["ypx"] ?? 0]) else { hide(); return }
    ctx = c; wordStart = s; wordEnd = e; selected = 0
    show(rect)
    if shownFd == nil { shownFd = c.fd; reply(c.fd, ["type": "shown", "session": c.session]) }
}

func startCompcore() {
    let dir = URL(fileURLWithPath: Bundle.main.executablePath ?? CommandLine.arguments[0]).deletingLastPathComponent()
    compcore = Process()
    compcore.executableURL = dir.appendingPathComponent("compcore")
    compcore.arguments = ["serve"]
    let inp = Pipe(), outp = Pipe()
    compcore.standardInput = inp
    compcore.standardOutput = outp
    do { try compcore.run() } catch { print("cannot start compcore: \(error)"); exit(1) }
    toCompcore = inp.fileHandleForWriting
    var pending = Data()
    outp.fileHandleForReading.readabilityHandler = { h in
        pending.append(h.availableData)
        while let z = pending.firstIndex(of: 0) {
            let rec = String(decoding: pending[..<z], as: UTF8.self)
            pending.removeSubrange(...z)
            DispatchQueue.main.async { onCandidates(rec) }
        }
    }
}

func handle(_ line: Data, _ fd: Int32) {
    guard let m = try? JSONSerialization.jsonObject(with: line) as? [String: Any], let type = m["type"] as? String else { return }
    let session = m["session"] as? Int ?? 0
    switch type {
    case "end":
        suppressed = nil; latestId += 1
        if shownFd == fd { hide() }
    case "line":
        guard let buffer = m["buffer"] as? String, let cursor = m["cursor"] as? Int, let cwd = m["cwd"] as? String,
              let path = m["path"] as? String, let cell = m["cell"] as? [String: Int], let grid = m["grid"] as? [String: Int] else { return }
        let active = NSWorkspace.shared.frontmostApplication.map { ancestors(Int32(session)).contains($0.processIdentifier) } ?? false
        if m["history"] as? Bool == true || !active || !enabled || buffer.trimmingCharacters(in: .whitespaces).isEmpty { latestId += 1; if shownFd == fd { hide() }; return }
        latestId += 1
        requests[latestId] = Ctx(fd: fd, session: session, seq: m["seq"] as? Int ?? 0, cell: cell, grid: grid, cursor: cursor)
        toCompcore.write(Data("\(latestId)\u{1f}\(cursor)\u{1f}\(cwd)\u{1f}\(path)\u{1f}\(buffer)\0".utf8))
    case "key":
        guard shownFd == fd, let c = ctx, !cands.isEmpty else { return }
        switch m["key"] as? String {
        case "up": selected = (selected + cands.count - 1) % cands.count; render()
        case "down": selected = (selected + 1) % cands.count; render()
        case "accept":
            reply(fd, ["type": "accept", "session": c.session, "seq": c.seq, "start": wordStart, "end": wordEnd, "text": cands[selected].insert])
            suppressed = (fd, wordStart); shownFd = nil; panel.orderOut(nil)
        case "dismiss": suppressed = (fd, wordStart); hide()
        default: break
        }
    default: break
    }
}

NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main) { _ in
    if let fd = shownFd { _ = fd; hide() }
}

// socket server on a background thread, one thread per client
let path = "/tmp/compui-\(getuid()).sock"
unlink(path)
let srv = socket(AF_UNIX, SOCK_STREAM, 0)
var addr = sockaddr_un()
addr.sun_family = sa_family_t(AF_UNIX)
withUnsafeMutableBytes(of: &addr.sun_path) { dst in path.utf8CString.withUnsafeBytes { src in dst.copyMemory(from: UnsafeRawBufferPointer(rebasing: src.prefix(dst.count - 1))) } }
_ = withUnsafePointer(to: &addr) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(srv, $0, socklen_t(MemoryLayout<sockaddr_un>.size)) } }
listen(srv, 8)
startCompcore()
Thread.detachNewThread {
    while true {
        let c = accept(srv, nil, nil)
        if c < 0 { continue }
        Thread.detachNewThread {
            var pending = Data()
            var buf = [UInt8](repeating: 0, count: 4096)
            while true {
                let n = read(c, &buf, buf.count)
                if n <= 0 { break }
                pending.append(contentsOf: buf[..<n])
                while let nl = pending.firstIndex(of: 10) {
                    let l = pending[..<nl]
                    pending.removeSubrange(...nl)
                    DispatchQueue.main.async { handle(Data(l), c) }
                }
            }
            DispatchQueue.main.async { if shownFd == c { hide() } }
            close(c)
        }
    }
}
print("compui host listening on \(path)")
nsApp.run()
