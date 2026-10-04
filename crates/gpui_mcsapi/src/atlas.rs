use std::borrow::Cow;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use egui::epaint::{ImageData, ImageDelta, TextureId};
use egui::{Color32, ColorImage, TextureOptions};
use etagere::{BucketedAtlasAllocator, size2};
use gpui::{
    AtlasBackend, AtlasKey, AtlasState, AtlasTextureId, AtlasTextureKind, AtlasTile, DevicePixels,
    PlatformAtlas, Point, Size,
};
use parking_lot::Mutex;

use crate::scene::TextureSlot;

/// The side of a new atlas page. A tile larger than this gets a page of its own size.
const PAGE_SIDE: i32 = 1024;

/// One empty pixel around each tile, so linear filtering never samples a neighbour.
const PADDING: i32 = 1;

/// GPUI's sprite atlas, kept in egui textures.
///
/// Every page is a managed egui texture, allocated and updated through the context's texture
/// manager. egui applies those updates at the start of its next frame, before it paints the
/// shapes that sample them, so a tile is ready by the time a frame that uses it is shown.
pub(crate) struct EguiAtlas(Mutex<AtlasState<Pages>>);

pub(crate) struct Pages {
    context: egui::Context,
    pages: Vec<Option<Page>>,
}

struct Page {
    kind: AtlasTextureKind,
    allocator: BucketedAtlasAllocator,
    texture: TextureId,
    size: [usize; 2],
    tiles: usize,
}

impl EguiAtlas {
    pub(crate) fn new(context: egui::Context) -> Self {
        Self(Mutex::new(AtlasState::new(Pages {
            context,
            pages: Vec::new(),
        })))
    }

    /// Where `id` lives on the egui side, for painting a frame.
    pub(crate) fn slot(&self, id: AtlasTextureId) -> Option<TextureSlot> {
        let state = self.0.lock();
        let page = state.backend.pages.get(id.index as usize)?.as_ref()?;
        Some(TextureSlot {
            id: page.texture,
            size: [page.size[0] as f32, page.size[1] as f32],
        })
    }
}

impl Drop for Pages {
    fn drop(&mut self) {
        let textures = self.context.tex_manager();
        let mut textures = textures.write();
        for page in self.pages.iter().flatten() {
            textures.free(page.texture);
        }
    }
}

impl PlatformAtlas for EguiAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: AtlasKey,
        build: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
        self.0.lock().get_or_insert_with(key, build)
    }

    fn remove(&self, key: &AtlasKey) {
        self.0.lock().remove(key);
    }
}

impl AtlasBackend for Pages {
    fn insert(
        &mut self,
        kind: AtlasTextureKind,
        size: Size<DevicePixels>,
        bytes: &[u8],
    ) -> Result<AtlasTile> {
        let image = to_color_image(kind, size, bytes)?;
        let padded = size2(size.width.0 + 2 * PADDING, size.height.0 + 2 * PADDING);

        let existing = self.pages.iter_mut().enumerate().find_map(|(index, page)| {
            let page = page.as_mut().filter(|page| page.kind == kind)?;
            let allocation = page.allocator.allocate(padded)?;
            Some((index, allocation))
        });
        let (index, allocation) = match existing {
            Some(found) => found,
            None => {
                let index = self.new_page(
                    kind,
                    padded.width.max(PAGE_SIDE),
                    padded.height.max(PAGE_SIDE),
                );
                let page = self.pages[index]
                    .as_mut()
                    .context("a page that was just created is missing")?;
                let allocation = page
                    .allocator
                    .allocate(padded)
                    .context("a fresh atlas page cannot hold the tile it was sized for")?;
                (index, allocation)
            }
        };

        let page = self.pages[index]
            .as_mut()
            .context("the atlas page for a new tile is missing")?;
        page.tiles += 1;
        let origin = Point {
            x: DevicePixels(allocation.rectangle.min.x + PADDING),
            y: DevicePixels(allocation.rectangle.min.y + PADDING),
        };
        self.context.tex_manager().write().set(
            page.texture,
            ImageDelta::partial(
                [origin.x.0 as usize, origin.y.0 as usize],
                image,
                TextureOptions::LINEAR,
            ),
        );
        Ok(AtlasTile {
            texture_id: AtlasTextureId {
                index: index as u32,
                kind,
            },
            tile_id: allocation.id.into(),
            padding: 0,
            bounds: gpui::Bounds { origin, size },
        })
    }

    fn remove(&mut self, tile: AtlasTile) {
        let index = tile.texture_id.index as usize;
        let Some(Some(page)) = self.pages.get_mut(index) else {
            log::warn!("removing a tile from atlas page {index}, which does not exist");
            return;
        };
        page.allocator.deallocate(tile.tile_id.into());
        page.tiles = page.tiles.saturating_sub(1);
        if page.tiles == 0 {
            self.context.tex_manager().write().free(page.texture);
            self.pages[index] = None;
        }
    }
}

impl Pages {
    fn new_page(&mut self, kind: AtlasTextureKind, width: i32, height: i32) -> usize {
        let size = [width as usize, height as usize];
        let texture = self.context.tex_manager().write().alloc(
            format!("gpui atlas {kind:?}"),
            ImageData::Color(Arc::new(ColorImage::filled(size, Color32::TRANSPARENT))),
            TextureOptions::LINEAR,
        );
        let page = Page {
            kind,
            allocator: BucketedAtlasAllocator::new(size2(width, height)),
            texture,
            size,
            tiles: 0,
        };
        match self.pages.iter().position(Option::is_none) {
            Some(index) => {
                self.pages[index] = Some(page);
                index
            }
            None => {
                self.pages.push(Some(page));
                self.pages.len() - 1
            }
        }
    }
}

/// Converts GPUI's tile bytes to egui's premultiplied colors.
///
/// Monochrome tiles are coverage, which becomes white with that alpha so a sprite's vertex
/// color tints it. Polychrome tiles are straight-alpha BGRA. Subpixel tiles hold one coverage
/// value per channel; egui blends whole pixels, so they become plain coverage.
fn to_color_image(
    kind: AtlasTextureKind,
    size: Size<DevicePixels>,
    bytes: &[u8],
) -> Result<ColorImage> {
    let width = usize::try_from(size.width.0).context("negative tile width")?;
    let height = usize::try_from(size.height.0).context("negative tile height")?;
    let pixels: Vec<Color32> = match kind {
        AtlasTextureKind::Monochrome => {
            anyhow::ensure!(bytes.len() >= width * height, "monochrome tile is short");
            bytes[..width * height]
                .iter()
                .map(|alpha| Color32::from_white_alpha(*alpha))
                .collect()
        }
        AtlasTextureKind::Polychrome => {
            anyhow::ensure!(
                bytes.len() >= width * height * 4,
                "polychrome tile is short"
            );
            bytes[..width * height * 4]
                .chunks_exact(4)
                .map(|bgra| Color32::from_rgba_unmultiplied(bgra[2], bgra[1], bgra[0], bgra[3]))
                .collect()
        }
        AtlasTextureKind::Subpixel => {
            anyhow::ensure!(bytes.len() >= width * height * 4, "subpixel tile is short");
            bytes[..width * height * 4]
                .chunks_exact(4)
                .map(|channels| {
                    let coverage =
                        (channels[0] as u16 + channels[1] as u16 + channels[2] as u16) / 3;
                    Color32::from_white_alpha(coverage as u8)
                })
                .collect()
        }
    };
    Ok(ColorImage::new([width, height], pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(width: i32, height: i32) -> Size<DevicePixels> {
        Size {
            width: DevicePixels(width),
            height: DevicePixels(height),
        }
    }

    #[test]
    fn polychrome_tiles_are_bgra() {
        let image = to_color_image(AtlasTextureKind::Polychrome, size(1, 1), &[10, 20, 30, 255])
            .expect("a whole pixel converts");
        assert_eq!(image.pixels[0], Color32::from_rgb(30, 20, 10));
    }

    #[test]
    fn short_tiles_are_refused() {
        assert!(to_color_image(AtlasTextureKind::Monochrome, size(2, 2), &[0; 3]).is_err());
    }

    #[test]
    fn tiles_reuse_freed_space() -> Result<()> {
        let context = egui::Context::default();
        let mut pages = Pages {
            context,
            pages: Vec::new(),
        };
        let tile = pages.insert(AtlasTextureKind::Monochrome, size(8, 8), &[255; 64])?;
        assert!(tile.bounds.origin.x.0 >= PADDING && tile.bounds.origin.y.0 >= PADDING);
        assert_eq!(tile.bounds.size, size(8, 8));
        pages.remove(tile);
        assert!(
            pages.pages.iter().all(Option::is_none),
            "an empty page is freed"
        );
        let again = pages.insert(AtlasTextureKind::Monochrome, size(8, 8), &[255; 64])?;
        assert_eq!(again.texture_id.index, 0, "the freed slot is reused");
        Ok(())
    }
}
