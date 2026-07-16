use anyhow::{anyhow, Context, Result};
use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};
use core_graphics::{
    geometry::CGRect,
    window::{
        kCGNullWindowID, kCGWindowBounds, kCGWindowLayer, kCGWindowListExcludeDesktopElements,
        kCGWindowListOptionOnScreenOnly, kCGWindowOwnerPID, CGWindowListCopyWindowInfo,
    },
};
use objc2::{rc::Retained, AnyThread, MainThreadMarker};
use objc2_app_kit::{NSImage, NSImageScaling, NSImageView, NSView, NSWindow};
use objc2_foundation::NSData;
use ohm_pet_core::Atlas;
use std::collections::HashMap;
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

pub struct NativeRenderer {
    image_view: Retained<NSImageView>,
    window: Retained<NSWindow>,
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
        let native_window = content_view
            .window()
            .ok_or_else(|| anyhow!("AppKit content view is not attached to a window"))?;
        let mut images = HashMap::new();
        images.insert((row, column), image);
        Ok(Self {
            image_view,
            window: native_window,
            images,
        })
    }

    pub fn walkable_surface(&self) -> Option<(i32, i32, i32)> {
        let windows = unsafe {
            let array = CGWindowListCopyWindowInfo(
                kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
                kCGNullWindowID,
            );
            if array.is_null() {
                return None;
            }
            CFArray::<CFDictionary<CFString, CFType>>::wrap_under_create_rule(array)
        };
        let layer_key = unsafe { CFString::wrap_under_get_rule(kCGWindowLayer) };
        let owner_pid_key = unsafe { CFString::wrap_under_get_rule(kCGWindowOwnerPID) };
        let bounds_key = unsafe { CFString::wrap_under_get_rule(kCGWindowBounds) };
        let current_pid = std::process::id() as i32;
        let screen_size = self.window.screen().map(|screen| screen.frame().size);
        let scale = self.window.backingScaleFactor();
        for window in windows.iter() {
            let Some(layer) = window
                .find(&layer_key)
                .and_then(|value| value.downcast::<CFNumber>())
                .and_then(|number| number.to_i32())
            else {
                continue;
            };
            let Some(owner_pid) = window
                .find(&owner_pid_key)
                .and_then(|value| value.downcast::<CFNumber>())
                .and_then(|number| number.to_i32())
            else {
                continue;
            };
            if layer != 0 || owner_pid == current_pid {
                continue;
            }
            let Some(bounds) = window
                .find(&bounds_key)
                .and_then(|value| value.downcast::<CFDictionary>())
                .and_then(|dictionary| CGRect::from_dict_representation(&dictionary))
            else {
                continue;
            };
            if bounds.size.width < 320.0 || bounds.size.height < 180.0 {
                continue;
            }
            if screen_size.is_some_and(|screen| {
                bounds.size.width >= screen.width * 0.94
                    && bounds.size.height >= screen.height * 0.90
            }) {
                return None;
            }
            return Some((
                (bounds.origin.x * scale).round() as i32,
                ((bounds.origin.x + bounds.size.width) * scale).round() as i32,
                (bounds.origin.y * scale).round() as i32,
            ));
        }
        None
    }

    pub fn pointer_vector(&self) -> (f64, f64) {
        let point = self.window.mouseLocationOutsideOfEventStream();
        let bounds = self.image_view.bounds();
        (
            point.x - bounds.size.width / 2.0,
            bounds.size.height / 2.0 - point.y,
        )
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
