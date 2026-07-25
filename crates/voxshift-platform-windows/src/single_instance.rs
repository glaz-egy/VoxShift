//! Single-instance guard — 設計書.md §20 (`Local\VoxShift.SingleInstance`
//! named mutex). The Named Pipe "show existing window" control channel is
//! not implemented in this pass — a second launch simply exits after
//! detecting the mutex is already held (flagged as a follow-up).

#[cfg(windows)]
mod win {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
    use windows::Win32::System::Threading::CreateMutexW;

    const MUTEX_NAME: &str = "Local\\VoxShift.SingleInstance";

    pub struct SingleInstanceGuard {
        handle: HANDLE,
    }

    impl Drop for SingleInstanceGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }

    /// Returns `Some(guard)` if this is the only running instance (the
    /// guard must be kept alive for the lifetime of the process); `None`
    /// if another instance already holds the mutex.
    pub fn acquire() -> Option<SingleInstanceGuard> {
        let wide: Vec<u16> = MUTEX_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe { CreateMutexW(None, true, PCWSTR(wide.as_ptr())) }.ok()?;
        if unsafe { windows::Win32::Foundation::GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return None;
        }
        Some(SingleInstanceGuard { handle })
    }
}

#[cfg(windows)]
pub use win::{acquire, SingleInstanceGuard};

#[cfg(not(windows))]
pub struct SingleInstanceGuard;

#[cfg(not(windows))]
pub fn acquire() -> Option<SingleInstanceGuard> {
    Some(SingleInstanceGuard)
}
