use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use matrix_sdk::{
    Client,
    media::{MediaFormat, MediaRequestParameters, UniqueKey},
};
use quick_cache::{Weighter, unsync::Cache};

use super::{Image, ImageError, ImageRequestPriority, ThumbnailDownloader, ThumbnailSettings};

const PIXEL_BUDGET: u64 = 32 * 1024 * 1024;

#[derive(Clone)]
pub struct ImageCache(Rc<CacheData>);
struct CacheData {
    entries: RefCell<Cache<String, Image, ImageWeight>>,
    generation: Cell<u64>,
}
#[derive(Clone)]
struct ImageWeight;
impl Weighter<String, Image> for ImageWeight {
    fn weight(&self, key: &String, image: &Image) -> u64 {
        image
            .first_frame
            .bytes
            .saturating_add(key.len() as u64)
            .saturating_add(256)
    }
}
impl Default for ImageCache {
    fn default() -> Self {
        Self(Rc::new(CacheData {
            entries: RefCell::new(Cache::with_weighter(256, PIXEL_BUDGET, ImageWeight)),
            generation: Cell::new(0),
        }))
    }
}
impl std::fmt::Debug for ImageCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageCache")
            .field("bytes", &self.0.entries.borrow().weight())
            .finish()
    }
}
impl ImageCache {
    pub fn clear(&self) {
        self.0
            .generation
            .set(self.0.generation.get().wrapping_add(1));
        self.0.entries.borrow_mut().clear();
    }
    pub async fn download(
        &self,
        downloader: ThumbnailDownloader<'_>,
        client: Client,
        settings: ThumbnailSettings,
        priority: ImageRequestPriority,
    ) -> Result<Image, ImageError> {
        let source = downloader.main.source.to_common_media_source();
        let alternate = downloader
            .alt
            .map(|source| source.source.to_common_media_source());
        let source_info = |source: super::ImageSource<'_>| {
            (
                source.dimensions().map(|d| (d.width, d.height)),
                source.filesize(),
                source.is_animated(),
                source.can_be_thumbnailed(),
            )
        };
        let source_info = (
            source_info(downloader.main),
            downloader.alt.map(source_info),
        );
        let rendition = MediaRequestParameters {
            source: source.clone(),
            format: MediaFormat::Thumbnail(settings.clone().into()),
        }
        .unique_key();
        // Include encryption metadata so cached pixels cannot bypass a changed key or
        // hash.
        let key = serde_json::to_string(&(
            client.homeserver(),
            client.user_id(),
            client.device_id(),
            source,
            alternate,
            rendition,
            settings.prefer_thumbnail,
            source_info,
        ))
        .map_err(|_| ImageError::Unknown)?;
        if let Some(image) = self.0.entries.borrow_mut().get(&key).cloned() {
            return Ok(image);
        }
        let generation = self.0.generation.get();
        let image = downloader.download(client, settings, priority).await?;
        if image.first_frame.delay.is_none() && self.0.generation.get() == generation {
            self.0.entries.borrow_mut().insert(key, image.clone());
        }
        Ok(image)
    }
}

#[cfg(test)]
mod tests {
    use gtk::{gdk, glib, prelude::*};

    use super::*;
    #[test]
    fn cached_static_frames_are_bounded_and_clearable() {
        let cache = ImageCache::default();
        let texture = gdk::MemoryTexture::new(
            2,
            2,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from_owned(vec![255; 16]),
            8,
        );
        let image = Image {
            file: None,
            decoder: None,
            first_frame: super::super::DisplayFrame {
                texture: texture.upcast(),
                delay: None,
                bytes: 16,
            },
            request_dimensions: Some(super::super::FrameDimensions {
                width: 2,
                height: 2,
            }),
        };
        cache
            .0
            .entries
            .borrow_mut()
            .insert("small".into(), image.clone());
        assert!(cache.0.entries.borrow_mut().get("small").is_some());
        let mut oversized = image;
        oversized.first_frame.bytes = PIXEL_BUDGET + 1;
        cache
            .0
            .entries
            .borrow_mut()
            .insert("oversized".into(), oversized);
        assert!(cache.0.entries.borrow().weight() <= PIXEL_BUDGET);
        assert!(cache.0.entries.borrow_mut().get("oversized").is_none());
        let generation = cache.0.generation.get();
        cache.clear();
        assert!(cache.0.entries.borrow().is_empty());
        assert_ne!(cache.0.generation.get(), generation);
    }
}
