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
use objc2_app_kit::{
    NSColor, NSFont, NSImage, NSImageScaling, NSImageView, NSTextField, NSView, NSWindow,
};
use objc2_foundation::{NSData, NSPoint, NSRect, NSSize, NSString};
use ohm_pet_core::Atlas;
use std::collections::HashMap;
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

pub struct NativeRenderer {
    image_view: Retained<NSImageView>,
    activity_label: Retained<NSTextField>,
    activity_lines: Vec<String>,
    window: Retained<NSWindow>,
    images: HashMap<(u32, u32), Retained<NSImage>>,
}

impl Drop for NativeRenderer {
    fn drop(&mut self) {
        self.image_view.removeFromSuperview();
        self.activity_label.removeFromSuperview();
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
        let bounds = content_view.bounds();
        let pet_width = bounds.size.height * (192.0 / 208.0);
        image_view.setFrame(NSRect::new(
            NSPoint::new(bounds.size.width - pet_width, 0.0),
            NSSize::new(pet_width, bounds.size.height),
        ));
        content_view.addSubview(&image_view);

        let activity_label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        activity_label.setFrame(NSRect::new(
            NSPoint::new(8.0, 18.0),
            NSSize::new(
                (bounds.size.width - pet_width - 16.0).max(1.0),
                (bounds.size.height - 36.0).max(1.0),
            ),
        ));
        activity_label.setMaximumNumberOfLines(6);
        activity_label.setBordered(false);
        activity_label.setDrawsBackground(true);
        activity_label.setBackgroundColor(Some(&NSColor::colorWithWhite_alpha(0.08, 0.88)));
        activity_label.setTextColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.96)));
        activity_label.setFont(Some(&NSFont::systemFontOfSize(13.0)));
        activity_label.setHidden(true);
        content_view.addSubview(&activity_label);
        let native_window = content_view
            .window()
            .ok_or_else(|| anyhow!("AppKit content view is not attached to a window"))?;
        let mut images = HashMap::new();
        images.insert((row, column), image);
        Ok(Self {
            image_view,
            activity_label,
            activity_lines: Vec::new(),
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
        let frame = self.image_view.frame();
        (
            point.x - (frame.origin.x + frame.size.width / 2.0),
            frame.origin.y + frame.size.height / 2.0 - point.y,
        )
    }

    pub fn set_activity(&mut self, lines: &[String]) {
        if self.activity_lines == lines {
            return;
        }
        self.activity_lines = lines.to_vec();
        self.activity_label
            .setStringValue(&NSString::from_str(&lines.join("\n")));
        self.activity_label.setHidden(lines.is_empty());
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
