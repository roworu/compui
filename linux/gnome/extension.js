// compui's gnome shell extension, embedded in `compui` and written and enabled by `compui host`:
// reports the focused window to the host like linux/common/kwin.js, and shows the popup image the
// host renders, since mutter has no layer-shell. wheel scrolls go back to the host
import Clutter from 'gi://Clutter';
import Cogl from 'gi://Cogl';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const HOST = 'io.github.roworu.compui';
const IFACE = `<node><interface name="io.github.roworu.compui.Shell">
  <method name="Show"><arg type="s"/><arg type="i"/><arg type="i"/><arg type="i"/><arg type="d"/><arg type="d"/><arg type="d"/><arg type="d"/></method>
  <method name="Hide"/>
</interface></node>`;

const call = (method, sig, args) =>
    Gio.DBus.session.call(HOST, '/', HOST, method, new GLib.Variant(sig, args), null, Gio.DBusCallFlags.NO_AUTO_START, -1, null, null);

export default class Compui extends Extension {
    enable() {
        this._mutter = new Gio.Settings({schema_id: 'org.gnome.mutter'});
        this._k = 1;
        this._acc = 0;
        this._popup = new Clutter.Actor({reactive: true, visible: false});
        this._popup.connect('scroll-event', (_a, e) => this._scroll(e));
        Main.layoutManager.addTopChrome(this._popup);
        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE, {Show: (...a) => this._show(...a), Hide: () => this._popup.hide()});
        this._dbus.export(Gio.DBus.session, '/io/github/roworu/compui');
        this._focus = global.display.connect('notify::focus-window', () => this._watch());
        // a host started later gets the current window once it owns its name
        this._name = Gio.bus_watch_name(Gio.BusType.SESSION, HOST, 0, () => this._send(), null);
        this._watch();
    }

    disable() {
        Gio.bus_unwatch_name(this._name);
        global.display.disconnect(this._focus);
        this._watch(true);
        this._dbus.unexport();
        this._popup.destroy();
        this._popup = this._dbus = this._mutter = null;
    }

    _watch(stop) {
        this._ids?.forEach(id => this._win.disconnect(id));
        this._ids = null;
        this._win = stop ? null : global.display.focus_window;
        if (!this._win)
            return;
        this._ids = ['position-changed', 'size-changed'].map(s => this._win.connect(s, () => this._send()));
        this._send();
    }

    // the host works in logical px; the stage is in device px unless monitor framebuffers are scaled
    _send() {
        const w = this._win;
        if (!w || w.get_monitor() < 0)
            return call('Window', '(s)', ['{"pid":0}']);
        const scale = global.display.get_monitor_scale(w.get_monitor());
        const k = this._k = this._mutter.get_strv('experimental-features').includes('scale-monitor-framebuffer') ? 1 : scale;
        const r = w.frame_rect_to_client_rect(w.get_frame_rect());
        call('Window', '(s)', [JSON.stringify({pid: w.get_pid(), x: r.x / k, y: r.y / k, w: r.width / k, h: r.height / k, output: '', scale})]);
    }

    // `file` is cairo argb32 (premultiplied bgra) of pw x ph device px, shown at x, y (w x h logical px)
    _show(file, pw, ph, stride, x, y, w, h) {
        const [, data] = GLib.file_get_contents(file);
        const content = St.ImageContent.new_with_preferred_size(pw, ph);
        content.set_bytes(global.stage.context.get_backend().get_cogl_context(), new GLib.Bytes(data), Cogl.PixelFormat.BGRA_8888_PRE, pw, ph, stride);
        const k = this._k;
        this._popup.set_content(content);
        this._popup.set_position(x * k, y * k);
        this._popup.set_size(w * k, h * k);
        this._popup.show();
    }

    // whole rows, down positive; touchpad deltas are summed
    _scroll(e) {
        const d = e.get_scroll_direction();
        this._acc += d === Clutter.ScrollDirection.UP ? -1 : d === Clutter.ScrollDirection.DOWN ? 1
            : d === Clutter.ScrollDirection.SMOOTH ? e.get_scroll_delta()[1] : 0;
        const n = Math.trunc(this._acc);
        this._acc -= n;
        if (n)
            call('Scroll', '(i)', [n]);
        return Clutter.EVENT_STOP;
    }
}
