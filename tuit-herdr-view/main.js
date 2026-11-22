// tuit's commit list, in a Herdr pane: the git log of the folder you are in.
// The list comes from the tuit binary itself (`tuit --json`), so the pane shows
// exactly what the TUI shows: same gix walk, same relative dates.
import { View, div } from "gpui-kit";
import { v_flex, h_flex } from "gpui-base";
import { exec, focused, panes, spaces, home, theme } from "herdr";

const LIMIT = 200;
const POLL_MS = 2000; // tuit's own poll interval

// A mod pane has no folder of its own: follow the pane beside it.
function folder() {
  const f = focused();
  const pane = f?.pane ? panes().find((p) => p.pane === f.pane) : null;
  return pane?.cwd || f?.cwd || spaces().find((s) => s.active)?.folder || home();
}

export default class TuitView extends View {
  init(_props, cx) {
    this.cwd = null;
    this.data = null;
    this.error = null;
    this.loading = false;
    this.refresh(cx);
    cx.timer.every(POLL_MS, (cx) => this.refresh(cx));
  }

  refresh(cx) {
    const cwd = folder();
    if (cwd !== this.cwd) {
      this.cwd = cwd;
      this.data = null;
      this.error = null;
    }
    if (this.loading) return;
    this.loading = true;
    cx.spawn(async (cx) => {
      try {
        const r = await exec("tuit", ["--json", "--limit", String(LIMIT)], { cwd });
        if (cwd !== this.cwd) return;
        if (r.code !== 0) {
          this.data = null;
          this.error = (r.stderr || "tuit failed").trim().split("\n")[0];
        } else {
          this.data = JSON.parse(r.stdout);
          this.error = null;
        }
      } catch (e) {
        if (cwd === this.cwd) {
          this.data = null;
          this.error = String(e);
        }
      } finally {
        if (cwd === this.cwd) this.loading = false;
        cx.notify();
      }
    });
  }

  render() {
    const t = theme();
    const commits = this.data?.commits ?? [];
    const note = (text, color) => div().p(10).text_color(color).child(text);
    return v_flex()
      .size_full()
      .bg(t.background)
      .text_size(11.5)
      .child(
        h_flex()
          .px(10)
          .py(5)
          .gap(8)
          .items_center()
          .border_b(1)
          .border_color(t.border)
          .child(div().text_color(t.accent).child(this.data?.branch ?? ""))
          .child(div().text_color(t.dim).child(this.data ? String(commits.length) : "")),
      )
      .child(
        this.error
          ? v_flex()
              .child(note(this.error, t.muted))
              .child(div().px(10).text_color(t.dim).child("tuit --json が要ります: cargo install --path ."))
          : !this.data
            ? note("loading…", t.dim)
            : !commits.length
              ? note("no commits", t.muted)
              : v_flex()
                  .size_full()
                  .overflow_y_scrollbar()
                  .children(commits.map((c) => this.row(c, t))),
      );
  }

  row(c, t) {
    return h_flex()
      .px(10)
      .py(2)
      .gap(8)
      .items_center()
      .flex_shrink_0()
      .child(div().text_color(t.dim).child(c.hash))
      .child(div().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().text_color(t.foreground).child(c.message))
      .child(div().text_color(t.dim).child(c.date));
  }
}
