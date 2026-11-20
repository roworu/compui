import Cocoa

/// iterm2: the focused element is the text area of the focused session, so splits are told apart
struct ITerm2: Handler {
    let name = "iterm2"
    func matches(_ bundleId: String) -> Bool { bundleId == "com.googlecode.iterm2" }
    func locate(_ win: AXUIElement, _ g: GridPos) -> CGRect? {
        var pid: pid_t = 0
        AXUIElementGetPid(win, &pid)
        let focused = attr(AXUIElementCreateApplication(pid), kAXFocusedUIElementAttribute).map { $0 as! AXUIElement }
        let ta = focused.flatMap { attr($0, kAXRoleAttribute) as? String == kAXTextAreaRole ? $0 : nil }
        if let r = exactCursor(ta ?? firstTextArea(win)) { return r }
        // shortcut: no window estimate, it would be wrong in a split and the exact path is untested without iterm2 here
        return nil
    }
}
