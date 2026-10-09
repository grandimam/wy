//! Two areas: the file tree and one reader that switches between Changes and Notes.
//! No boxes; a single contextual hint line at the bottom.
use super::*;

impl Workspace {
    pub(super) fn draw(&mut self, frame: &mut Frame) {
        self.areas = Areas::default();
        if frame.area().width < 32 || frame.area().height < 10 {
            frame.render_widget(
                Paragraph::new("wy · enlarge terminal to 32×10\nq quits")
                    .style(Style::default().fg(ACCENT)),
                frame.area(),
            );
            return;
        }
        let area = frame.area();
        let status_height = u16::from(self.job.is_some() || !self.status.is_empty());
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(status_height),
            Constraint::Length(1),
        ])
        .split(area);
        self.draw_header(frame, rows[0]);
        let body = rows[2];
        self.areas.body = body;
        let narrow = area.width < 88;
        let show_files = self.sidebar && (!narrow || self.focus == Focus::Files);
        let show_reader = !show_files || !narrow;
        let file_width = match (show_files, narrow) {
            (false, _) => 0,
            (true, true) => body.width,
            (true, false) => self.pane_sizes.file_width(body.width),
        };
        let columns = Layout::horizontal([
            Constraint::Length(file_width),
            Constraint::Length(u16::from(show_files && show_reader)),
            Constraint::Min(0),
        ])
        .split(body);
        if show_files {
            self.draw_files(frame, columns[0]);
        }
        if show_files && show_reader {
            self.areas.file_divider = columns[1];
            self.draw_divider(frame, columns[1]);
        }
        if show_reader {
            let reader = columns[2].inner(Margin::new(2, 0));
            self.draw_reader(frame, reader);
        }
        self.draw_status(frame, rows[3]);
        if self.editing.is_some() {
            self.draw_input(frame, rows[4]);
        } else {
            self.draw_keys(frame, rows[4]);
        }
    }
    fn draw_header(&mut self, frame: &mut Frame, area: Rect) {
        let repo = self.root.file_name().unwrap_or_default().to_string_lossy();
        let files = explorer::files(&self.review).len();
        let title = Line::from(vec![
            Span::styled(" wy", Style::default().fg(ACCENT).bold()),
            Span::styled(format!("  {repo}"), Style::default().fg(TEXT).bold()),
            Span::styled(
                format!(" · {files} file{}", if files == 1 { "" } else { "s" }),
                Style::default().fg(MUTED),
            ),
        ]);
        frame.render_widget(Paragraph::new(title), area);
        let agent = Line::from(Span::styled(
            format!("{} ", self.agent),
            Style::default().fg(MUTED),
        ));
        frame.render_widget(Paragraph::new(agent).right_aligned(), area);
    }
    fn draw_divider(&self, frame: &mut Frame, area: Rect) {
        let lines: Vec<_> = (0..area.height).map(|_| Line::from("│")).collect();
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().fg(if self.dragging.is_some() {
                ACCENT
            } else {
                BORDER
            })),
            area,
        );
    }
    fn draw_files(&mut self, frame: &mut Frame, area: Rect) {
        // The filter line appears only while a filter is being typed or applied.
        let filtering = self.editing == Some(Input::Filter) || !self.explorer.filter.is_empty();
        let sections = Layout::vertical([
            Constraint::Length(u16::from(filtering)),
            Constraint::Min(1),
        ])
        .split(area);
        if filtering {
            frame.render_widget(
                Paragraph::new(format!(" / {}", self.explorer.filter))
                    .style(Style::default().fg(ACCENT)),
                sections[0],
            );
        }
        self.areas.files = sections[1];
        let available = sections[1].width.saturating_sub(3) as usize;
        let items: Vec<_> = self
            .explorer
            .rows
            .iter()
            .map(|row| {
                let icon = if row.expandable {
                    if row.expanded { "▾" } else { "▸" }
                } else if row.kind == Kind::Symbol {
                    "·"
                } else {
                    " "
                };
                let reviewed = row.kind == Kind::File
                    && row
                        .target
                        .as_ref()
                        .is_some_and(|t| self.explorer.reviewed.contains(&t.file));
                let label = format!(
                    "{}{icon} {}{}",
                    "  ".repeat(row.depth.min(6)),
                    row.label,
                    if row.kind == Kind::Folder { "/" } else { "" }
                );
                let state = row
                    .target
                    .as_ref()
                    .filter(|_| row.kind == Kind::File)
                    .and_then(|t| self.file_state(&t.file));
                let suffix = match row.kind {
                    Kind::File if state.is_some() => format!(" · {}", state.unwrap().0),
                    Kind::Folder => format!(" {}", row.count),
                    Kind::File if reviewed => " ✓".into(),
                    Kind::File if row.diff => format!(" +{} −{}", row.added, row.removed),
                    Kind::File if row.session => " · recorded".into(),
                    Kind::File => String::new(),
                    Kind::Symbol => {
                        format!(" :{}", row.target.as_ref().map(|t| t.line).unwrap_or(0))
                    }
                };
                let label = fit(
                    &label,
                    available
                        .saturating_sub(Line::from(suffix.as_str()).width())
                        .max(4),
                );
                let color = match row.kind {
                    Kind::Folder => ACCENT,
                    Kind::File if reviewed => GREEN,
                    Kind::File => TEXT,
                    Kind::Symbol => MUTED,
                };
                ListItem::new(Line::from(vec![
                    Span::styled(label, Style::default().fg(color)),
                    Span::styled(
                        suffix,
                        Style::default().fg(state
                            .map(|(_, color)| color)
                            .unwrap_or(if reviewed { GREEN } else { MUTED })),
                    ),
                ]))
            })
            .collect();
        if items.is_empty() {
            frame.render_widget(
                Paragraph::new(if self.explorer.filter.is_empty() {
                    " No changed files or recorded edits\n r refreshes"
                } else {
                    " No matching files\n Esc clears the filter"
                })
                .style(Style::default().fg(MUTED)),
                sections[1],
            );
        } else {
            let highlight = if self.focus == Focus::Files {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default().bold()
            };
            frame.render_stateful_widget(
                List::new(items)
                    .highlight_symbol("› ")
                    .highlight_style(highlight),
                sections[1],
                &mut self.explorer.state,
            );
        }
    }
    /// Title on the left; an Enriched tab on the right when a saved answer exists.
    fn draw_title(&mut self, frame: &mut Frame, area: Rect) {
        let mut x = area.right();
        let enriched = self.code.is_some()
            && self
                .current_key()
                .is_some_and(|k| self.answer_for(&k).is_some());
        if enriched && area.width > 30 {
            let label = "Enriched";
            let width = label.len() as u16 + 2;
            x -= width;
            let rect = Rect::new(x, area.y, width, 1);
            let style = if self.document.kind == View::Explanation {
                Style::default().fg(ACCENT).bold()
            } else {
                Style::default().fg(MUTED)
            };
            frame.render_widget(Paragraph::new(Span::styled(label, style)), rect);
            self.areas.tabs.push((rect, View::Explanation, Focus::Reader));
        }
        let title_width = x.saturating_sub(area.x).saturating_sub(1);
        frame.render_widget(
            Paragraph::new(fit(&self.document.title, title_width as usize))
                .style(Style::default().fg(TEXT).bold()),
            Rect::new(area.x, area.y, title_width, 1),
        );
    }
    fn draw_reader(&mut self, frame: &mut Frame, area: Rect) {
        self.areas.reader = area;
        let notice_height = self
            .document
            .notice
            .as_ref()
            .map(|(text, _)| {
                Paragraph::new(text.as_str())
                    .wrap(Wrap { trim: false })
                    .line_count(area.width)
                    .clamp(1, 2) as u16
            })
            .unwrap_or(0);
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(notice_height),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);
        self.draw_title(frame, rows[0]);
        if let Some((text, color)) = &self.document.notice {
            frame.render_widget(
                Paragraph::new(text.as_str())
                    .wrap(Wrap { trim: false })
                    .style(Style::default().fg(*color)),
                rows[1],
            );
        }
        let content = Rect::new(rows[3].x, rows[3].y, rows[3].width.saturating_sub(1), rows[3].height);
        let mut lines = self.document.lines.clone();
        let mut source_positions = vec![];
        for (position, (line, link)) in self.document.sources.iter().enumerate() {
            let offset = if *line == 0 {
                0
            } else {
                Paragraph::new(lines[..*line].to_vec())
                    .wrap(Wrap { trim: false })
                    .line_count(content.width)
            };
            let height = Paragraph::new(lines[*line].clone())
                .wrap(Wrap { trim: false })
                .line_count(content.width)
                .max(1);
            if self.document.source_selection == Some(position) {
                lines[*line] = lines[*line].clone().style(Style::default().add_modifier(Modifier::REVERSED));
                let scroll = usize::from(self.document.scroll);
                if offset < scroll {
                    self.document.scroll = offset.min(u16::MAX as usize) as u16;
                } else if offset + height > scroll + usize::from(content.height) {
                    self.document.scroll = (offset + height)
                        .saturating_sub(usize::from(content.height))
                        .min(u16::MAX as usize) as u16;
                }
            }
            source_positions.push((offset, height, link.clone()));
        }
        let mut paragraph = Paragraph::new(lines);
        if !self.document.code() {
            paragraph = paragraph.wrap(Wrap { trim: false });
        }
        let count = paragraph.line_count(content.width);
        let scroll_max = count
            .saturating_sub(content.height as usize)
            .min(u16::MAX as usize) as u16;
        self.document.scroll = self.document.scroll.min(scroll_max);
        for (offset, height, link) in source_positions {
            let scroll = usize::from(self.document.scroll);
            let top = offset.max(scroll);
            let bottom = (offset + height).min(scroll + usize::from(content.height));
            if bottom > top {
                self.areas.sources.push((
                    Rect::new(
                        content.x,
                        content.y + (top - scroll) as u16,
                        content.width,
                        (bottom - top) as u16,
                    ),
                    link,
                ));
            }
        }
        let longest = self
            .document
            .lines
            .iter()
            .map(Line::width)
            .max()
            .unwrap_or(0);
        self.document.horizontal = self.document.horizontal.min(
            longest
                .saturating_sub(content.width as usize)
                .min(u16::MAX as usize) as u16,
        );
        self.scroll_max = scroll_max;
        self.page_size = content.height.saturating_sub(2).max(1);
        frame.render_widget(
            paragraph.scroll((self.document.scroll, self.document.horizontal)),
            content,
        );
        if count > content.height as usize && content.height > 0 {
            let mut state = ScrollbarState::new(count)
                .position(self.document.scroll as usize)
                .viewport_content_length(content.height as usize);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .thumb_style(Style::default().fg(MUTED))
                    .track_style(Style::default().fg(BORDER)),
                Rect::new(rows[3].right() - 1, rows[3].y, 1, rows[3].height),
                &mut state,
            );
        }
    }
    fn draw_status(&mut self, frame: &mut Frame, area: Rect) {
        let status = if let Some(job) = &self.job {
            let spinner =
                ['◐', '◓', '◑', '◒'][(job.started.elapsed().as_millis() / 180 % 4) as usize];
            format!(
                " {spinner} {}s · {} · {}{} · x cancels",
                job.started.elapsed().as_secs(),
                job.scope,
                job.progress,
                if self.queue.is_empty() {
                    String::new()
                } else {
                    format!(" · {} queued", self.queue.len())
                }
            )
        } else {
            format!(" {}", self.status)
        };
        let color = if self.error {
            RED
        } else if self.job.is_some() {
            AMBER
        } else {
            MUTED
        };
        frame.render_widget(Paragraph::new(status).style(Style::default().fg(color)), area);
    }
    /// At most four keys, chosen for what is on screen.
    fn draw_keys(&mut self, frame: &mut Frame, area: Rect) {
        let reader = self.focus == Focus::Reader;
        let reasons = if self.brief { "reasons" } else { "brief" };
        let keys: Vec<(&str, &str)> = if reader && self.document.kind == View::Commits {
            vec![("↑↓", "commits"), ("Enter", "open"), ("Esc", "back")]
        } else if reader && self.document.source_selection.is_some() {
            vec![("↑↓", "select"), ("Enter", "open"), ("Esc", "done")]
        } else if reader && self.document.historical() {
            vec![("↑↓", "scroll"), ("Esc", "back"), ("?", "more")]
        } else if reader && self.document.kind == View::Explanation {
            vec![("i", "ask"), ("s", "sources"), ("Esc", "back"), ("?", "more")]
        } else if reader && self.document.kind == View::Recorded {
            vec![("Enter", "open turn"), ("w", reasons), ("e", "explain"), ("?", "more")]
        } else if self.focus == Focus::Files {
            vec![("↑↓", "files"), ("Tab", "read"), ("e", "explain"), ("?", "more")]
        } else {
            vec![("↑↓", "scroll"), ("Tab", "files"), ("e", "explain"), ("?", "more")]
        };
        let mut spans = vec![Span::raw(" ")];
        for (key, label) in keys {
            spans.push(Span::styled(key, Style::default().fg(ACCENT).bold()));
            spans.push(Span::styled(format!(" {label}   "), Style::default().fg(MUTED)));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
    fn draw_input(&mut self, frame: &mut Frame, area: Rect) {
        self.areas.input = area;
        let Some(mode) = self.editing else { return };
        let label = match mode {
            Input::Command => " / ".to_owned(),
            Input::Question => format!(" Ask {} · {} › ", self.agent, self.question_scope()),
            Input::Filter => " Filter › ".to_owned(),
        };
        let label_width = Line::from(label.as_str()).width() as u16;
        let room = area.width.saturating_sub(label_width + 1) as usize;
        let visible = input_tail(&self.input, room);
        let width = Line::from(visible.as_str()).width() as u16;
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(label, Style::default().fg(ACCENT).bold()),
                Span::styled(visible, Style::default().fg(TEXT)),
            ])),
            area,
        );
        frame.set_cursor_position((
            (area.x + label_width + width).min(area.right().saturating_sub(1)),
            area.y,
        ));
    }
}
fn fit(text: &str, width: usize) -> String {
    if Line::from(text).width() <= width {
        return text.into();
    }
    let mut result = String::new();
    for c in text.chars() {
        if Line::from(format!("{result}{c}…")).width() > width {
            break;
        }
        result.push(c);
    }
    result.push('…');
    result
}
fn input_tail(text: &str, width: usize) -> String {
    let mut offset = 0;
    while Line::from(&text[offset..]).width() > width {
        let Some(c) = text[offset..].chars().next() else {
            break;
        };
        offset += c.len_utf8();
    }
    text[offset..].to_owned()
}
