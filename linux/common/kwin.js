// loaded into kwin by compui-host: reports the active window's pid, client rect and output on
// activation, move and resize, as one json string to the host's d-bus service
var svc = "io.github.roworu.compui";
var cur = null;

function send() {
    var w = workspace.activeWindow;
    var m = { pid: 0 };
    if (w) {
        var g = w.clientGeometry, o = w.output;
        m = { pid: w.pid, x: g.x, y: g.y, w: g.width, h: g.height, output: o.name, scale: o.devicePixelRatio };
    }
    callDBus(svc, "/", svc, "Window", JSON.stringify(m));
}

function activated(w) {
    if (cur) cur.frameGeometryChanged.disconnect(send);
    cur = w;
    if (cur) cur.frameGeometryChanged.connect(send);
    send();
}

workspace.windowActivated.connect(activated);
activated(workspace.activeWindow);
