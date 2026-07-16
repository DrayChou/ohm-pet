use anyhow::{anyhow, Context, Result};
use objc2::{rc::Retained, AnyThread, MainThreadMarker};
use objc2_app_kit::{NSImage, NSImageScaling, NSImageView, NSView};
use objc2_foundation::NSData;
use ohm_pet_core::Atlas;
use std::collections::HashMap;
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

pub struct NativeRenderer {
    image_view: Retained<NSImageView>,
    images: HashMap<(u32, u32), Retained<NSImage>>,
}

impl Drop for NativeRenderer {
    fn drop(&mut self) {
        self.image_view.removeFromSuperview();
    }
}

impl NativeRenderer {
    pub fn attach(window: &Window, atlas: &Atlas, row: u32, column: u32) -> Result<Self> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| anyhow!("renderer must be created on the main thread"))?;
        let image = image_for_frame(atlas, row, column)?;
        let image_view = NSImageView::imageViewWithImage(&image, mtm);
        image_view.setImageScaling(NSImageScaling::ScaleAxesIndependently);

        let handle = window.window_handle().context("get native window handle")?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err(anyhow!("expected an AppKit window handle"));
        };
        let content_view = unsafe { &*(handle.ns_view.as_ptr().cast::<NSView>()) };
        image_view.setFrame(content_view.bounds());
        content_view.addSubview(&image_view);
        let mut images = HashMap::new();
        images.insert((row, column), image);
        Ok(Self { image_view, images })
    }

    pub fn render(&mut self, atlas: &Atlas, row: u32, column: u32) -> Result<()> {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.images.entry((row, column)) {
            entry.insert(image_for_frame(atlas, row, column)?);
        }
        self.image_view
            .setImage(self.images.get(&(row, column)).map(|image| &**image));
        Ok(())
    }
}

fn image_for_frame(atlas: &Atlas, row: u32, column: u32) -> Result<Retained<NSImage>> {
    let encoded = atlas
        .frame_png(row, column)
        .context("encode native frame")?;
    let data = NSData::with_bytes(&encoded);
    NSImage::initWithData(NSImage::alloc(), &data)
        .ok_or_else(|| anyhow!("AppKit rejected the frame image"))
}
