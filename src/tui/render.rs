use super::*;

impl Workspace {
    pub(super) fn draw(&mut self, frame: &mut Frame) {
        frame.render_widget(
            Block::default().style(Style::default().bg(BG).fg(TEXT)),
            frame.area(),
        );
        self.areas = Areas::default();
        if frame.area().width < 32 || frame.area().height < 10 {
            frame.render_widget(
                Paragraph::new("wy · enlarge terminal to 32×10\nq quits")
                    .style(Style::default().fg(ACCENT)),
                frame.area(),
            );
            return;
        }
        let area = frame.area().inner(Margin::new(1, 0));
        let inline_input = self.document.artifact.is_some()
            && matches!(self.editing, None | Some(Input::Question));
        let input_height = if inline_input {
            0
        } else if self.editing.is_some() {
            3
        } else {
            0
        };
        let status_height = u16::from(self.job.is_some() || !self.status.is_empty());
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(input_height),
            Constraint::Length(status_height),
            Constraint::Length(1),
        ])
        .split(area);
        self.draw_header(frame, rows[0]);
        let narrow = area.width < 88;
        let paired = self.code.is_some() && self.document.artifact.is_some();
        let split = paired && area.width >= 108;
        let show_files = self.sidebar && (!narrow || self.focus == Focus::Files);
        let show_reader = !show_files || !narrow;
        let file_width = if show_files {
            if narrow {
                rows[1].width
            } else {
                if split {
                    (rows[1].width / 5).clamp(24, 34)
                } else {
                    (rows[1].width / 3).clamp(28, 44)
                }
            }
        } else {
            0
        };
        let columns = Layout::horizontal([
            Constraint::Length(file_width),
            Constraint::Length(u16::from(show_files && show_reader)),
            Constraint::Min(0),
        ])
        .split(rows[1]);
        if show_files {
            self.draw_files(frame, columns[0]);
        }
        if show_reader {
            if split {
                let panes = Layout::horizontal([
                    Constraint::Percentage(46),
                    Constraint::Length(1),
                    Constraint::Percentage(54),
                ])
                .split(columns[2]);
                self.draw_code(frame, panes[0]);
                self.draw_answer(frame, panes[2], inline_input);
            } else if paired && self.focus == Focus::Code {
                self.draw_code(frame, columns[2]);
            } else {
                self.draw_answer(frame, columns[2], inline_input);
            }
        }
        if !inline_input {
            self.draw_input(frame, rows[2]);
        }
        let status = if let Some(job) = &self.job {
            let spinner =
                ['◐', '◓', '◑', '◒'][(job.started.elapsed().as_millis() / 180 % 4) as usize];
            format!(
                " {spinner} {}s · {} · {} · Esc cancels",
                job.started.elapsed().as_secs(),
                job.scope,
                self.status
            )
        } else {
            format!(" {} {}", if self.error { "!" } else { "·" }, self.status)
        };
        frame.render_widget(
            Paragraph::new(status).style(Style::default().fg(if self.error {
                RED
            } else if self.job.is_some() {
                AMBER
            } else {
                MUTED
            })),
            rows[3],
        );
        let keys = if self.editing.is_some() {
            " Enter submit   Esc cancel   Ctrl+U clear"
        } else if area.width < 70 {
            " w Why this change?  Tab panes  ? help"
        } else if self.document.source_selection.is_some() && self.focus == Focus::Reader {
            " ↑↓ sources   Enter open   Esc read explanation   ? help"
        } else if self.document.artifact.is_some() && self.focus == Focus::Reader {
            " s Sources   1–9 open   Tab code   R Update answer   Esc back   ? help"
        } else if self.focus == Focus::Files {
            " ↑↓ navigate   Enter read   w Why this change?   Tab panes   ? help"
        } else if self.focus == Focus::Code {
            " ↑↓ code   w Why this change?   Tab explanation   ? help"
        } else {
            " ↑↓ scroll   w Why this change?   Tab files   Esc back   ? help"
        };
        frame.render_widget(
            Paragraph::new(keys).style(Style::default().fg(ACCENT).bg(PANEL)),
            rows[4],
        );
    }
    fn draw_code(&mut self, frame: &mut Frame, area: Rect) {
        if let Some(code) = self.code.take() {
            let answer = std::mem::replace(&mut self.document, code);
            self.draw_reader(frame, area, Focus::Code);
            self.code = Some(std::mem::replace(&mut self.document, answer));
        }
    }
    fn draw_answer(&mut self, frame: &mut Frame, area: Rect, inline_input: bool) {
        if inline_input {
            let parts = Layout::vertical([
                Constraint::Min(1),
                Constraint::Length(if self.editing.is_some() { 3 } else { 1 }),
            ])
            .split(area);
            self.draw_reader(frame, parts[0], Focus::Reader);
            self.draw_input(frame, parts[1]);
        } else {
            self.draw_reader(frame, area, Focus::Reader);
        }
    }
    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let repo = self.root.file_name().unwrap_or_default().to_string_lossy();
        let (added, removed) = document::totals(&self.review);
        let mut spans = vec![
            Span::styled(" wy ", Style::default().fg(BG).bg(ACCENT).bold()),
            Span::styled(format!("  {repo}  "), Style::default().fg(TEXT).bold()),
            Span::styled(
                format!("·  {} files  ", explorer::files(&self.review).len()),
                Style::default().fg(MUTED),
            ),
            Span::styled(format!("+{added} "), Style::default().fg(GREEN)),
            Span::styled(format!("−{removed}"), Style::default().fg(RED)),
        ];
        if area.width > 85 {
            spans.push(Span::styled(
                format!("    {} · on request", self.agent),
                Style::default().fg(MUTED),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
    fn panel(&self, title: impl Into<String>, focus: Focus) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(if self.focus == focus { ACCENT } else { BORDER }))
            .title(Line::styled(
                format!(" {} ", title.into()),
                Style::default()
                    .fg(if self.focus == focus { ACCENT } else { MUTED })
                    .bold(),
            ))
    }
    fn draw_files(&mut self, frame: &mut Frame, area: Rect) {
        let matched = explorer::files(&self.review)
            .into_iter()
            .filter(|c| {
                s(&c["file"])
                    .to_lowercase()
                    .contains(&self.explorer.filter.to_lowercase())
            })
            .count();
        let block = self
            .panel(format!("Files · {matched}"), Focus::Files)
            .style(Style::default().bg(PANEL));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let sections = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(if inner.height > 8 { 2 } else { 0 }),
        ])
        .split(inner);
        let filter = if self.explorer.filter.is_empty() {
            " f  filter files".into()
        } else {
            format!(" / {}", self.explorer.filter)
        };
        frame.render_widget(
            Paragraph::new(filter).style(Style::default().fg(MUTED)),
            sections[0],
        );
        self.areas.files = sections[1];
        let items = self
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
                let indent = "  ".repeat(row.depth.min(6));
                let reviewed = row.kind == Kind::File
                    && row
                        .target
                        .as_ref()
                        .is_some_and(|t| self.explorer.reviewed.contains(&t.file));
                let label = format!(
                    "{}{} {}{}",
                    indent,
                    icon,
                    row.label,
                    if row.kind == Kind::Folder { "/" } else { "" }
                );
                let suffix = match row.kind {
                    Kind::Folder => format!(" {}", row.count),
                    Kind::File if reviewed => " ✓".into(),
                    Kind::File => format!(
                        "{}{}",
                        if row.diff {
                            format!(" +{} −{}", row.added, row.removed)
                        } else {
                            String::new()
                        },
                        if row.session { " · session" } else { "" }
                    ),
                    Kind::Symbol => {
                        format!(" :{}", row.target.as_ref().map(|t| t.line).unwrap_or(0))
                    }
                };
                let available = sections[1].width.saturating_sub(3) as usize;
                let label = fit(
                    &label,
                    available
                        .saturating_sub(Line::from(suffix.as_str()).width())
                        .max(4),
                );
                ListItem::new(Line::from(vec![
                    Span::styled(
                        label,
                        Style::default().fg(match row.kind {
                            Kind::Folder => ACCENT,
                            Kind::File => {
                                if reviewed {
                                    GREEN
                                } else {
                                    TEXT
                                }
                            }
                            Kind::Symbol => MUTED,
                        }),
                    ),
                    Span::styled(
                        suffix,
                        Style::default().fg(if reviewed { GREEN } else { MUTED }),
                    ),
                ]))
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            frame.render_widget(
                Paragraph::new(if self.explorer.filter.is_empty() {
                    " No changed files or session code\n r refreshes"
                } else {
                    " No matching files\n f edits · Esc clears"
                })
                .style(Style::default().fg(MUTED)),
                sections[1],
            );
        } else {
            frame.render_stateful_widget(
                List::new(items)
                    .highlight_symbol("› ")
                    .highlight_style(Style::default().bg(SELECT).bold()),
                sections[1],
                &mut self.explorer.state,
            );
        }
        let detail = " ←→ expand · Space toggle";
        frame.render_widget(
            Paragraph::new(detail)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(MUTED))
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(BORDER)),
                ),
            sections[2],
        );
    }
    fn draw_reader(&mut self, frame: &mut Frame, area: Rect, pane: Focus) {
        if pane == Focus::Code {
            self.areas.code = area;
        } else {
            self.areas.reader = area;
        }
        let block = self
            .panel(self.document.kind.label(), pane)
            .padding(Padding::horizontal(1));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let title_height = if inner.height < 6 { 1 } else { 2 };
        let tabs_height =
            u16::from(inner.height >= 6 && !(pane == Focus::Reader && self.areas.code.width > 0));
        let notice_height = self
            .document
            .notice
            .as_ref()
            .map(|(text, _)| {
                Paragraph::new(text.as_str())
                    .wrap(Wrap { trim: false })
                    .line_count(inner.width)
                    .clamp(1, 3) as u16
            })
            .unwrap_or(0)
            .min(inner.height.saturating_sub(title_height + tabs_height + 1));
        let rows = Layout::vertical([
            Constraint::Length(tabs_height),
            Constraint::Length(title_height),
            Constraint::Length(notice_height),
            Constraint::Min(1),
        ])
        .split(inner);
        let mut x = rows[0].x;
        let file = self.document.target.as_ref().map(|t| t.file.as_str());
        let mut tabs = vec![];
        if file.is_none_or(|f| arr(&self.review["changes"]).iter().any(|c| c["file"] == f)) {
            tabs.push(("d", View::Diff));
        }
        if file.is_some_and(|f| document::recent_edit(&self.review, f).is_some()) {
            tabs.push(("c", View::SessionCode));
        }
        tabs.push(("w", View::Explanation));
        for (key, view) in tabs {
            let label = format!(" {key} {} ", view.label());
            let width = Line::from(label.as_str()).width() as u16;
            if rows[0].height == 0 || x + width > rows[0].right() {
                break;
            }
            let area = Rect::new(x, rows[0].y, width, 1);
            frame.render_widget(
                Paragraph::new(label).style(if self.document.kind == view {
                    Style::default().fg(ACCENT).bg(SELECT).bold()
                } else {
                    Style::default().fg(MUTED)
                }),
                area,
            );
            self.areas.tabs.push((area, view, pane));
            x += width + 1;
        }
        frame.render_widget(
            Paragraph::new(format!(
                "{}{}",
                if title_height > 1 { "\n" } else { "" },
                self.document.title
            ))
            .style(Style::default().fg(TEXT).bold()),
            rows[1],
        );
        if let Some((text, color)) = &self.document.notice {
            frame.render_widget(
                Paragraph::new(text.as_str())
                    .wrap(Wrap { trim: false })
                    .style(Style::default().fg(*color)),
                rows[2],
            );
        }
        let mut lines = self.document.lines.clone();
        let mut source_positions = vec![];
        for (position, (line, index)) in self.document.sources.iter().enumerate() {
            let offset = if *line == 0 {
                0
            } else {
                Paragraph::new(lines[..*line].to_vec())
                    .wrap(Wrap { trim: false })
                    .line_count(rows[3].width)
            };
            let height = Paragraph::new(lines[*line].clone())
                .wrap(Wrap { trim: false })
                .line_count(rows[3].width)
                .max(1);
            if self.document.source_selection == Some(position) {
                lines[*line] = lines[*line]
                    .clone()
                    .style(Style::default().fg(ACCENT).bg(SELECT).bold());
                let scroll = usize::from(self.document.scroll);
                if offset < scroll {
                    self.document.scroll = offset.min(u16::MAX as usize) as u16;
                } else if offset + height > scroll + usize::from(rows[3].height) {
                    self.document.scroll = (offset + height)
                        .saturating_sub(usize::from(rows[3].height))
                        .min(u16::MAX as usize) as u16;
                }
            }
            source_positions.push((offset, height, *index));
        }
        let mut paragraph = Paragraph::new(lines);
        if !self.document.code() {
            paragraph = paragraph.wrap(Wrap { trim: false });
        }
        let count = paragraph.line_count(rows[3].width);
        let scroll_max = count
            .saturating_sub(rows[3].height as usize)
            .min(u16::MAX as usize) as u16;
        self.document.scroll = self.document.scroll.min(scroll_max);
        for (offset, height, index) in source_positions {
            let scroll = usize::from(self.document.scroll);
            let top = offset.max(scroll);
            let bottom = (offset + height).min(scroll + usize::from(rows[3].height));
            if bottom > top {
                self.areas.sources.push((
                    Rect::new(
                        rows[3].x,
                        rows[3].y + (top - scroll) as u16,
                        rows[3].width,
                        (bottom - top) as u16,
                    ),
                    index,
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
                .saturating_sub(rows[3].width as usize)
                .min(u16::MAX as usize) as u16,
        );
        let page_size = rows[3].height.saturating_sub(2).max(1);
        if pane == Focus::Code {
            self.code_scroll_max = scroll_max;
            self.code_page_size = page_size;
        } else {
            self.scroll_max = scroll_max;
            self.page_size = page_size;
        }
        frame.render_widget(
            paragraph.scroll((self.document.scroll, self.document.horizontal)),
            rows[3],
        );
        if count > rows[3].height as usize && rows[3].height > 0 {
            let mut state = ScrollbarState::new(count)
                .position(self.document.scroll as usize)
                .viewport_content_length(rows[3].height as usize);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .thumb_style(Style::default().fg(ACCENT))
                    .track_style(Style::default().fg(BORDER)),
                Rect::new(area.right() - 1, rows[3].y, 1, rows[3].height),
                &mut state,
            );
        }
        if area.width > 22 && area.height > 3 {
            let position = format!(
                " {}–{} / {} ",
                usize::from(self.document.scroll) + 1,
                (usize::from(self.document.scroll) + usize::from(rows[3].height)).min(count),
                count
            );
            let width = position.len() as u16;
            frame.render_widget(
                Paragraph::new(position).style(Style::default().fg(MUTED).bg(BG)),
                Rect::new(
                    area.right().saturating_sub(width + 2),
                    area.bottom() - 1,
                    width,
                    1,
                ),
            );
        }
    }
    fn draw_input(&mut self, frame: &mut Frame, area: Rect) {
        self.areas.input = area;
        if let Some(mode) = self.editing {
            let label = match mode {
                Input::Command => format!("Command · {} · history {}", self.agent, self.source),
                Input::Question => format!("Ask {} · {}", self.agent, self.question_scope()),
                Input::Filter => "Filter file paths · live results".into(),
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(ACCENT))
                .title(format!(" {label} "));
            let inner = block.inner(area);
            frame.render_widget(block, area);
            let visible = input_tail(&self.input, inner.width.saturating_sub(1) as usize);
            let width = Line::from(visible.as_str()).width() as u16;
            frame.render_widget(
                Paragraph::new(visible).style(Style::default().fg(TEXT)),
                inner,
            );
            if inner.width > 0 && inner.height > 0 {
                frame.set_cursor_position((inner.x + width.min(inner.width - 1), inner.y));
            }
        } else if let Some(artifact) = &self.document.artifact {
            let scope = document::artifact_target(artifact)
                .map(|t| t.label())
                .unwrap_or_else(|| "all changes".into());
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(" i Follow-up ", Style::default().fg(ACCENT).bold()),
                    Span::styled(format!("· {scope}"), Style::default().fg(MUTED)),
                ])),
                area,
            );
        }
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
