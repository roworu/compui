import Cocoa

/// unknown terminals: exact where ax gives it, else the window estimate
struct Generic: Handler {
    func matches(_ bundleId: String) -> Bool { true }
    func locate(_ win: AXUIElement, _ g: GridPos) -> CGRect? { genericLocate(win, g) }
}
