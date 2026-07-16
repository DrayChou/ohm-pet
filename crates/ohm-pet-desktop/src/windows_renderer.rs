use anyhow::{anyhow, Context, Result};
use ohm_pet_core::{Atlas, CELL_HEIGHT, CELL_WIDTH};
use std::{ffi::c_void, mem::size_of, ptr::copy_nonoverlapping};
use windows::Win32::{
    Foundation::{COLORREF, HWND, POINT, RECT, SIZE},
    Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        BLENDFUNCTION, DIB_RGB_COLORS, HGDIOBJ,
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
        let center_x = (f64::from(rect.left) + f64::from(rect.right)) / 2.0;
        let center_y = (f64::from(rect.top) + f64::from(rect.bottom)) / 2.0;
        (
            f64::from(cursor.x) - center_x,
            f64::from(cursor.y) - center_y,
        )
    }

    pub fn render(&mut self, atlas: &Atlas, row: u32, column: u32) -> Result<()> {
        let source =
            image::RgbaImage::from_raw(CELL_WIDTH, CELL_HEIGHT, atlas.frame_rgba(row, column))
                .ok_or_else(|| anyhow!("invalid atlas frame"))?;
        let scaled = image::imageops::resize(
            &source,
            self.width,
            self.height,
            image::imageops::FilterType::Nearest,
        );
        let mut bgra = Vec::with_capacity((self.width * self.height * 4) as usize);
        for pixel in scaled.pixels() {
            let alpha = u16::from(pixel[3]);
            bgra.push(((u16::from(pixel[2]) * alpha) / 255) as u8);
            bgra.push(((u16::from(pixel[1]) * alpha) / 255) as u8);
            bgra.push(((u16::from(pixel[0]) * alpha) / 255) as u8);
            bgra.push(pixel[3]);
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
