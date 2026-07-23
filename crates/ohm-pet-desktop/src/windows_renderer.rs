use anyhow::{anyhow, Context, Result};
use ohm_pet_core::{Atlas, CELL_HEIGHT, CELL_WIDTH};
use std::{collections::HashMap, ffi::c_void, mem::size_of, ptr::copy_nonoverlapping};
use windows::Win32::{
    Foundation::{COLORREF, HWND, POINT, RECT, SIZE},
    Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, DrawTextW, GetDC, ReleaseDC,
        SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, DT_END_ELLIPSIS, DT_LEFT,
        DT_NOPREFIX, DT_WORDBREAK, HGDIOBJ, TRANSPARENT,
    },
    UI::WindowsAndMessaging::{
        GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, IsIconic,
        IsWindowVisible, IsZoomed, SetWindowLongPtrW, UpdateLayeredWindow, GWL_EXSTYLE, ULW_ALPHA,
        WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    },
};
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

pub struct NativeRenderer {
    hwnd: HWND,
    width: u32,
    height: u32,
    activity_lines: Vec<String>,
    pet_frames: HashMap<(u32, u32), Vec<u8>>,
}

impl NativeRenderer {
    pub fn attach(window: &Window, atlas: &Atlas, row: u32, column: u32) -> Result<Self> {
        let handle = window.window_handle().context("get native window handle")?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err(anyhow!("expected a Win32 window handle"));
        };
        let hwnd = HWND(handle.hwnd.get() as *mut c_void);
        let size = window.inner_size();
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd,
                GWL_EXSTYLE,
                (style & !(WS_EX_APPWINDOW.0 as isize))
                    | WS_EX_LAYERED.0 as isize
                    | WS_EX_NOACTIVATE.0 as isize
                    | WS_EX_TOOLWINDOW.0 as isize,
            );
        }
        let mut renderer = Self {
            hwnd,
            width: size.width.max(1),
            height: size.height.max(1),
            activity_lines: Vec::new(),
            pet_frames: HashMap::new(),
        };
        renderer.render(atlas, row, column)?;
        Ok(renderer)
    }

    pub fn walkable_surface(&self) -> Option<(i32, i32, i32)> {
        let foreground = unsafe { GetForegroundWindow() };
        if foreground.is_invalid()
            || foreground == self.hwnd
            || !unsafe { IsWindowVisible(foreground).as_bool() }
            || unsafe { IsIconic(foreground).as_bool() || IsZoomed(foreground).as_bool() }
        {
            return None;
        }
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(foreground, &mut rect) }.is_err()
            || rect.right - rect.left < 320
            || rect.bottom - rect.top < 180
        {
            return None;
        }
        Some((rect.left, rect.right, rect.top))
    }

    pub fn pointer_vector(&self) -> (f64, f64) {
        let mut cursor = POINT::default();
        let mut rect = RECT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
            let _ = GetWindowRect(self.hwnd, &mut rect);
        }
        let pet_width = f64::from(self.height) * f64::from(CELL_WIDTH) / f64::from(CELL_HEIGHT);
        let center_x = f64::from(rect.right) - pet_width / 2.0;
        let center_y = (f64::from(rect.top) + f64::from(rect.bottom)) / 2.0;
        (
            f64::from(cursor.x) - center_x,
            f64::from(cursor.y) - center_y,
        )
    }

    pub fn set_activity(&mut self, lines: &[String]) {
        if self.activity_lines != lines {
            self.activity_lines = lines.to_vec();
        }
    }

    pub fn render(&mut self, atlas: &Atlas, row: u32, column: u32) -> Result<()> {
        let pet_width = ((self.height as f64 * f64::from(CELL_WIDTH) / f64::from(CELL_HEIGHT))
            .round() as u32)
            .min(self.width);
        let bubble_width = self.width - pet_width;
        let pet_frame = if let Some(frame) = self.pet_frames.get(&(row, column)) {
            frame
        } else {
            let source =
                image::RgbaImage::from_raw(CELL_WIDTH, CELL_HEIGHT, atlas.frame_rgba(row, column))
                    .ok_or_else(|| anyhow!("invalid atlas frame"))?;
            let scaled = image::imageops::resize(
                &source,
                pet_width,
                self.height,
                image::imageops::FilterType::Nearest,
            );
            let mut frame = Vec::with_capacity((pet_width * self.height * 4) as usize);
            for pixel in scaled.pixels() {
                let alpha = u16::from(pixel[3]);
                frame.push(((u16::from(pixel[2]) * alpha) / 255) as u8);
                frame.push(((u16::from(pixel[1]) * alpha) / 255) as u8);
                frame.push(((u16::from(pixel[0]) * alpha) / 255) as u8);
                frame.push(pixel[3]);
            }
            self.pet_frames.insert((row, column), frame);
            self.pet_frames.get(&(row, column)).expect("inserted frame")
        };
        let mut bgra = vec![0_u8; (self.width * self.height * 4) as usize];
        if !self.activity_lines.is_empty() && bubble_width > 16 {
            for y in 12..self.height.saturating_sub(12) {
                for x in 6..bubble_width.saturating_sub(6) {
                    let offset = ((y * self.width + x) * 4) as usize;
                    bgra[offset] = 20;
                    bgra[offset + 1] = 20;
                    bgra[offset + 2] = 20;
                    bgra[offset + 3] = 224;
                }
            }
        }
        for y in 0..self.height {
            let source_start = (y * pet_width * 4) as usize;
            let destination_start = ((y * self.width + bubble_width) * 4) as usize;
            bgra[destination_start..destination_start + (pet_width * 4) as usize]
                .copy_from_slice(&pet_frame[source_start..source_start + (pet_width * 4) as usize]);
        }

        unsafe {
            let screen_dc = GetDC(None);
            if screen_dc.is_invalid() {
                return Err(anyhow!("GetDC failed"));
            }
            let memory_dc = CreateCompatibleDC(Some(screen_dc));
            if memory_dc.is_invalid() {
                let _ = ReleaseDC(None, screen_dc);
                return Err(anyhow!("CreateCompatibleDC failed"));
            }

            let mut bitmap_info = BITMAPINFO::default();
            bitmap_info.bmiHeader = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: self.width as i32,
                biHeight: -(self.height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(
                Some(memory_dc),
                &bitmap_info,
                DIB_RGB_COLORS,
                &mut bits,
                None,
                0,
            )?;
            if bits.is_null() {
                let _ = DeleteDC(memory_dc);
                let _ = ReleaseDC(None, screen_dc);
                return Err(anyhow!("CreateDIBSection returned no pixel buffer"));
            }
            copy_nonoverlapping(bgra.as_ptr(), bits.cast::<u8>(), bgra.len());
            let previous = SelectObject(memory_dc, HGDIOBJ(bitmap.0));
            if !self.activity_lines.is_empty() && bubble_width > 16 {
                let mut text: Vec<u16> = self.activity_lines.join("\r\n").encode_utf16().collect();
                let mut text_rect = RECT {
                    left: 16,
                    top: 22,
                    right: bubble_width as i32 - 14,
                    bottom: self.height as i32 - 18,
                };
                let _ = SetBkMode(memory_dc, TRANSPARENT);
                let _ = SetTextColor(memory_dc, COLORREF(0x00FF_FFFF));
                DrawTextW(
                    memory_dc,
                    &mut text,
                    &mut text_rect,
                    DT_LEFT | DT_WORDBREAK | DT_END_ELLIPSIS | DT_NOPREFIX,
                );
                for y in 12..self.height.saturating_sub(12) {
                    for x in 6..bubble_width.saturating_sub(6) {
                        let offset = ((y * self.width + x) * 4 + 3) as usize;
                        *bits.cast::<u8>().add(offset) = 224;
                    }
                }
            }

            let mut rect = RECT::default();
            GetWindowRect(self.hwnd, &mut rect)?;
            let destination = POINT {
                x: rect.left,
                y: rect.top,
            };
            let source_point = POINT::default();
            let size = SIZE {
                cx: self.width as i32,
                cy: self.height as i32,
            };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let result = UpdateLayeredWindow(
                self.hwnd,
                Some(screen_dc),
                Some(&destination),
                Some(&size),
                Some(memory_dc),
                Some(&source_point),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );

            SelectObject(memory_dc, previous);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(None, screen_dc);
            result.map_err(|error| anyhow!("UpdateLayeredWindow failed: {error}"))?;
        }
        Ok(())
    }
}
