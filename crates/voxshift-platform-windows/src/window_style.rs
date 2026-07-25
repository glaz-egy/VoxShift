//! Native window style tweaks not exposed by Slint's `.slint` markup (e.g.
//! removing a specific title-bar button) — requires the raw HWND.

use std::ffi::c_void;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, GWL_STYLE, WS_MAXIMIZEBOX};

/// Removes the maximize button from the title bar of the window identified
/// by `hwnd` (a raw Win32 `HWND` value, e.g. from
/// `raw_window_handle::RawWindowHandle::Win32(handle).hwnd`). The window
/// stays manually resizable — only the maximize affordance is removed.
pub fn disable_maximize_button(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut c_void);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(WS_MAXIMIZEBOX.0 as isize));
    }
}
