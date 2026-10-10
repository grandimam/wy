//! Pane-local mouse selection over rendered rows, including rows scrolled past.
use super::*;
use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Default)]
pub(super) struct Selection {
    pub area: Rect,
    rows: BTreeMap<usize, Vec<String>>,
    anchor: Option<(usize, u16)>,
    end: (usize, u16),
    pub active: bool,
    pointer: Option<Position>,
    pub link: Option<Link>,
    pub pending: Option<String>,
}

impl Selection {
    pub fn clear(&mut self) {
        self.anchor = None;
        self.active = false;
        self.pointer = None;
        self.link = None;
    }

    pub fn autoscroll(&self) -> Option<(Position, isize)> {
        let point = self.pointer?;
        let delta = self.edge_scroll(point);
        (self.active && delta != 0).then_some((point, delta))
    }

    fn position(&self, point: Position, scroll: u16) -> (usize, u16) {
        (
            usize::from(scroll)
                + usize::from(point.y.clamp(self.area.y, self.area.bottom() - 1) - self.area.y),
            point.x.clamp(self.area.x, self.area.right() - 1) - self.area.x,
        )
    }

    pub fn start(&mut self, point: Position, scroll: u16) -> bool {
        if self.area.is_empty() || !self.area.contains(point) {
            return false;
        }
        self.pointer = Some(point);
        self.end = self.position(point, scroll);
        self.anchor = Some(self.end);
        self.active = true;
        true
    }

    pub fn extend(&mut self, point: Position, scroll: u16) {
        if self.active && !self.area.is_empty() {
            self.pointer = Some(point);
            self.end = self.position(point, scroll);
        }
    }

    pub fn edge_scroll(&self, point: Position) -> isize {
        if point.y < self.area.y {
            -1
        } else if point.y >= self.area.bottom() {
            1
        } else {
            0
        }
    }

    fn bounds(&self) -> Option<((usize, u16), (usize, u16))> {
        let anchor = self.anchor?;
        (anchor != self.end).then_some((anchor.min(self.end), anchor.max(self.end)))
    }

    fn text(&self) -> Option<String> {
        let (start, end) = self.bounds()?;
        let mut lines = Vec::new();
        for row in start.0..=end.0 {
            let cells = self.rows.get(&row)?;
            let first = if row == start.0 {
                usize::from(start.1)
            } else {
                0
            };
            let last = if row == end.0 {
                usize::from(end.1) + 1
            } else {
                cells.len()
            };
            let mut text = String::new();
            let mut x = 0;
            while x < cells.len().min(last) {
                let symbol = &cells[x];
                let width = Line::from(symbol.as_str()).width().max(1);
                if x + width > first {
                    text.push_str(symbol);
                }
                x += width;
            }
            lines.push(text.trim_end().to_owned());
        }
        Some(lines.join("\n"))
    }

    pub fn finish(&mut self, point: Position, scroll: u16) -> Option<Link> {
        if !self.active {
            return None;
        }
        self.extend(point, scroll);
        self.active = false;
        let link = self.link.take();
        if self.bounds().is_none() && self.area.contains(point) {
            return link;
        }
        self.pending = self.text().filter(|text| !text.is_empty());
        None
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, scroll: u16) {
        if self.area != area {
            self.clear();
            self.rows.clear();
        }
        self.area = area;
        if self.anchor.is_none() {
            self.rows.clear();
        }
        let bounds = self.bounds();
        let buffer = frame.buffer_mut();
        for y in area.y..area.bottom() {
            let row = usize::from(scroll) + usize::from(y - area.y);
            self.rows.insert(
                row,
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect(),
            );
            if let Some((start, end)) = bounds {
                for x in area.x..area.right() {
                    let position = (row, x - area.x);
                    if position >= start && position <= end {
                        buffer[(x, y)].set_style(
                            Style::default().bg(ACCENT).fg(ratatui::style::Color::Black),
                        );
                    }
                }
            }
        }
    }
}

/// Prefer the desktop clipboard; OSC 52 also works over SSH in supporting terminals.
pub(super) fn copy(text: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let commands: &[(&str, &[&str])] = &[("pbcopy", &[])];
    #[cfg(not(target_os = "macos"))]
    let commands: &[(&str, &[&str])] = &[
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    for (program, args) in commands {
        if let Ok(mut child) = Command::new(program)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            let result = child.stdin.take().unwrap().write_all(text.as_bytes());
            let status = child.wait();
            if result.is_ok() && status.is_ok_and(|s| s.success()) {
                return Ok(());
            }
        }
    }
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let mut stdout = io::stdout().lock();
    write!(stdout, "\x1b]52;c;{encoded}\x07")?;
    stdout.flush()?;
    Ok(())
}
