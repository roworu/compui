import Cocoa

/// ghostty's window-padding-x / -y (left / top value) in points; default 2. shortcut: ignores config-file includes
let ghosttyPadding: (x: CGFloat, y: CGFloat) = {
    var out: [String: CGFloat] = ["x": 2, "y": 2]
    for f in ["~/.config/ghostty/config", "~/Library/Application Support/com.mitchellh.ghostty/config"] {
        guard let text = try? String(contentsOfFile: NSString(string: f).expandingTildeInPath, encoding: .utf8) else { continue }
        for line in text.split(separator: "\n") {
            let kv = line.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            guard kv.count == 2, kv[0].hasPrefix("window-padding-"), let a = kv[0].last.map(String.init), out[a] != nil,
                  let v = Double(kv[1].split(separator: ",")[0].trimmingCharacters(in: .whitespaces)) else { continue }
            out[a] = CGFloat(v)
        }
    }
    return (out["x"]!, out["y"]!)
}()

/// ghostty: exact if its surface ever exposes it, else the surface's ax frame plus the padding
struct Ghostty: Handler {
    func matches(_ bundleId: String) -> Bool { bundleId == "com.mitchellh.ghostty" }
    func locate(_ win: AXUIElement, _ g: GridPos) -> CGRect? {
        let ta = firstTextArea(win)
        if let r = exactCursor(ta) { return r }
        guard var e = windowGrid(win, g) else { return nil }
        // the text area is the terminal surface: the grid starts at its top-left plus the padding
        if g.xpx > 0 && g.ypx > 0, let ta = ta, let tp = attr(ta, kAXPositionAttribute) {
            var t = CGPoint.zero
            AXValueGetValue(tp as! AXValue, .cgPoint, &t)
            e.origin = CGPoint(x: t.x + ghosttyPadding.x, y: t.y + ghosttyPadding.y)
        }
        return cellRect(g, e.origin, e.cell)
    }
}
