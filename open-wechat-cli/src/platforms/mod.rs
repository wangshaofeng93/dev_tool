#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(target_os = "windows")]
pub mod win;

#[cfg(target_os = "linux")]
pub fn is_supported() -> bool {
    linux::SUPPORTED
}

#[cfg(target_os = "macos")]
pub fn is_supported() -> bool {
    mac::SUPPORTED
}

#[cfg(target_os = "windows")]
pub fn is_supported() -> bool {
    win::SUPPORTED
}
