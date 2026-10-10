//! Section navigation beside the reader; the file tree belongs only to Code.
use super::*;
use std::collections::HashMap;
use ratatui::widgets::{Block, Borders, BorderType, Padding};

/// Native panel chrome. A scrolled panel draws only its visible edges.
fn reader_block(request:bool)->Block<'static>{
    Block::default().border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if request{ACCENT}else{BORDER}))
        .borders(Borders::LEFT|Borders::RIGHT).padding(Padding::horizontal(1))
}

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
        let narrow = area.width < 88;
        let body = if narrow {
            self.draw_navigation(frame, rows[1], false);
            rows[2]
        } else {
            let columns=Layout::horizontal([Constraint::Length(14),Constraint::Min(0)]).split(rows[2]);
            self.draw_navigation(frame, columns[0], true);
            columns[1]
        };
        self.areas.body = body;
        let show_files = self.section()==View::Recorded && self.sidebar && (!narrow || self.focus == Focus::Files);
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
        let title = Line::from(vec![
            Span::styled(" wy", Style::default().fg(ACCENT).bold()),
            Span::styled(format!("  {repo}"), Style::default().fg(TEXT).bold()),

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
            Constraint::Length(1),
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
        if !filtering {
            frame.render_widget(Paragraph::new(if self.explorer.rows.iter().any(|r|r.kind==Kind::Section){" Files · f filters"}else{" Current changes · f filters"}).style(Style::default().fg(TEXT).bold()),sections[0]);
        }
        self.areas.files = sections[1];
        let available = sections[1].width.saturating_sub(3) as usize;
        let items: Vec<_> = self
            .explorer
            .rows
            .iter()
            .map(|row| {
                if row.kind==Kind::Section{return ListItem::new(Line::styled(format!("{} {} · {}",if row.expanded{"▾"}else{"▸"},row.label,row.count),Style::default().fg(TEXT).bold()));}
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
                let state:Option<(&str,Color)>=None;
                let suffix = match row.kind {
                    Kind::Section => String::new(),
                    Kind::Folder => format!(" {}", row.count),
                    Kind::File if reviewed => " ✓".into(),
                    Kind::File if row.diff => format!(" +{} −{}", row.added, row.removed),
                    Kind::File if row.session => String::new(),
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
                    Kind::Section => TEXT,
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
    fn draw_title(&mut self, frame: &mut Frame, area: Rect) {
        let state=self.document.target.as_ref().and_then(|t|self.file_state(&t.file));
        let suffix=state.map(|(s,_)|format!(" · explanation {s}")).unwrap_or_default();
        frame.render_widget(Paragraph::new(fit(&format!("{}{}",self.document.title,suffix),area.width as usize)).style(Style::default().fg(TEXT).bold()),area);
    }
    fn draw_navigation(&mut self,frame:&mut Frame,area:Rect,vertical:bool){
        let horizontal=Layout::horizontal([Constraint::Ratio(1,2);2]).split(area);
        for (index,(label,view)) in [("Decisions",View::DecisionOverview),("Sessions",View::Sessions)].into_iter().enumerate() {
            let rect=if vertical {Rect::new(area.x,area.y+index as u16*2,area.width.saturating_sub(1),1)}else{horizontal[index]};
            let selected=self.section()==view || (view==View::Sessions && self.section()==View::Recorded);
            let style=if selected{Style::default().fg(ACCENT).bold().add_modifier(Modifier::REVERSED)}else{Style::default().fg(TEXT)};
            frame.render_widget(Paragraph::new(format!(" {label}")).style(style),rect);
            self.areas.tabs.push((rect,view,Focus::Reader));
        }
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
        let panels=if content.width>=6{self.document.reader_panels()}else{vec![]};
        let mut panel_rows=HashMap::new();
        for (first,last,request) in &panels{
            for row in *first..=*last{panel_rows.insert(row,(*first,*last,*request));}
        }
        let mut indents=HashMap::new();
        if self.document.kind==View::SessionWork {
            for (index,line) in lines.iter_mut().enumerate().filter(|(i,_)|!panel_rows.contains_key(i)) {
                if let Some(first)=line.spans.first_mut() {
                    let indent=first.content.bytes().take_while(|b|*b==b' ').count().min(4).min(content.width.saturating_sub(1) as usize);
                    if indent>0 {first.content=first.content[indent..].to_owned().into();indents.insert(index,indent as u16);}
                }
            }
        }
        let panel_width=|request:bool|if request{content.width.min(100)}else{content.width};
        let row_width=|index:usize|panel_rows.get(&index).map(|(_,_,request)|reader_block(*request).inner(Rect::new(0,0,panel_width(*request),1)).width).unwrap_or(content.width.saturating_sub(*indents.get(&index).unwrap_or(&0)));
        // Compute wrapped offsets once. Re-laying out every preceding line for
        // each clickable row made long history views quadratic to redraw.
        let mut offsets=Vec::with_capacity(lines.len()+1);offsets.push(0usize);
        for (index,line) in lines.iter().enumerate() {
            let width=row_width(index).max(1);
            let edge=panel_rows.get(&index).is_some_and(|(first,last,_)|index==*first||index==*last);
            let height=if edge||self.document.code()||self.document.code_gutters.contains_key(&index){1}else{Paragraph::new(line.clone()).wrap(Wrap{trim:false}).line_count(width).max(1)};
            offsets.push(offsets.last().unwrap()+height);
        }
        for (position, (line, link)) in self.document.sources.iter().enumerate() {
            let offset=offsets[*line];
            let height=offsets[*line+1]-offset;
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
            source_positions.push((offset, height, panel_rows.get(line).map(|(_,_,request)|panel_width(*request)).unwrap_or(content.width), link.clone()));
        }
        let count = *offsets.last().unwrap_or(&0);
        let scroll_max = count
            .saturating_sub(content.height as usize)
            .min(u16::MAX as usize) as u16;
        self.document.scroll = self.document.scroll.min(scroll_max);
        for (offset, height, width, link) in source_positions {
            let scroll = usize::from(self.document.scroll);
            let top = offset.max(scroll);
            let bottom = (offset + height).min(scroll + usize::from(content.height));
            if bottom > top {
                self.areas.sources.push((
                    Rect::new(
                        content.x,
                        content.y + (top - scroll) as u16,
                        width,
                        (bottom - top) as u16,
                    ),
                    link,
                ));
            }
        }
        let overflow=self.document.lines.iter().enumerate().filter(|(i,_)|self.document.code()||self.document.code_gutters.contains_key(i)).map(|(i,line)|line.width().saturating_sub(row_width(i) as usize)).max().unwrap_or(0);
        self.document.horizontal=self.document.horizontal.min(overflow.min(u16::MAX as usize) as u16);
        self.scroll_max = scroll_max;
        self.page_size = content.height.saturating_sub(2).max(1);
        // Layout prose and code separately: prose wraps, code stays on one row.
        // Only render visible source rows; never materialize a fully wrapped document.
        let top=self.document.scroll as usize;let bottom=top+content.height as usize;
        for (first,last,request) in &panels{
            let start=offsets[*first];let end=offsets[*last+1];
            if end<=top||start>=bottom{continue;}
            let mut borders=Borders::LEFT|Borders::RIGHT;
            if start>=top{borders|=Borders::TOP;}
            if first!=last&&end<=bottom{borders|=Borders::BOTTOM;}
            let mut block=reader_block(*request).borders(borders);
            if start>=top{
                let mut spans=vec![Span::raw(" ")];spans.extend(lines[*first].spans.clone());spans.push(Span::raw(" "));
                block=block.title(Line::from(spans).style(lines[*first].style.add_modifier(Modifier::BOLD)));
            }
            frame.render_widget(block,Rect::new(content.x,content.y+(start.max(top)-top) as u16,panel_width(*request),(end.min(bottom)-start.max(top)) as u16));
        }
        for (index,line) in lines.iter().enumerate(){
            if offsets[index+1]<=top{continue;}if offsets[index]>=bottom{break;}
            let y=offsets[index].max(top);
            let rect=Rect::new(content.x,content.y+(y-top) as u16,content.width,(offsets[index+1].min(bottom)-y) as u16);
            let code=self.document.code()||self.document.code_gutters.contains_key(&index);
            let rect=if let Some((first,last,request))=panel_rows.get(&index){
                if index==*first||index==*last{continue;}
                reader_block(*request).inner(Rect::new(rect.x,rect.y,panel_width(*request),rect.height))
            }else{
                let indent=*indents.get(&index).unwrap_or(&0);
                Rect::new(rect.x+indent,rect.y,rect.width.saturating_sub(indent),rect.height)
            };
            if let Some(gutter)=self.document.code_gutters.get(&index){
                let width=(*gutter as u16).min(rect.width.saturating_sub(1));
                frame.render_widget(Paragraph::new(line.spans[0].clone()),Rect::new(rect.x,rect.y,width,1));
                let body=Line::from(line.spans[1..].to_vec());
                frame.render_widget(Paragraph::new(body).scroll((0,self.document.horizontal)),Rect::new(rect.x+width,rect.y,rect.width-width,1));
            }else if code{
                frame.render_widget(Paragraph::new(line.clone()).scroll((0,self.document.horizontal)),rect);
            }else{
                frame.render_widget(Paragraph::new(line.clone()).wrap(Wrap{trim:false}).scroll(((y-offsets[index]) as u16,0)),rect);
            }
            if code&&rect.width>0&&line.width()>rect.width as usize+self.document.horizontal as usize{
                frame.render_widget(Paragraph::new("›").style(Style::default().fg(ACCENT)),Rect::new(rect.right()-1,rect.y,1,1));
            }
        }
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
        } else if reader && self.document.kind==View::DecisionOverview {
            vec![("↑↓", "decisions"), ("Enter", "open"), ("e", "discover"), ("t", "session")]
        } else if reader && self.document.kind==View::SessionWork {
            vec![("s", "select"), ("Enter", "open"), ("d", "decisions"), ("b", "sessions")]
        } else if reader && self.document.kind==View::DecisionDetail {
            vec![("↑↓", "read"), ("s", "evidence"), ("d", "decisions"), ("Esc", "back")]
        } else if reader && self.document.source_selection.is_some() {
            vec![("↑↓", "select"), ("Enter", "open"), ("Esc", "done")]
        } else if reader && self.document.historical() {
            vec![("↑↓", "scroll"), ("Esc", "back"), ("?", "more")]
        } else if reader && self.document.kind == View::Explanation {
            vec![("i", "ask"), ("s", "sources"), ("Esc", "back"), ("?", "more")]
        } else if reader && self.document.kind == View::Recorded {
            vec![("Enter", "select action"), ("w", reasons), ("e", "explain"), ("?", "more")]
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
