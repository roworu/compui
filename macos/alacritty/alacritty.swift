import Cocoa

/// alacritty's window.padding x / y (points), dynamic_padding and decorations from its toml; default 0,
/// false, full. shortcut: a line-based read that ignores `import`s
let alacrittyConfig: (x: CGFloat, y: CGFloat, dynamic: Bool, decorations: String) = {
    var out: (x: CGFloat, y: CGFloat, dynamic: Bool, decorations: String) = (0, 0, false, "full")
    let env = ProcessInfo.processInfo.environment
    let xdg = env["XDG_CONFIG_HOME"] ?? NSString(string: "~/.config").expandingTildeInPath
    let files = ["\(xdg)/alacritty/alacritty.toml", "\(xdg)/alacritty.toml", "~/.config/alacritty/alacritty.toml", "~/.alacritty.toml"]
    guard let text = files.lazy.compactMap({ try? String(contentsOfFile: NSString(string: $0).expandingTildeInPath, encoding: .utf8) }).first else { return out }
    var section = ""
    for raw in text.split(separator: "\n") {
        let line = String(raw.split(separator: "#", omittingEmptySubsequences: false)[0]).trimmingCharacters(in: .whitespaces)
        if line.hasPrefix("[") { section = line.trimmingCharacters(in: CharacterSet(charactersIn: "[] ")); continue }
        let kv = line.split(separator: "=", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
        guard kv.count == 2 else { continue }
        let key = section.isEmpty ? kv[0] : "\(section).\(kv[0])"
        switch key {
        case "window.padding":
            for p in kv[1].trimmingCharacters(in: CharacterSet(charactersIn: "{}")).split(separator: ",") {
                let a = p.split(separator: "=").map { $0.trimmingCharacters(in: .whitespaces) }
                guard a.count == 2, let n = Double(a[1]) else { continue }
                if a[0] == "x" { out.x = CGFloat(n) } else if a[0] == "y" { out.y = CGFloat(n) }
            }
        case "window.padding.x": out.x = Double(kv[1]).map { CGFloat($0) } ?? out.x
        case "window.padding.y": out.y = Double(kv[1]).map { CGFloat($0) } ?? out.y
        case "window.dynamic_padding": out.dynamic = kv[1] == "true"
        case "window.decorations": out.decorations = kv[1].trimmingCharacters(in: CharacterSet(charactersIn: "\"'")).lowercased()
        default: break
        }
    }
    return out
}()

/// alacritty: no ax text area, so the window frame below the title bar plus the padding, which
/// alacritty scales and floors to device px; dynamic padding adds half the leftover that is not a whole cell
struct Alacritty: Handler {
    func matches(_ bundleId: String) -> Bool { bundleId == "org.alacritty" }
    func locate(_ win: AXUIElement, _ g: GridPos) -> CGRect? {
        guard g.xpx > 0 && g.ypx > 0, let pos = attr(win, kAXPositionAttribute), let size = attr(win, kAXSizeAttribute) else { return nil }
        var p = CGPoint.zero, s = CGSize.zero
        AXValueGetValue(pos as! AXValue, .cgPoint, &p)
        AXValueGetValue(size as! AXValue, .cgSize, &s)
        let c = alacrittyConfig
        // only "full" draws a title bar above the content; transparent and buttonless draw under it
        let title = c.decorations == "full" ? NSWindow.frameRect(forContentRect: .zero, styleMask: [.titled]).height : 0
        let scale = NSScreen.main?.backingScaleFactor ?? 2
        // alacritty's pixel size is the grid alone, so these are whole device px
        let cw = CGFloat(g.xpx / g.cols), ch = CGFloat(g.ypx / g.rows)
        func inset(_ pad: CGFloat, _ dim: CGFloat, _ cell: CGFloat) -> CGFloat {
            let px = (pad * scale).rounded(.down)
            return (c.dynamic ? px + (dim * scale - 2 * px).truncatingRemainder(dividingBy: cell) / 2 : px) / scale
        }
        let origin = CGPoint(x: p.x + inset(c.x, s.width, cw), y: p.y + title + inset(c.y, s.height - title, ch))
        return cellRect(g, origin, CGSize(width: cw / scale, height: ch / scale))
    }
}
