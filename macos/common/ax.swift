import Cocoa
import ApplicationServices

func log(_ m: String) { if configDebug || ProcessInfo.processInfo.environment["COMPUI_DEBUG"] != nil { FileHandle.standardError.write(Data((m + "\n").utf8)) } }

/// pid and its parents, to tell whether the frontmost app is the terminal a session runs in
func ancestors(_ pid: Int32) -> Set<Int32> {
    var out = Set<Int32>(), p = pid
    while p > 1 && !out.contains(p) {
        out.insert(p)
        // sysctl, not proc_pidinfo: that is denied for setuid processes like terminal.app's `login`
        var info = kinfo_proc(), size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, p]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { break }
        p = info.kp_eproc.e_ppid
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

/// the cursor cell and the terminal grid, as reported by compui-pty
struct GridPos { let row: Int, col: Int, rows: Int, cols: Int, xpx: Int, ypx: Int }

/// a terminal app the popup can be placed over
protocol Handler {
    /// for `terminal.<name> = off` in the config
    var name: String { get }
    /// true when this handler knows the app
    func matches(_ bundleId: String) -> Bool
    /// cursor cell rect in ax coordinates (points, top-left origin), or nil
    func locate(_ win: AXUIElement, _ g: GridPos) -> CGRect?
}

/// exact: bounds of the selected range, where the terminal exposes it (terminal.app, iterm2)
func exactCursor(_ ta: AXUIElement?) -> CGRect? {
    guard let ta = ta, let sel = attr(ta, kAXSelectedTextRangeAttribute) else { return nil }
    var range = CFRange()
    AXValueGetValue(sel as! AXValue, .cfRange, &range)
    range.length = 1
    let param = AXValueCreate(.cfRange, &range)!
    var out: CFTypeRef?
    guard AXUIElementCopyParameterizedAttributeValue(ta, kAXBoundsForRangeParameterizedAttribute as CFString, param, &out) == .success,
          let r = rect(of: out), r.width > 0 else { return nil }
    return r
}

/// estimate from the window frame: cell size from the pty's pixel size (points = px / scale), grid centred
/// horizontally and anchored to the bottom with the same padding; without pixels, window frame / grid
func windowGrid(_ win: AXUIElement, _ g: GridPos) -> (origin: CGPoint, cell: CGSize)? {
    guard let pos = attr(win, kAXPositionAttribute), let size = attr(win, kAXSizeAttribute) else { return nil }
    var p = CGPoint.zero, s = CGSize.zero
    AXValueGetValue(pos as! AXValue, .cgPoint, &p)
    AXValueGetValue(size as! AXValue, .cgSize, &s)
    var cw = s.width / CGFloat(g.cols), ch = s.height / CGFloat(g.rows)
    var o = p
    if g.xpx > 0 && g.ypx > 0 {
        let scale = NSScreen.main?.backingScaleFactor ?? 2
        cw = CGFloat(g.xpx) / CGFloat(g.cols) / scale
        ch = CGFloat(g.ypx) / CGFloat(g.rows) / scale
        let pad = max((s.width - cw * CGFloat(g.cols)) / 2, 0)
        o = CGPoint(x: p.x + pad, y: p.y + s.height - ch * CGFloat(g.rows) - pad)
    }
    return (o, CGSize(width: cw, height: ch))
}

/// rect of the cursor cell given the grid's top-left and the cell size
func cellRect(_ g: GridPos, _ origin: CGPoint, _ cell: CGSize) -> CGRect {
    log("cell \(cell) origin \(origin)")
    return CGRect(x: origin.x + CGFloat(g.col) * cell.width, y: origin.y + CGFloat(g.row) * cell.height, width: cell.width, height: cell.height)
}

/// exact when the terminal allows, else the window estimate
func genericLocate(_ win: AXUIElement, _ g: GridPos) -> CGRect? {
    if let r = exactCursor(firstTextArea(win)) { return r }
    guard let e = windowGrid(win, g) else { return nil }
    return cellRect(g, e.origin, e.cell)
}
