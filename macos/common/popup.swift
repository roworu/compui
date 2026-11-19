// the popup's look, as kuterm's: kuterm's one dark / one light colors, rows one terminal line high, label
// column in the kind's color, smaller muted descriptions, the selected one's whole description under a
// divider, a scrollbar thumb when the list is longer
import Cocoa

struct Theme { let bg, border, selected, text, muted, scrollbar: NSColor; let kinds: [String: NSColor] }

func hex(_ v: UInt32) -> NSColor {
    NSColor(srgbRed: CGFloat((v >> 16) & 0xff) / 255, green: CGFloat((v >> 8) & 0xff) / 255, blue: CGFloat(v & 0xff) / 255, alpha: 1)
}

/// kuterm's bundled dark and light themes; kinds are ansi magenta, cyan, yellow, green, blue
let darkTheme = Theme(bg: hex(0x2f343e), border: hex(0x464b57), selected: hex(0x282c33), text: hex(0xdce0e5), muted: hex(0xa9afbc), scrollbar: hex(0x4e5666),
                      kinds: ["subcommand": hex(0xc678dd), "suggestion": hex(0x56b6c2), "option": hex(0xe5c07b), "command": hex(0x98c379), "folder": hex(0x61afef)])
let lightTheme = Theme(bg: hex(0xebebec), border: hex(0xc9c9ca), selected: hex(0xfafafa), text: hex(0x383a42), muted: hex(0x6a6b70), scrollbar: hex(0xc4c4c7),
                       kinds: ["subcommand": hex(0xa626a4), "suggestion": hex(0x0997b3), "option": hex(0xc18401), "command": hex(0x50a14f), "folder": hex(0x0184bc)])

let maxRows = 8
// widest label and description, in cells, longer ones end in an ellipsis
let maxLabelCells = 40, maxDescCells = 32
let descScale: CGFloat = 0.85
let maxDetailLines: CGFloat = 3
let pad: CGFloat = 6, radius: CGFloat = 6

func termFont(_ size: CGFloat) -> NSFont {
    NSFont(name: "JetBrainsMono-Regular", size: size) ?? .monospacedSystemFont(ofSize: size, weight: .regular)
}

func advance(_ f: NSFont) -> CGFloat { ("MMMMMMMMMM" as NSString).size(withAttributes: [.font: f]).width / 10 }

/// one line cut with an ellipsis, centred in `r` vertically
func drawLine(_ s: String, _ f: NSFont, _ c: NSColor, _ r: NSRect) {
    let p = NSMutableParagraphStyle()
    p.lineBreakMode = .byTruncatingTail
    let a = NSAttributedString(string: s, attributes: [.font: f, .foregroundColor: c, .paragraphStyle: p])
    let h = a.size().height
    a.draw(with: NSRect(x: r.minX, y: r.minY + (r.height - h) / 2, width: max(r.width, 1), height: h), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
}

func detailText(_ s: String, _ f: NSFont, _ c: NSColor) -> NSAttributedString {
    let p = NSMutableParagraphStyle()
    p.lineBreakMode = .byWordWrapping
    return NSAttributedString(string: s, attributes: [.font: f, .foregroundColor: c, .paragraphStyle: p])
}

/// height of the wrapped detail, at most maxDetailLines lines
func detailHeight(_ s: String, _ f: NSFont, _ width: CGFloat) -> CGFloat {
    let line = NSLayoutManager().defaultLineHeight(for: f)
    let h = detailText(s, f, .black).boundingRect(with: NSSize(width: width, height: .greatestFiniteMagnitude), options: [.usesLineFragmentOrigin]).height
    return min(ceil(h), line * maxDetailLines)
}

final class PopupView: NSView {
    var theme = darkTheme
    var rows: [(label: String, desc: String, color: NSColor)] = []
    var selected: Int?
    var detail: String?
    var thumb: (offset: CGFloat, length: CGFloat)?
    var font = termFont(12)
    var rowH: CGFloat = 16
    var labelW: CGFloat = 0
    override var isFlipped: Bool { true }
    var scrollRest: CGFloat = 0

    /// wheel or trackpad, in whole rows; trackpad deltas are in points
    override func scrollWheel(with e: NSEvent) {
        scrollRest -= e.hasPreciseScrollingDeltas ? e.scrollingDeltaY / max(rowH, 1) : e.scrollingDeltaY
        let n = Int(scrollRest)
        scrollRest -= CGFloat(n)
        if n != 0 { scrollBy(n) }
    }

    /// lays out `cands[top..<top + maxRows]` for a terminal cell of `cell`; returns the popup's size
    func layout(_ cands: [Cand], selected sel: Int, top: Int, cell: CGSize, maxWidth: CGFloat, dark: Bool) -> NSSize {
        theme = dark ? darkTheme : lightTheme
        // the font is sized so a char is as wide as the terminal's cell
        let base = termFont(max(cell.width / 0.6, 6))
        font = termFont(base.pointSize * cell.width / max(advance(base), 1))
        let adv = advance(font)
        let shown = Array(cands[min(top, cands.count)..<min(top + maxRows, cands.count)])
        let labelCells = max(shown.map { min($0.label.count, maxLabelCells) }.max() ?? 0, 12)
        let descCells = shown.filter { !$0.desc.isEmpty }.map { min($0.desc.count, maxDescCells) }.max().map { $0 + 2 } ?? 0
        let w = ceil(min(adv * CGFloat(labelCells + descCells) + 2 * pad, maxWidth))
        rowH = round(cell.height)
        labelW = adv * CGFloat(labelCells)
        rows = shown.map { ($0.label, $0.desc, $0.kind == "history" ? theme.muted : theme.kinds[$0.kind] ?? theme.text) }
        selected = sel >= top && sel < top + shown.count ? sel - top : nil
        detail = sel < cands.count && !cands[sel].desc.isEmpty ? cands[sel].desc : nil
        let detailH = detail.map { detailHeight($0, descFont, w - 2 * pad) + 7 } ?? 0
        thumb = nil
        if cands.count > maxRows {
            let track = rowH * CGFloat(maxRows)
            let len = max(track * CGFloat(maxRows) / CGFloat(cands.count), 8)
            thumb = ((track - len) * CGFloat(top) / CGFloat(cands.count - maxRows), len)
        }
        needsDisplay = true
        return NSSize(width: w, height: ceil(rowH * CGFloat(shown.count) + 8 + detailH))
    }

    var descFont: NSFont { termFont(font.pointSize * descScale) }

    override func draw(_ dirtyRect: NSRect) {
        let b = bounds
        let shape = NSBezierPath(roundedRect: b, xRadius: radius, yRadius: radius)
        theme.bg.setFill()
        shape.fill()
        NSGraphicsContext.saveGraphicsState()
        shape.addClip()
        let top: CGFloat = 4
        for (i, r) in rows.enumerated() {
            let y = top + CGFloat(i) * rowH
            if selected == i { theme.selected.setFill(); NSBezierPath(rect: NSRect(x: 0, y: y, width: b.width, height: rowH)).fill() }
            drawLine(r.label, font, r.color, NSRect(x: pad, y: y, width: labelW, height: rowH))
            if !r.desc.isEmpty {
                let x = pad + labelW + rowH * 0.5
                drawLine(r.desc, descFont, theme.muted, NSRect(x: x, y: y, width: b.width - x - pad, height: rowH))
            }
        }
        if let d = detail {
            let y = top + CGFloat(rows.count) * rowH + 3
            theme.border.setFill()
            NSBezierPath(rect: NSRect(x: pad, y: y, width: b.width - 2 * pad, height: 1)).fill()
            let h = detailHeight(d, descFont, b.width - 2 * pad)
            detailText(d, descFont, theme.muted).draw(with: NSRect(x: pad, y: y + 3, width: b.width - 2 * pad, height: h), options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
        }
        if let t = thumb {
            theme.scrollbar.setFill()
            NSBezierPath(roundedRect: NSRect(x: b.width - 4, y: top + t.offset, width: 3, height: t.length), xRadius: 1.5, yRadius: 1.5).fill()
        }
        NSGraphicsContext.restoreGraphicsState()
        let edge = NSBezierPath(roundedRect: b.insetBy(dx: 0.5, dy: 0.5), xRadius: radius - 0.5, yRadius: radius - 0.5)
        edge.lineWidth = 1
        theme.border.setStroke()
        edge.stroke()
    }
}
