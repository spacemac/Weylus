use crate::capturable::{Capturable, Recorder};
use captrs::Capturer;
use std::boxed::Box;
use std::error::Error;
use winapi::shared::windef::RECT;

use super::Geometry;

#[derive(Clone)]
pub struct CaptrsCapturable {
    id: u8,
    name: String,
    screen: RECT,
    virtual_screen: RECT,
}

impl CaptrsCapturable {
    pub fn new(id: u8, name: String, screen: RECT, virtual_screen: RECT) -> CaptrsCapturable {
        CaptrsCapturable {
            id,
            name,
            screen,
            virtual_screen,
        }
    }
}

impl Capturable for CaptrsCapturable {
    fn name(&self) -> String {
        format!("Desktop {} (captrs)", self.name).into()
    }
    fn before_input(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn recorder(&self, _capture_cursor: bool) -> Result<Box<dyn Recorder>, Box<dyn Error>> {
        // Desktop duplication gets no frames from virtual (IddCx) monitors on hybrid-GPU laptops;
        // GDI can read any screen, so use it for everything except the primary.
        if self.id == 0 {
            Ok(Box::new(CaptrsRecorder::new(self.id)?))
        } else {
            Ok(Box::new(GdiRecorder::new(self.screen)?))
        }
    }
    fn geometry(&self) -> Result<Geometry, Box<dyn Error>> {
        Ok(Geometry::VirtualScreen(
            self.screen.left - self.virtual_screen.left,
            self.screen.top - self.virtual_screen.top,
            (self.screen.right - self.screen.left) as u32,
            (self.screen.bottom - self.screen.top) as u32,
            self.screen.left,
            self.screen.top,
        ))
    }
}
#[derive(Debug)]
pub struct CaptrsError(String);

impl std::fmt::Display for CaptrsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self(s) = self;
        write!(f, "{}", s)
    }
}

impl Error for CaptrsError {}
pub struct CaptrsRecorder {
    capturer: Capturer,
}

impl CaptrsRecorder {
    pub fn new(id: u8) -> Result<CaptrsRecorder, Box<dyn Error>> {
        Ok(CaptrsRecorder {
            capturer: Capturer::new(id.into())?,
        })
    }
}

impl Recorder for CaptrsRecorder {
    fn capture(&mut self) -> Result<crate::video::PixelProvider, Box<dyn Error>> {
        self.capturer
            .capture_store_frame()
            .map_err(|_e| CaptrsError("Captrs failed to capture frame".into()))?;
        let (w, h) = self.capturer.geometry();
        Ok(crate::video::PixelProvider::BGR0(
            w as usize,
            h as usize,
            unsafe { std::mem::transmute(self.capturer.get_stored_frame().unwrap()) },
        ))
    }
}

pub struct GdiRecorder {
    screen: RECT,
    screen_dc: winapi::shared::windef::HDC,
    mem_dc: winapi::shared::windef::HDC,
    bitmap: winapi::shared::windef::HBITMAP,
    bits: *mut u8,
    w: usize,
    h: usize,
}

impl GdiRecorder {
    pub fn new(screen: RECT) -> Result<GdiRecorder, Box<dyn Error>> {
        use winapi::um::wingdi::*;
        use winapi::um::winuser::GetDC;
        let w = (screen.right - screen.left) as usize;
        let h = (screen.bottom - screen.top) as usize;
        unsafe {
            let screen_dc = GetDC(std::ptr::null_mut());
            if screen_dc.is_null() {
                return Err(Box::new(CaptrsError("GetDC failed".into())));
            }
            let mem_dc = CreateCompatibleDC(screen_dc);
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w as i32;
            bmi.bmiHeader.biHeight = -(h as i32); // top-down rows
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let mut bits: *mut winapi::ctypes::c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(
                screen_dc,
                &bmi,
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            );
            let rec = GdiRecorder {
                screen,
                screen_dc,
                mem_dc,
                bitmap,
                bits: bits as *mut u8,
                w,
                h,
            };
            if mem_dc.is_null() || bitmap.is_null() || bits.is_null() {
                return Err(Box::new(CaptrsError("Failed to create GDI bitmap".into())));
            }
            SelectObject(mem_dc, bitmap as _);
            Ok(rec)
        }
    }
}

impl Drop for GdiRecorder {
    fn drop(&mut self) {
        use winapi::um::wingdi::{DeleteDC, DeleteObject};
        use winapi::um::winuser::ReleaseDC;
        unsafe {
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap as _);
            }
            if !self.mem_dc.is_null() {
                DeleteDC(self.mem_dc);
            }
            ReleaseDC(std::ptr::null_mut(), self.screen_dc);
        }
    }
}

impl Recorder for GdiRecorder {
    fn capture(&mut self) -> Result<crate::video::PixelProvider, Box<dyn Error>> {
        use winapi::um::wingdi::{BitBlt, GdiFlush, SRCCOPY};
        use winapi::um::winuser::SetThreadDpiAwarenessContext;
        // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 (missing from winapi 0.3)
        let per_monitor_v2 = -4isize as winapi::shared::windef::DPI_AWARENESS_CONTEXT;
        unsafe {
            // DXGI rects are physical pixels; make GDI use them too, on this thread only.
            let prev = SetThreadDpiAwarenessContext(per_monitor_v2);
            let ok = BitBlt(
                self.mem_dc,
                0,
                0,
                self.w as i32,
                self.h as i32,
                self.screen_dc,
                self.screen.left,
                self.screen.top,
                SRCCOPY,
            );
            // GDI doesn't copy the mouse cursor; draw it in so the iPad shows where the pointer is.
            {
                use winapi::um::wingdi::DeleteObject;
                use winapi::um::winuser::{
                    DrawIconEx, GetCursorInfo, GetIconInfo, CURSORINFO, CURSOR_SHOWING, DI_NORMAL,
                    ICONINFO,
                };
                let mut ci: CURSORINFO = std::mem::zeroed();
                ci.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
                if ok != 0 && GetCursorInfo(&mut ci) != 0 && ci.flags & CURSOR_SHOWING != 0 {
                    let mut ii: ICONINFO = std::mem::zeroed();
                    if GetIconInfo(ci.hCursor, &mut ii) != 0 {
                        DrawIconEx(
                            self.mem_dc,
                            ci.ptScreenPos.x - ii.xHotspot as i32 - self.screen.left,
                            ci.ptScreenPos.y - ii.yHotspot as i32 - self.screen.top,
                            ci.hCursor,
                            0,
                            0,
                            0,
                            std::ptr::null_mut(),
                            DI_NORMAL,
                        );
                        if !ii.hbmMask.is_null() {
                            DeleteObject(ii.hbmMask as _);
                        }
                        if !ii.hbmColor.is_null() {
                            DeleteObject(ii.hbmColor as _);
                        }
                    }
                }
            }
            GdiFlush();
            if !prev.is_null() {
                SetThreadDpiAwarenessContext(prev);
            }
            if ok == 0 {
                return Err(Box::new(CaptrsError("GDI BitBlt failed".into())));
            }
            Ok(crate::video::PixelProvider::BGR0(
                self.w,
                self.h,
                std::slice::from_raw_parts(self.bits, self.w * self.h * 4),
            ))
        }
    }
}