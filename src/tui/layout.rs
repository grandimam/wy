use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Divider {
    Files,
    Content,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub(super) struct PaneSizes {
    pub files: Option<u16>,
    pub code_percent: u16,
}
impl Default for PaneSizes {
    fn default() -> Self {
        Self {
            files: None,
            code_percent: 46,
        }
    }
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
    pub fn file_width(&self, width: u16, split: bool) -> u16 {
        let default = if split {
            (width / 5).clamp(24, 34)
        } else {
            (width / 3).clamp(28, 44)
        };
        let maximum = width.saturating_sub(if split { 58 } else { 33 }).max(20);
        self.files.unwrap_or(default).clamp(20, maximum)
    }
    pub fn code_width(&self, width: u16) -> u16 {
        let available = width.saturating_sub(1);
        let minimum = 28.min(available / 2);
        ((u32::from(available) * u32::from(self.code_percent.clamp(1, 99)) / 100) as u16)
            .clamp(minimum, available.saturating_sub(minimum))
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
            Divider::Files if self.areas.file_divider.width > 0 => {
                let split = self.areas.content_divider.width > 0;
                let requested = column.saturating_sub(self.areas.body.x);
                let mut sizes = PaneSizes {
                    files: Some(requested),
                    code_percent: self.pane_sizes.code_percent,
                };
                sizes.files = Some(sizes.file_width(self.areas.body.width, split));
                self.pane_sizes = sizes;
            }
            Divider::Content if self.areas.content_divider.width > 0 => {
                let available = self.areas.content.width.saturating_sub(1).max(1);
                let minimum = 28.min(available / 2);
                let requested = column
                    .saturating_sub(self.areas.content.x)
                    .clamp(minimum, available.saturating_sub(minimum));
                self.pane_sizes.code_percent = ((u32::from(requested) * 100
                    + u32::from(available) / 2)
                    / u32::from(available)) as u16;
            }
            _ => {}
        }
    }
    pub(super) fn adjust_pane(&mut self, delta: i16) -> Result<()> {
        let (divider, area) = if self.focus != Focus::Files && self.areas.content_divider.width > 0
        {
            (Divider::Content, self.areas.content_divider)
        } else {
            (Divider::Files, self.areas.file_divider)
        };
        if area.width == 0 {
            self.message("Widen the terminal or show the sidebar to adjust pane widths");
            return Ok(());
        }
        self.resize_pane(divider, area.x.saturating_add_signed(delta));
        self.save_layout()?;
        self.message(
            "Pane size saved · drag a divider or use [ / ] · /layout reset restores defaults",
        );
        Ok(())
    }
}
