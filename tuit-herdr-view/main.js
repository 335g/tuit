// tuit's commit graph, in a Herdr pane: every branch's history of the folder
// you are in, as colored lanes with branch labels.
// The rows come from the tuit binary itself (`tuit --json`), so the pane shows
// exactly what the TUI shows: same gix walk, same lanes, same relative dates.
import { View, div } from "gpui-kit";
import { v_flex, h_flex } from "gpui-base";
import { exec, focused, panes, spaces, home, theme, settings } from "herdr";

const LIMIT = 200;
const POLL_MS = 2000; // tuit's own poll interval

// A mod pane has no folder of its own: follow the pane beside it.
function folder() {
  const f = focused();
  const pane = f?.pane ? panes().find((p) => p.pane === f.pane) : null;
  return pane?.cwd || f?.cwd || spaces().find((s) => s.active)?.folder || home();
}

// Conventional-commit split, as tuit's ui.rs parses it: type(scope)!: description
function conventional(message) {
  const colon = message.indexOf(":");
  if (colon < 0) return null;
  const prefix = message.slice(0, colon).trimEnd();
  const desc = message.slice(colon + 1).trimStart();
  const breaking = prefix.endsWith("!");
  const clean = breaking ? prefix.slice(0, -1) : prefix;
  const open = clean.indexOf("(");
  const close = clean.lastIndexOf(")");
  const scoped = open >= 0 && close > open;
  const type = (scoped ? clean.slice(0, open) : clean).trim();
  if (!type) return null;
  return { type, scope: scoped ? clean.slice(open + 1, close).trim() : null, breaking, desc };
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
    const rows = this.data?.history ?? [];
    // One character per lane: the graph only lines up in a monospaced font.
    const width = rows.reduce((n, r) => Math.max(n, r.graph.length), 0);
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
          .child(div().text_color(t.dim).child(this.data ? String(rows.length) : "")),
      )
      .child(
        this.error
          ? v_flex()
              .child(note(this.error, t.muted))
              .child(div().px(10).text_color(t.dim).child("tuit --json が要ります: cargo install --path ."))
          : !this.data
            ? note("loading…", t.dim)
            : !rows.length
              ? note("no commits", t.muted)
              : v_flex()
                  .size_full()
                  .overflow_y_scrollbar()
                  .children(rows.map((r) => this.row(r, t, width))),
      );
  }

  // tuit's row colours, from `tuit --json` (its config.toml + theme), so the
  // pane and the TUI look the same. Only the pane's background stays herdr's.
  row(r, t, width) {
    const colors = this.data.colors, cc = this.data.cc;
    const parsed = conventional(r.message);
    const text = (color) => div().text_color(color).whitespace_nowrap();
    const parts = parsed
      ? [
          text(cc[parsed.type] ?? cc.chore).font_weight(700).child(parsed.type),
          parsed.scope ? text(colors.date).child(`(${parsed.scope})`) : null,
          parsed.breaking ? text(colors.diff_del).child("!") : null,
          text(colors.message).child(parsed.desc ? `: ${parsed.desc}` : ""),
        ]
      : [text(colors.message).child(r.message)];
    return h_flex()
      .px(10)
      .py(2)
      .gap(8)
      .items_center()
      .flex_shrink_0()
      .child(
        div()
          .font_family(settings().font_family ?? "JetBrains Mono")
          .text_color(t.dim)
          .whitespace_nowrap()
          .child(r.graph.padEnd(width, " ")),
      )
      .child(
        h_flex()
          .flex_1()
          .overflow_hidden()
          .whitespace_nowrap()
          .text_ellipsis()
          .children(parts.filter(Boolean)),
      )
      .when(r.refs.length > 0, (el) =>
        el.child(div().text_color(t.accent).whitespace_nowrap().child(r.refs.join(" "))),
      )
      .child(text(colors.date).child(r.date));
  }
}
