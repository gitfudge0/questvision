//! Screen Recording access for the running application. Queries never prompt.

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// `None` means that this platform does not expose this macOS permission.
pub fn screen_recording_access() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: CoreGraphics accepts no arguments and returns the current
        // process's permission status without changing it or displaying UI.
        Some(unsafe { CGPreflightScreenCaptureAccess() })
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Called only from an explicit user action in the dashboard.
pub fn request_screen_recording_access() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: CoreGraphics accepts no arguments. The dashboard calls this
        // on the application's main thread in response to a user click.
        unsafe { CGRequestScreenCaptureAccess() };
    }
    screen_recording_access()
}
