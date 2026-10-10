use super::*;
use serde::{Deserialize, Serialize};

/// Draggable boundaries around the detail reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Divider {
    Files,
    Sessions,
    Requests,
}

/// Saved per repository. Unknown fields from older layouts are ignored.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct PaneSizes {
    pub files: Option<u16>,
    pub sessions: Option<u16>,
    pub requests: Option<u16>,
}
impl PaneSizes {
    pub fn load(root: &Path) -> Result<Self> {
        let store = crate::storage::Store::open(root)?;
        Ok(store
            .get("workspace", "layout")
            .ok()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default())
    }
    pub fn file_width(&self, width: u16) -> u16 {
        let default = (width / 4).clamp(24, 40);
        let maximum = width.saturating_sub(40).max(20);
        self.files.unwrap_or(default).clamp(20, maximum)
    }
}
impl Workspace {
    pub(super) fn save_layout(&self) -> Result<()> {
        crate::storage::Store::open(&self.root)?.put(
            "workspace",
            "layout",
            &serde_json::to_value(&self.pane_sizes)?,
        )
    }
    pub(super) fn resize_pane(&mut self, divider: Divider, column: u16) {
        match divider {
            Divider::Files => {
                if self.areas.file_divider.width == 0 { return; }
                let requested = column.saturating_sub(self.areas.body.x);
                let sizes = PaneSizes { files: Some(requested), ..Default::default() };
                self.pane_sizes.files = Some(sizes.file_width(self.areas.body.width));
            }
            Divider::Sessions => {
                let area = self.areas.session_divider;
                if area.width == 0 { return; }
                let current = area.x.saturating_sub(self.areas.workspace.x);
                let maximum = current.saturating_add(self.areas.body.width).saturating_sub(44).max(16);
                self.pane_sizes.sessions = Some(column.saturating_sub(self.areas.workspace.x).clamp(16, maximum));
            }
            Divider::Requests => {
                let area = self.areas.request_divider;
                if area.width == 0 { return; }
                let current = self.areas.workspace.right().saturating_sub(area.right());
                let maximum = current.saturating_add(self.areas.body.width).saturating_sub(44).max(18);
                self.pane_sizes.requests = Some(self.areas.workspace.right().saturating_sub(column.saturating_add(1)).clamp(18, maximum));
            }
        }
    }
    pub(super) fn adjust_pane(&mut self, delta: i16) -> Result<()> {
        let (divider, area, delta) = if self.request_nav_focus {
            (Divider::Requests, self.areas.request_divider, -delta)
        } else if self.areas.file_divider.width > 0 {
            (Divider::Files, self.areas.file_divider, delta)
        } else {
            (Divider::Sessions, self.areas.session_divider, delta)
        };
        if area.width == 0 {
            self.message("Widen the terminal to resize this pane");
            return Ok(());
        }
        self.resize_pane(divider, area.x.saturating_add_signed(delta));
        self.save_layout()?;
        self.message("Pane width saved · /layout reset restores the defaults");
        Ok(())
    }
}
