//! The PhotoCraft brand mark at the left of the title bar: the app icon (the kitsune,
//! `assets/app-icon/`, see its README), where Photoshop shows its "Ps" tile. The PNG carries the
//! icon's rounded corners; the framework `Picture` stores the decoded image once and draws it at the
//! title bar's icon size.

use craft::ui::display::Picture;
use egui::Color32;
use photocraft_codecs::{ChannelLayout, SampleType};

/// 128 px: sharp at 20 pt on a 3× display.
const ICON_PNG: &[u8] = include_bytes!("../../../../assets/app-icon/hicolor/128x128/apps/ai.storyteller.photocraft.png");

/// The SpektraFilm mark (`PHOTOCRAFT_BRAND_MARK=spektra`, custom deployments that
/// install the app under their own product name). Same rounded-corner tile contract.
const SPEKTRA_PNG: &[u8] = include_bytes!("../../../../assets/app-icon/spektra/mark-128.png");

/// Whether the app is running under the `PhotoCraft-Spektrafilm` brand (`PHOTOCRAFT_BRAND_MARK=spektra`).
pub fn is_spektra() -> bool {
    static SPEKTRA: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SPEKTRA.get_or_init(|| std::env::var("PHOTOCRAFT_BRAND_MARK").as_deref() == Ok("spektra"))
}

/// The key the picture store keeps the decoded icon under.
const KEY: u64 = 0x7068_6f74_6f63_7266;

/// The icon's pixels (one transparent pixel if it couldn't be decoded, which a test rules out).
fn decode() -> egui::ColorImage {
    let bytes = if is_spektra() { SPEKTRA_PNG } else { ICON_PNG };
    let rgba = photocraft_codecs::decode(bytes).ok().map(|img| img.convert(ChannelLayout::Rgba, SampleType::U8));
    match rgba {
        Some(img) if img.data().len() == img.width() as usize * img.height() as usize * 4 => {
            egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.data())
        }
        _ => egui::ColorImage::filled([1, 1], Color32::TRANSPARENT),
    }
}

/// The brand mark as a decorative picture: the empty label keeps a screen reader from announcing it.
pub(crate) fn mark() -> Picture<'static> {
    Picture::new("", KEY, &decode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_decodes_to_a_rounded_128_px_tile() {
        let img = decode();
        assert_eq!(img.size, [128, 128]);
        // Rounded corners: the corner pixel is transparent, the centre opaque.
        assert_eq!(img.pixels[0].a(), 0);
        assert_eq!(img.pixels[64 * 128 + 64].a(), 255);
    }
}
