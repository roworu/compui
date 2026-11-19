import Cocoa

/// terminal.app: exposes `AXBoundsForRange`, so the exact path does the work
struct Terminal: Handler {
    func matches(_ bundleId: String) -> Bool { bundleId == "com.apple.Terminal" }
    func locate(_ win: AXUIElement, _ g: GridPos) -> CGRect? { genericLocate(win, g) }
}
