//! The few toolkit icons Kalem's own widgets ask for by name (menu check marks, submenu arrows).
//! Everything else is drawn from `icons.rs`; nothing else is bundled.

use gpui_kit::assets::IconName;
use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

pub struct KalemAssets;

impl AssetSource for KalemAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = if IconName::Check.path() == path {
            include_bytes!("../assets/icons/check.svg")
        } else if IconName::ChevronRight.path() == path {
            include_bytes!("../assets/icons/chevron-right.svg")
        } else if IconName::ChevronDown.path() == path {
            include_bytes!("../assets/icons/chevron-down.svg")
        } else if IconName::ChevronUp.path() == path {
            include_bytes!("../assets/icons/chevron-up.svg")
        } else {
            return Ok(None);
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
