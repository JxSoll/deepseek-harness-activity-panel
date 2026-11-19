//! Translucent always-on-top answer panel.
//!
//! Reads one JSON state file written by the Host plugin and renders the answer
//! the way the chat does: headings, bullet lists with dots, tables with a
//! header rule, and code blocks on a warm background.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use serde::Deserialize;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_MENU, VK_X};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetSystemMetrics, GetWindowRect, GetWindowTextW, IsWindowVisible, SystemParametersInfoW,
    SM_CXSCREEN, SM_CYSCREEN, SPI_GETWORKAREA,
};

const WIDTH: f32 = 400.0;
const MIN_HEIGHT: f32 = 44.0;
const PAD_X: f32 = 14.0;
const PAD_Y: f32 = 10.0;
const GAP: f32 = 6.0;
const POLL: Duration = Duration::from_millis(16);
const READ_GRACE: Duration = Duration::from_secs(60);
const STALE: Duration = Duration::from_secs(3);
const CORNER: u8 = 14;
const BOTTOM_GAP: f32 = 8.0;
const TOP_GAP: f32 = 16.0;
const FADE_STEP: f32 = 0.06;
const SIZE_STEP: f32 = 0.045;

const FONT_SIZE: f32 = 13.0;
const SMALL: f32 = 11.0;
const CODE_SIZE: f32 = 11.5;
const BASE_ALPHA: u8 = 150;

const TEXT: egui::Color32 = egui::Color32::from_rgb(214, 222, 232);
const DIM: egui::Color32 = egui::Color32::from_rgb(140, 152, 166);
const OK: egui::Color32 = egui::Color32::from_rgb(140, 226, 196);
const TABLE_LINE: egui::Color32 = egui::Color32::from_rgb(58, 66, 76);
const CODE_BG: egui::Color32 = egui::Color32::from_rgb(42, 36, 22);
const CODE_TEXT: egui::Color32 = egui::Color32::from_rgb(232, 201, 138);
const CODE_BORDER: egui::Color32 = egui::Color32::from_rgb(122, 90, 34);
const CODE_CHIP: egui::Color32 = egui::Color32::from_rgb(58, 62, 68);

#[derive(Deserialize, Default, Clone)]
struct State {
    #[serde(default)]
    ts: i64,
    #[serde(default)]
    since: i64,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    title: String,
    #[serde(default)]
    answer: String,
    #[serde(default)]
    previous: String,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn default_state_path() -> PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_owned());
    PathBuf::from(home).join(".dsh").join("activity").join("state.json")
}

fn quit_hotkey() -> bool {
    unsafe {
        let ctrl = GetAsyncKeyState(VK_CONTROL as i32) as u16 & 0x8000 != 0;
        let alt = GetAsyncKeyState(VK_MENU as i32) as u16 & 0x8000 != 0;
        let x = GetAsyncKeyState(VK_X as i32) as u16 & 0x8000 != 0;
        ctrl && alt && x
    }
}

static DSH_TOP: AtomicI32 = AtomicI32::new(-1);

unsafe extern "system" fn find_dsh(hwnd: *mut core::ffi::c_void, _param: isize) -> i32 {
    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    let mut buf = [0u16; 256];
    let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if len <= 0 {
        return 1;
    }
    let title = String::from_utf16_lossy(&buf[..len as usize]);
    if title.contains("DeepSeek Harness") {
        let mut rect = windows_sys::Win32::Foundation::RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(hwnd, &mut rect) != 0 {
            let top = rect.top.max(0);
            let current = DSH_TOP.load(Ordering::Relaxed);
            if current < 0 || top < current {
                DSH_TOP.store(top, Ordering::Relaxed);
            }
        }
        return 0;
    }
    1
}

fn dsh_top() -> Option<f32> {
    DSH_TOP.store(-1, Ordering::Relaxed);
    unsafe {
        EnumWindows(Some(find_dsh), 0);
    }
    let top = DSH_TOP.load(Ordering::Relaxed);
    if top > 0 { Some(top as f32) } else { None }
}

fn work_bottom() -> f32 {
    let mut rect = windows_sys::Win32::Foundation::RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut rect as *mut _ as *mut core::ffi::c_void, 0);
    }
    if rect.bottom > 0 {
        rect.bottom as f32
    } else {
        unsafe { GetSystemMetrics(SM_CYSCREEN) as f32 }
    }
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for path in ["C:\\Windows\\Fonts\\segoeui.ttf", "C:\\Windows\\Fonts\\SegoeUI.ttf"] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("segoe".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.insert(0, "segoe".to_owned());
            }
            break;
        }
    }
    if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\consola.ttf") {
        fonts
            .font_data
            .insert("consola".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            family.insert(0, "consola".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}


/// A piece of one paragraph: plain text, bold text or inline code.
enum Seg {
    Plain(Arc<egui::Galley>),
    Bold(Arc<egui::Galley>),
    Code(Arc<egui::Galley>),
}

fn parse_inline(painter: &egui::Painter, text: &str, size: f32, color: egui::Color32, max_w: f32) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut chars = text.chars().peekable();
    let flush = |buf: &mut String, out: &mut Vec<Seg>| {
        if buf.is_empty() {
            return;
        }
        let text = std::mem::take(buf);
        // Word by word, so the greedy wrap can break long sentences.
        for word in text.split(' ') {
            if word.is_empty() {
                continue;
            }
            let mut piece = word.to_owned();
            loop {
                let g = painter.layout(piece.clone(), egui::FontId::proportional(size), color, f32::INFINITY);
                if g.size().x <= max_w || piece.chars().count() <= 3 {
                    out.push(Seg::Plain(g));
                    break;
                }
                // Long unbroken token: cut it in half and keep going.
                let mid = piece.chars().count() / 2;
                let cut: String = piece.chars().take(mid).collect();
                let rest: String = piece.chars().skip(mid).collect();
                let g_cut = painter.layout(cut, egui::FontId::proportional(size), color, f32::INFINITY);
                out.push(Seg::Plain(g_cut));
                piece = rest;
            }
        }
    };
    while let Some(c) = chars.next() {
        if c == '`' {
            flush(&mut buf, &mut out);
            let mut code = String::new();
            while let Some(&n) = chars.peek() {
                chars.next();
                if n == '`' {
                    break;
                }
                code.push(n);
            }
            if !code.is_empty() {
                let g = painter.layout(
                    code,
                    egui::FontId::monospace(size - 1.5),
                    CODE_TEXT,
                    f32::INFINITY,
                );
                out.push(Seg::Code(g));
            }
        } else if c == '*' && chars.peek() == Some(&'*') {
            chars.next();
            flush(&mut buf, &mut out);
            let mut bold = String::new();
            let mut closed = false;
            while let Some(n) = chars.next() {
                if n == '*' && chars.peek() == Some(&'*') {
                    chars.next();
                    closed = true;
                    break;
                }
                bold.push(n);
            }
            if closed && !bold.is_empty() {
                let g = painter.layout(
                    bold,
                    egui::FontId::proportional(size),
                    TEXT,
                    f32::INFINITY,
                );
                out.push(Seg::Bold(g));
            }
        } else {
            buf.push(c);
        }
    }
    flush(&mut buf, &mut out);
    out
}

/// Greedy wrap of inline segments into visual lines.
fn wrap_segments(segs: Vec<Seg>, width: f32) -> Vec<Vec<Seg>> {
    let mut lines: Vec<Vec<Seg>> = Vec::new();
    let mut current: Vec<Seg> = Vec::new();
    let mut used = 0.0f32;
    for seg in segs {
        let w = match &seg {
            Seg::Plain(g) | Seg::Bold(g) => g.size().x,
            Seg::Code(g) => g.size().x + 12.0,
        };
        if used > 0.0 && used + w > width {
            lines.push(std::mem::take(&mut current));
            used = 0.0;
        }
        used += w + 3.0;
        current.push(seg);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// One drawable block of the answer.
enum Block {
    Para { lines: Vec<Vec<Seg>>, bullet: bool },
    Code { lines: Vec<Arc<egui::Galley>> },
    Table { header: Vec<Arc<egui::Galley>>, rows: Vec<Vec<Arc<egui::Galley>>>, cols: usize },
}

impl Block {
    fn height(&self) -> f32 {
        match self {
            Block::Para { lines, .. } => lines
                .iter()
                .map(|l| l.iter().map(|s| match s { Seg::Plain(g) | Seg::Bold(g) | Seg::Code(g) => g.size().y }).fold(0.0f32, f32::max) + 2.0)
                .sum::<f32>(),
            Block::Code { lines } => {
                lines.iter().map(|l| l.size().y).sum::<f32>() + 12.0
            }
            Block::Table { header, rows, .. } => {
                let head = header.iter().map(|g| g.size().y).fold(0.0f32, f32::max);
                let body: f32 = rows
                    .iter()
                    .map(|r| r.iter().map(|g| g.size().y).fold(0.0f32, f32::max) + 8.0)
                    .sum();
                head + 6.0 + body + 4.0
            }
        }
    }
}

fn split_row(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_owned()).collect()
}

fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
        || (t.starts_with('-') && t.len() > 2 && t.chars().all(|c| c == '-'))
}

struct App {
    path: PathBuf,
    state: State,
    fonts_done: bool,
    passthrough_done: bool,
    visible: bool,
    idle_since: Option<Instant>,
    seen: Instant,
    height: f32,
    alpha: f32,
    last_size: egui::Vec2,
    last_pos: egui::Pos2,
}

impl App {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: State::default(),
            fonts_done: false,
            passthrough_done: false,
            visible: false,
            idle_since: None,
            seen: Instant::now() - Duration::from_secs(60),
            height: MIN_HEIGHT,
            alpha: 0.0,
            last_size: egui::vec2(0.0, 0.0),
            last_pos: egui::pos2(0.0, 0.0),
        }
    }

    fn reload(&mut self) {
        if let Ok(text) = std::fs::read_to_string(&self.path) {
            if text.trim().is_empty() {
                return;
            }
            if let Ok(next) = serde_json::from_str::<State>(&text) {
                if next.ts != self.state.ts {
                    self.seen = Instant::now();
                }
                self.state = next;
            }
        }
    }

    fn clock(&self) -> String {
        let since = if self.state.since > 0 { self.state.since } else { self.state.ts };
        let seconds = if since > 0 { ((now_ms() - since) / 1000).max(0) } else { 0 };
        if !(self.state.active && self.seen.elapsed() < STALE) {
            return String::new();
        }
        if seconds < 60 {
            format!("{seconds} с")
        } else {
            format!("{}:{:02}", seconds / 60, seconds % 60)
        }
    }

    fn text_galley(&self, painter: &egui::Painter, text: &str, size: f32, color: egui::Color32, wrap: f32) -> Arc<egui::Galley> {
        painter.layout(text.to_owned(), egui::FontId::proportional(size), color, wrap)
    }

    /// Turn the raw answer into blocks the panel can draw.
    fn parse(&self, painter: &egui::Painter, body: &str, dim: bool) -> Vec<Block> {
        let color = if dim { DIM } else { TEXT };
        let wrap = WIDTH - PAD_X * 2.0;
        let mut blocks = Vec::new();
        let lines: Vec<&str> = body.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let raw = lines[i];
            let trimmed = raw.trim();

            if trimmed.is_empty() {
                i += 1;
                continue;
            }

            // fenced code
            if trimmed.starts_with("```") {
                i += 1;
                let mut code = Vec::new();
                while i < lines.len() && !lines[i].trim().starts_with("```") {
                    let text = lines[i].to_owned();
                    code.push(painter.layout(
                        text,
                        egui::FontId::monospace(CODE_SIZE),
                        CODE_TEXT,
                        wrap - 16.0,
                    ));
                    i += 1;
                }
                i += 1;
                blocks.push(Block::Code { lines: code });
                continue;
            }

            // table
            if trimmed.starts_with('|') {
                let mut table: Vec<Vec<String>> = Vec::new();
                while i < lines.len() && lines[i].trim().starts_with('|') {
                    if !is_rule(lines[i]) {
                        table.push(split_row(lines[i]));
                    }
                    i += 1;
                }
                if !table.is_empty() {
                    let cols = table.iter().map(|r| r.len()).max().unwrap_or(1);
                    let cell_w = (wrap - (cols as f32 - 1.0) * 8.0) / cols as f32;
                    let mut header = Vec::new();
                    let mut rows = Vec::new();
                    for (idx, row) in table.iter().enumerate() {
                        let mut cells = Vec::new();
                        for c in row.iter() {
                            let font = egui::FontId::proportional(if idx == 0 { 11.5 } else { 11.0 });
                            let col = if idx == 0 { DIM } else { color };
                            let plain = c.replace("**", "").replace('`', "");
                            cells.push(painter.layout(plain, font, col, cell_w));
                        }
                        if idx == 0 {
                            header = cells;
                        } else {
                            rows.push(cells);
                        }
                    }
                    blocks.push(Block::Table { header, rows, cols });
                }
                continue;
            }

            // heading
            if let Some(rest) = trimmed.strip_prefix('#') {
                let text = rest.trim_start_matches('#').trim();
                let segs = parse_inline(painter, text, FONT_SIZE + 1.5, TEXT, wrap);
                let lines = wrap_segments(segs, wrap);
                blocks.push(Block::Para { lines, bullet: false });
                i += 1;
                continue;
            }

            // bullet or numbered item
            let (bullet, text) = if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).or_else(|| trimmed.strip_prefix("• ")) {
                (true, rest.to_owned())
            } else {
                let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    let rest = trimmed[digits.len()..].trim_start();
                    if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
                        (true, format!("{digits}. {rest}"))
                    } else {
                        (false, trimmed.to_owned())
                    }
                } else {
                    (false, trimmed.to_owned())
                }
            };

            let segs = parse_inline(painter, &text, FONT_SIZE, color, wrap - if bullet { 12.0 } else { 0.0 });
            let lines = wrap_segments(segs, wrap - if bullet { 12.0 } else { 0.0 });
            blocks.push(Block::Para { lines, bullet });
            i += 1;
        }

        blocks
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if !self.fonts_done {
            install_fonts(&ctx);
            self.fonts_done = true;
        }
        if quit_hotkey() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !self.passthrough_done {
            ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
            self.passthrough_done = true;
        }

        self.reload();
        ctx.request_repaint_after(POLL);

        let working = self.state.active && self.seen.elapsed() < STALE;
        if working {
            self.idle_since = None;
        } else if self.idle_since.is_none() {
            self.idle_since = Some(Instant::now());
        }
        let idle = self.idle_since.map(|t| t.elapsed() > READ_GRACE).unwrap_or(false);

        let body = if !self.state.answer.is_empty() {
            self.state.answer.clone()
        } else {
            self.state.previous.clone()
        };
        let body_is_previous = self.state.answer.is_empty();
        let show = !body.is_empty() && !idle;

        // The window stays mapped: fading and sliding look smooth, showing
        // and hiding a native window does not animate at all.
        let target_alpha = if show { 1.0 } else { 0.0 };
        self.alpha += (target_alpha - self.alpha) * FADE_STEP;
        if (target_alpha - self.alpha).abs() < 0.01 {
            self.alpha = target_alpha;
        }
        if !self.visible {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
            self.visible = true;
        }

        let painter = ui.painter().clone();
        let wrap = WIDTH - PAD_X * 2.0;
        let header = painter.layout(self.state.title.clone(), egui::FontId::proportional(12.0), DIM, wrap);
        let clock = painter.layout(self.clock(), egui::FontId::proportional(SMALL), OK, wrap);
        let blocks = self.parse(&painter, &body, body_is_previous);

        let mut target = PAD_Y * 2.0 + header.size().y.max(clock.size().y) + GAP;
        if body_is_previous {
            target += SMALL + 2.0;
        }
        for block in &blocks {
            target += block.height() + 3.0;
        }
        target = if show { target.max(MIN_HEIGHT) } else { MIN_HEIGHT };

        self.height += (target - self.height) * SIZE_STEP;
        if (target - self.height).abs() < 0.5 {
            self.height = target;
        }

        let ceiling = dsh_top().unwrap_or(TOP_GAP);
        let floor = work_bottom() - BOTTOM_GAP;
        let height = self.height.min(floor - ceiling).max(MIN_HEIGHT);
        let pos = egui::pos2(
            (unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32 - WIDTH - 16.0).max(0.0),
            (floor - height).max(ceiling),
        );
        let size = egui::vec2(WIDTH, height);

        if (size - self.last_size).length() > 0.5 {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            self.last_size = size;
        }
        if (pos - self.last_pos).length() > 0.5 {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            self.last_pos = pos;
        }

        let alpha_byte = (BASE_ALPHA as f32 * self.alpha) as u8;
        let bg = egui::Color32::from_rgba_unmultiplied(12, 16, 20, alpha_byte);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(bg).inner_margin(egui::Margin::ZERO))
            .show(ui, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter();
                painter.rect_filled(rect, egui::CornerRadius::same(CORNER), bg);

                let mut y = rect.top() + PAD_Y;
                let left = rect.left() + PAD_X;
                painter.galley(egui::pos2(left, y), header.clone(), DIM);
                painter.galley(
                    egui::pos2(rect.right() - PAD_X - clock.size().x, y + (header.size().y - clock.size().y).max(0.0)),
                    clock.clone(),
                    OK,
                );
                y += header.size().y.max(clock.size().y) + GAP;

                if body_is_previous {
                    painter.text(
                        egui::pos2(left, y),
                        egui::Align2::LEFT_TOP,
                        "Прошлый ответ",
                        egui::FontId::proportional(SMALL),
                        DIM,
                    );
                    y += SMALL + 2.0;
                }

                for block in &blocks {
                    match block {
                        Block::Para { lines, bullet } => {
                            for (li, segs) in lines.iter().enumerate() {
                                if *bullet && li == 0 {
                                    painter.text(
                                        egui::pos2(left, y + 2.0),
                                        egui::Align2::LEFT_TOP,
                                        "•",
                                        egui::FontId::proportional(FONT_SIZE),
                                        DIM,
                                    );
                                }
                                let mut x = left + if *bullet { 12.0 } else { 0.0 };
                                let mut row_h = 0.0f32;
                                for seg in segs {
                                    match seg {
                                        Seg::Code(galley) => {
                                            let r = egui::Rect::from_min_size(
                                                egui::pos2(x, y + 1.0),
                                                egui::vec2(galley.size().x + 10.0, galley.size().y + 4.0),
                                            );
                                            painter.rect_filled(r, egui::CornerRadius::same(4), CODE_CHIP);
                                            painter.galley(egui::pos2(x + 5.0, y + 3.0), galley.clone(), CODE_TEXT);
                                            x += galley.size().x + 14.0;
                                            row_h = row_h.max(galley.size().y + 4.0);
                                        }
                                        Seg::Plain(galley) | Seg::Bold(galley) => {
                                            painter.galley(egui::pos2(x, y), galley.clone(), TEXT);
                                            x += galley.size().x + 4.0;
                                            row_h = row_h.max(galley.size().y);
                                        }
                                    }
                                }
                                y += row_h + 2.0;
                            }
                        }
                        Block::Code { lines } => {
                            let inner_h: f32 = lines.iter().map(|l| l.size().y).sum::<f32>() + 12.0;
                            let box_rect = egui::Rect::from_min_size(
                                egui::pos2(left, y),
                                egui::vec2(wrap, inner_h),
                            );
                            painter.rect_filled(box_rect, egui::CornerRadius::same(8), CODE_BG);
                            painter.rect_stroke(
                                box_rect,
                                egui::CornerRadius::same(8),
                                egui::Stroke::new(1.0, CODE_BORDER),
                                egui::StrokeKind::Inside,
                            );
                            let mut cy = y + 6.0;
                            for line in lines {
                                painter.galley(egui::pos2(left + 8.0, cy), line.clone(), CODE_TEXT);
                                cy += line.size().y;
                            }
                            y += inner_h + 3.0;
                        }
                        Block::Table { header: cells, rows, cols } => {
                            let cols = (*cols).max(1);
                            let gap = 8.0;
                            let cell_w = (wrap - (cols as f32 - 1.0) * gap) / cols as f32;
                            let head_h = cells.iter().map(|g| g.size().y).fold(0.0f32, f32::max);
                            let mut x = left;
                            for cell in cells {
                                painter.galley(egui::pos2(x, y), cell.clone(), DIM);
                                x += cell_w + gap;
                            }
                            y += head_h + 4.0;
                            painter.line_segment(
                                [egui::pos2(left, y), egui::pos2(left + wrap, y)],
                                egui::Stroke::new(1.0, TABLE_LINE),
                            );
                            y += 4.0;
                            for row in rows {
                                let row_h = row.iter().map(|g| g.size().y).fold(0.0f32, f32::max);
                                let mut cx = left;
                                for (c, cell) in row.iter().enumerate() {
                                    if c < cols {
                                        painter.galley(egui::pos2(cx, y), cell.clone(), TEXT);
                                    }
                                    cx += cell_w + gap;
                                }
                                y += row_h + 8.0;
                                painter.line_segment(
                                    [egui::pos2(left, y - 4.0), egui::pos2(left + wrap, y - 4.0)],
                                    egui::Stroke::new(1.0, TABLE_LINE.gamma_multiply(0.5)),
                                );
                            }
                            y += 4.0;
                        }
                    }
                }
            });
    }
}

fn main() -> eframe::Result<()> {
    let path = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(default_state_path);
    let (screen_w, screen_h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([WIDTH, MIN_HEIGHT])
            .with_position([
                (screen_w as f32 - WIDTH - 16.0).max(0.0),
                (screen_h as f32 - MIN_HEIGHT - BOTTOM_GAP).max(0.0),
            ])
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false)
            .with_resizable(false)
            .with_visible(false)
            .with_title("DSH activity"),
        ..Default::default()
    };

    eframe::run_native("DSH activity", options, Box::new(|_cc| Ok(Box::new(App::new(path)))))
}
