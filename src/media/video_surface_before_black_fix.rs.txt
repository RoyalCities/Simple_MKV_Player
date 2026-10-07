use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use std::{ffi::c_void, ptr};

use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        CREATESTRUCTW, CW_USEDEFAULT, CreateWindowExW, DestroyWindow, GetClientRect, MoveWindow,
        SW_HIDE, SW_SHOW, ShowWindow, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_VISIBLE,
    },
};

pub struct VideoSurface {
    hwnd: HWND,
}

impl VideoSurface {
    pub fn new(parent: HWND) -> Result<Self, String> {
        if parent.is_null() {
            return Err("Parent HWND is NULL.".into());
        }

        let class_name: Vec<u16> = "STATIC\0".encode_utf16().collect();

        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                ptr::null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                0,
                0,
                1,
                1,
                parent,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };

        if hwnd.is_null() {
            return Err("CreateWindowExW() failed for video surface.".into());
        }

        unsafe {
            ShowWindow(hwnd, SW_HIDE);
        }

        Ok(Self { hwnd })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn set_rect(&self, x: i32, y: i32, width: i32, height: i32) {
        if self.hwnd.is_null() {
            return;
        }

        unsafe {
            MoveWindow(self.hwnd, x, y, width.max(1), height.max(1), 1);
        }
    }

    pub fn show(&self) {
        if !self.hwnd.is_null() {
            unsafe {
                ShowWindow(self.hwnd, SW_SHOW);
            }
        }
    }

    pub fn hide(&self) {
        if !self.hwnd.is_null() {
            unsafe {
                ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }
}

impl Drop for VideoSurface {
    fn drop(&mut self) {
        if !self.hwnd.is_null() {
            unsafe {
                DestroyWindow(self.hwnd);
            }

            self.hwnd = ptr::null_mut();
        }
    }
}

// ------------------------------------------------------------
// Extract Win32 HWND from anything implementing
// raw-window-handle 0.6's HasWindowHandle.
// ------------------------------------------------------------

pub fn hwnd_from_handle<T>(value: &T) -> Result<HWND, String>
where
    T: HasWindowHandle,
{
    let handle = value
        .window_handle()
        .map_err(|e| format!("Could not obtain window handle: {e}"))?;

    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Ok(win32.hwnd.get() as *mut c_void),

        _ => Err("Simple MKV Player native video embedding currently requires Windows.".into()),
    }
}
