# Agent activity window

A Host-only bundle that writes the agent's answer, its reasoning trace, and a
"working" flag to `%USERPROFILE%\.dsh\activity\state.json`, and runs a small
always-on-top window that mirrors that file. The window is a single Rust
executable (`eframe`/`egui`), built without a console and without .NET.

## What the window shows

| Block | Content |
|---|---|
| Header | `DSH · работаю` / `DSH · свободен` and the step timer (`12 с`, then `2:04`) |
| Reasoning | the live `reasoning-delta` tail, italic and muted — what the agent is thinking right now |
| Answer | the visible answer (`text-delta`), markdown flattened, line breaks kept |
| Previous | when a new answer has not produced text yet, the last finished answer stays visible under `Прошлый ответ` |

Lines that offer a choice — bullets (`-`, `•`, `*`), numbered (`1.`, `2)`), or
starting with «вариант» — are drawn in **sky blue** `#56B6F7` and bold, so the
options stand out from the body text.

## Behaviour

- **Instant updates**: the window re-reads the state file every 60 ms, so streamed
  text appears with roughly that delay.
- **Visible only while working**: the plugin sets `active: true` on the start of an
  answer, on every chunk, and on every tool call; it sets `active: false` when the
  stream ends or `turn/end` fires. While `active` is false the window is hidden
  with `ViewportCommand::Visible(false)` — it does not linger as an empty frame.
- **Grows upward**: the window keeps its bottom edge anchored and sizes itself to
  the measured text height (64–620 px), so long answers fit without scrolling.
- **Translucent**: transparent clear colour and a 213/255 background, rounded
  14 px corners, always on top, every click passes through to the window below.

## Controls

- **Ctrl+Alt+X** — quit the window process.
- Closing it does not stop the plugin; a DSH restart brings it back.
- Position is the bottom-right corner of the primary screen.

## Configuration

| Field | Default | Meaning |
|---|---|---|
| `showViewer` | `true` | `false` keeps the state file but never launches the window |

```yaml
- id: dsh-activity
  name: '@local/dsh-activity'
  config:
    showViewer: false
```

## Source and build

`activity-rs/` is the crate (`eframe`, `serde_json`, `windows-sys`). Build with the
GNU toolchain and copy the result over `viewer/ActivityWindow.exe`:

```powershell
cargo build --release --manifest-path activity-rs/Cargo.toml
```

The C# implementation that came before this one is kept as
`viewer/ActivityWindow.cs.txt` for reference only.

## Limitations

- Height is applied one frame after measuring, so a very long answer settles in
  about 60 ms instead of instantly.
- Option detection is a text heuristic: a bullet that is not a choice is still
  highlighted.
- Reasoning text is truncated to the last 220 characters; the answer to 600.
- The window cannot be dragged (click-through is always on); Ctrl+Alt+X closes it.
