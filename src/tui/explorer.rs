//! A projection of the review, not a second collection of investigation targets.
use crate::{arr, n, s};
use ratatui::widgets::ListState;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

pub(super) fn files(review: &Value) -> Vec<&Value> {
    let mut files: Vec<_> = arr(&review["changes"]).iter().collect();
    for code in arr(&review["recent_code"]) {
        if !files.iter().any(|f| f["file"] == code["file"]) {
            files.push(code);
        }
    }
    files
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Target {
    pub file: String,
    pub symbol: Option<String>,
    pub line: usize,
}
impl Target {
    pub fn label(&self) -> String {
        match &self.symbol {
            Some(symbol) => format!("{} · {symbol}", self.file),
            None => self.file.clone(),
        }
    }
    pub fn selector(&self) -> String {
        // Line anchors also disambiguate identically named structs and impls.
        if self.symbol.is_none() && self.line <= 1 {
            self.file.clone()
        } else {
            format!("{}:{}", self.file, self.line.max(1))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Section,
    Folder,
    File,
    Symbol,
}
#[derive(Clone, Debug)]
pub(super) struct Row {
    pub key: String,
    pub label: String,
    pub depth: usize,
    pub kind: Kind,
    pub target: Option<Target>,
    pub expandable: bool,
    pub expanded: bool,
    pub count: usize,
    pub added: usize,
    pub removed: usize,
    pub session: bool,
    pub diff: bool,
}

#[derive(Default)]
struct Folder<'a> {
    folders: BTreeMap<String, Folder<'a>>,
    files: BTreeMap<String, &'a Value>,
}
impl<'a> Folder<'a> {
    fn insert(&mut self, path: &str, change: &'a Value) {
        if let Some((head, tail)) = path.split_once('/') {
            self.folders
                .entry(head.into())
                .or_default()
                .insert(tail, change);
        } else {
            self.files.insert(path.into(), change);
        }
    }
    fn count(&self) -> usize {
        self.files.len() + self.folders.values().map(Self::count).sum::<usize>()
    }
    fn flatten(&self, prefix: &str, depth: usize, explorer: &mut Explorer, historical: bool) {
        for (name, folder) in &self.folders {
            let path = format!("{prefix}{name}/");
            let folder_key=if historical{format!("history:{path}")}else{path.clone()};
            let expanded = !explorer.filter.is_empty() || !explorer.closed.contains(&folder_key);
            explorer.rows.push(Row {
                key: folder_key,
                label: name.clone(),
                depth,
                kind: Kind::Folder,
                target: None,
                expandable: true,
                expanded,
                count: folder.count(),
                added: 0,
                removed: 0,
                session: false,
                diff: false,
            });
            if expanded {
                folder.flatten(&path, depth + 1, explorer, historical);
            }
        }
        for (name, change) in &self.files {
            let file = s(&change["file"]);
            let symbols = arr(&change["symbols"]);
            let expanded = explorer.open_files.contains(file);
            explorer.rows.push(Row {
                key: file.into(),
                label: name.clone(),
                depth,
                kind: Kind::File,
                target: Some(Target {
                    file: file.into(),
                    symbol: None,
                    line: 1,
                }),
                expandable: !symbols.is_empty(),
                expanded,
                count: symbols.len(),
                added: arr(&change["added_lines"]).len(),
                removed: n(&change["removed_line_count"]),
                session: change["session_key"].is_string(),
                diff: change["diff"].is_string(),
            });
            if !expanded {
                continue;
            }
            let mut symbols: Vec<_> = symbols.iter().collect();
            symbols.sort_by_key(|symbol| {
                (
                    n(&symbol["start_line"]),
                    std::cmp::Reverse(n(&symbol["end_line"])),
                )
            });
            let mut parents: Vec<&Value> = vec![];
            let mut seen = HashSet::new();
            for symbol in symbols {
                let start = n(&symbol["start_line"]);
                let end = n(&symbol["end_line"]);
                let key = format!("{file}:{start}:{}", s(&symbol["symbol"]));
                if !seen.insert(key.clone()) {
                    continue;
                }
                while parents
                    .last()
                    .is_some_and(|p| n(&p["end_line"]) < end || n(&p["end_line"]) <= start)
                {
                    parents.pop();
                }
                let name = s(&symbol["symbol"]);
                let label = parents
                    .last()
                    .and_then(|p| name.strip_prefix(&format!("{}.", s(&p["symbol"]))))
                    .unwrap_or(name);
                explorer.rows.push(Row {
                    key,
                    label: label.into(),
                    depth: depth + 1 + parents.len(),
                    kind: Kind::Symbol,
                    target: Some(Target {
                        file: file.into(),
                        symbol: Some(name.into()),
                        line: start,
                    }),
                    expandable: false,
                    expanded: false,
                    count: 0,
                    added: 0,
                    removed: 0,
                    session: false,
                    diff: false,
                });
                parents.push(symbol);
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Explorer {
    pub rows: Vec<Row>,
    pub state: ListState,
    pub filter: String,
    pub reviewed: HashSet<String>,
    closed: HashSet<String>,
    open_files: HashSet<String>,
}
impl Explorer {
    fn section(&mut self,key:&str,label:&str,count:usize){
        self.rows.push(Row{key:key.into(),label:label.into(),depth:0,kind:Kind::Section,target:None,expandable:true,expanded:!self.filter.is_empty()||!self.closed.contains(key),count,added:0,removed:0,session:false,diff:false});
    }
    pub fn selected(&self) -> Option<&Row> {
        self.state.selected().and_then(|i| self.rows.get(i))
    }
    pub fn target(&self) -> Option<Target> {
        self.selected().and_then(|r| r.target.clone())
    }
    pub fn rebuild(&mut self, review: &Value) {
        let key = self.selected().map(|r| r.key.clone());
        let mut tree = Folder::default();
        let mut historical=Folder::default();
        let query = self.filter.to_lowercase();
        for change in files(review) {
            if s(&change["file"]).to_lowercase().contains(&query) {
                if arr(&review["changes"]).iter().any(|c|c["file"]==change["file"]){tree.insert(s(&change["file"]),change);}
                else{historical.insert(s(&change["file"]),change);}
            }
        }
        self.rows.clear();
        if historical.count()>0 {
            if tree.count()>0 {
                self.section("@changes","Current changes",tree.count());
                if !self.closed.contains("@changes")||!self.filter.is_empty(){tree.flatten("",1,self,false);}
            }
            self.section("@history","Earlier sessions",historical.count());
            if !self.closed.contains("@history")||!self.filter.is_empty(){historical.flatten("",1,self,true);}
        } else {tree.flatten("",0,self,false);}
        for row in &mut self.rows {
            if row.kind == Kind::File {
                row.session = arr(&review["recent_code"])
                    .iter()
                    .any(|r| r["file"] == row.key);
            }
        }
        let selected = key
            .and_then(|key| self.rows.iter().position(|r| r.key == key))
            .unwrap_or_else(|| {
                self.state
                    .selected()
                    .unwrap_or(0)
                    .min(self.rows.len().saturating_sub(1))
            });
        self.state
            .select((!self.rows.is_empty()).then_some(selected));
    }
    pub fn step(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let mut next = self
            .state
            .selected()
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(self.rows.len() - 1);
        while self.rows[next].kind==Kind::Section && !self.rows[next].expandable {
            let candidate=next.saturating_add_signed(if delta<0{-1}else{1}).min(self.rows.len()-1);
            if candidate==next{return;}
            next=candidate;
        }
        self.state.select(Some(next));
    }
    pub fn toggle(&mut self, review: &Value) {
        let Some(row) = self.selected().cloned().filter(|r| r.expandable) else {
            return;
        };
        match row.kind {
            Kind::Folder | Kind::Section => {
                if !self.filter.is_empty() {
                    return;
                }
                if !self.closed.remove(&row.key) {
                    self.closed.insert(row.key);
                }
            }
            Kind::File => {
                if !self.open_files.remove(&row.key) {
                    self.open_files.insert(row.key);
                }
            }
            Kind::Symbol => {}
        }
        self.rebuild(review);
    }
    pub fn expand(&mut self, review: &Value) {
        if self.selected().is_some_and(|r| r.expandable && !r.expanded) {
            self.toggle(review);
        } else if self.selected().is_some_and(|r| r.expanded) {
            self.step(1);
        }
    }
    pub fn collapse(&mut self, review: &Value) {
        let Some(row) = self.selected().cloned() else {
            return;
        };
        if row.expanded {
            self.toggle(review);
            return;
        }
        let index = self.state.selected().unwrap_or(0);
        if let Some(parent) = (0..index).rev().find(|&i| self.rows[i].depth < row.depth) {
            self.state.select(Some(parent));
        }
    }
    pub fn refresh(&mut self, old: &Value, new: &Value) {
        self.reviewed.retain(|file| {
            let before = files(old).into_iter().find(|c| c["file"] == *file);
            let after = files(new).into_iter().find(|c| c["file"] == *file);
            after.is_some()
                && before == after
                && old["file_hashes"][file] == new["file_hashes"][file]
                && arr(&old["recent_code"]).iter().find(|c| c["file"] == *file)
                    == arr(&new["recent_code"]).iter().find(|c| c["file"] == *file)
        });
        self.rebuild(new);
    }
}
