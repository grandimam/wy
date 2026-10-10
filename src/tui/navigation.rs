use super::*;

#[derive(Clone)]
pub(super) struct ReadingState {
    pub key: String,
    pub document: Document,
    pub code: Option<Document>,
}
fn key(target: Option<String>, edit: &Value) -> String {
    format!(
        "{}|{}",
        target.unwrap_or_else(|| "all changes".into()),
        s(&edit["id"])
    )
}
pub(super) fn artifact_key(artifact: &Value) -> String {
    key(
        document::artifact_target(artifact).map(|t| t.selector()),
        &artifact["packet"]["focus_session_edit"],
    )
}
pub(super) fn code_key(code: &Document) -> String {
    key(
        code.target.as_ref().map(Target::selector),
        code.session_edit.as_ref().unwrap_or(&Value::Null),
    )
}
pub(super) fn options_key(options: &reasoning::Options) -> String {
    key(
        options.target.clone().or(options.file.clone()),
        options.session_edit.as_ref().unwrap_or(&Value::Null),
    )
}
impl Workspace {
    pub(super) fn current_key(&self) -> Option<String> {
        self.document
            .artifact
            .as_ref()
            .map(|a| artifact_key(a))
            .or_else(|| self.code.as_ref().map(code_key))
            .or_else(|| self.document.code().then(|| code_key(&self.document)))
    }
    pub(super) fn remember_view(&mut self) {
        let Some(key) = self.current_key() else {
            return;
        };
        if self.document.artifact.is_none() {
            return;
        }
        self.views.retain(|v| v.key != key);
        self.views.push(ReadingState {
            key,
            document: self.document.clone(),
            code: self.code.clone(),
        });
        if self.views.len() > 80 {
            self.views.remove(0);
        }
    }
    pub(super) fn cache_answer(&mut self, artifact: Arc<Value>) {
        let key = artifact_key(&artifact);
        self.answers.retain(|a| artifact_key(a) != key);
        self.answers.push(artifact);
        if self.answers.len() > 40 {
            self.answers.remove(0);
        }
    }
    pub(super) fn answer_for(&self, key: &str) -> Option<Arc<Value>> {
        self.answers
            .iter()
            .rev()
            .find(|a| artifact_key(a) == key)
            .cloned()
    }
    pub(super) fn local_notes(&self, code: &Document) -> Document {
        if let Some(edit) = &code.session_edit {
            if !arr(&self.review["sessions"])
                .iter()
                .any(|r| r["storage_key"] == edit["session_key"])
            {
                if let Ok((_, session)) = crate::history::saved_edit(&self.root, edit) {
                    let mut review = self.review.clone();
                    review["sessions"] = serde_json::json!([{"id":session["id"],"agent":session["agent"],"storage_key":edit["session_key"]}]);
                    return document::recorded(&review, &[session], code, self.brief);
                }
            }
        }
        document::recorded(&self.review, &self.sessions, code, self.brief)
    }
    pub(super) fn show_code(&mut self, code: Document, notes: bool) {
        let key = code_key(&code);
        self.remember_view();
        if !notes {
            if let Some(saved) = self.views.iter().find(|v| v.key == key).cloned() {
                self.document = if saved.document.kind == View::Recorded {
                    match self.answer_for(&key) {
                        Some(answer) => document::explanation(answer),
                        None => {
                            // Rebuild the reader (reasons may have changed) but keep the position.
                            let mut doc = self.local_notes(&code);
                            doc.scroll = saved.document.scroll;
                            doc
                        }
                    }
                } else {
                    saved.document
                };
                self.code = saved.code;
                let mut doc = self.document.clone();
                self.check_freshness(&mut doc);
                self.document = doc;
                return;
            }
            if let Some(answer) = self.answer_for(&key) {
                let mut doc = document::explanation(answer);
                self.check_freshness(&mut doc);
                self.document = doc;
                self.code = Some(code);
                return;
            }
        }
        self.document = self.local_notes(&code);
        self.code = Some(code);
    }
    pub(super) fn show_notes(&mut self) {
        let code = self
            .code
            .clone()
            .or_else(|| self.target().map(|t| document::preview(&self.review, t)));
        if let Some(code) = code {
            self.show_code(code, true);
            self.focus = Focus::Reader;
        }
    }
    pub(super) fn file_state(&self, file: &str) -> Option<(&'static str, Color)> {
        if self
            .job
            .as_ref()
            .is_some_and(|j| j.file.as_deref() == Some(file))
        {
            return Some(("working", AMBER));
        }
        if self.queue.iter().any(|q| q.file.as_deref() == Some(file)) {
            return Some(("queued", MUTED));
        }
        if self
            .answers
            .iter()
            .any(|a| document::artifact_target(a).is_some_and(|t| t.file == file))
        {
            return Some(("ready", ACCENT));
        }
        None
    }
}
