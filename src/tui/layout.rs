use super::*;
use serde::{Deserialize, Serialize};

/// The only resizable boundary: between the file tree and the reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Divider {
    Files,
}

/// Saved per repository. Unknown fields from older layouts are ignored.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct PaneSizes {
    pub files: Option<u16>,
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
    pub(super) fn resize_pane(&mut self, Divider::Files: Divider, column: u16) {
        if self.areas.file_divider.width == 0 {
            return;
        }
        let requested = column.saturating_sub(self.areas.body.x);
        let sizes = PaneSizes {
            files: Some(requested),
        };
        self.pane_sizes.files = Some(sizes.file_width(self.areas.body.width));
    }
    pub(super) fn adjust_pane(&mut self, delta: i16) -> Result<()> {
        let area = self.areas.file_divider;
        if area.width == 0 {
            self.message("Widen the terminal or show the sidebar to adjust the file tree");
            return Ok(());
        }
        self.resize_pane(Divider::Files, area.x.saturating_add_signed(delta));
        self.save_layout()?;
        self.message("File tree width saved · /layout reset restores the default");
        Ok(())
    }
}
