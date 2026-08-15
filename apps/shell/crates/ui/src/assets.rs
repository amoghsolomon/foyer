use std::borrow::Cow;

use anyhow::anyhow;
use gpui::{AssetSource, SharedString};
use rust_embed::RustEmbed;

/// Foyer Shell-specific Lucide glyphs layered over gpui-component's bundled assets.
#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        if let Some(asset) = Self::get(path) {
            return Ok(Some(asset.data));
        }
        gpui_component_assets::Assets
            .load(path)
            .map_err(|_| anyhow!("could not find asset at path \"{path}\""))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let mut assets = gpui_component_assets::Assets.list(path)?;
        assets.extend(Self::iter().filter_map(|item| item.starts_with(path).then(|| item.into())));
        assets.sort();
        assets.dedup();
        Ok(assets)
    }
}
