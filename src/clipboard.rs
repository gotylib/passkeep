use zeroize::{Zeroize, Zeroizing};

/// Copies a secret and asks Windows not to put it in clipboard history or the cloud clipboard.
pub fn copy(text: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::copy(text)
    }
    #[cfg(not(windows))]
    {
        other::copy(text)
    }
}

/// Clears the clipboard when it still holds `secret`.
/// Returns false when the clipboard could not be opened.
pub fn clear_if_matches(secret: &str) -> bool {
    #[cfg(windows)]
    {
        windows::clear_if_matches(secret)
    }
    #[cfg(not(windows))]
    {
        other::clear_if_matches(secret)
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    use super::{Zeroize, Zeroizing};
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
        SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
    };

    const CF_UNICODETEXT: u32 = 13;

    const EXCLUDE_FORMATS: [&str; 3] = [
        "CanIncludeInClipboardHistory",
        "CanUploadToCloudClipboard",
        "ExcludeClipboardContentFromMonitorProcessing",
    ];

    struct ClipboardGuard;

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe { CloseClipboard() };
        }
    }

    pub fn copy(text: &str) -> Result<(), String> {
        let mut wide = Zeroizing::new(encode_wide(text));
        let _guard = open()?;
        unsafe {
            if EmptyClipboard() == 0 {
                return Err("couldn't clear the clipboard".to_owned());
            }
            for name in EXCLUDE_FORMATS {
                let format = register_format(name);
                if format != 0 {
                    let _ = set_bytes(format, &[0, 0, 0, 0]);
                }
            }
            set_bytes(
                CF_UNICODETEXT,
                std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2),
            )?;
        }
        wide.zeroize();
        Ok(())
    }

    pub fn clear_if_matches(secret: &str) -> bool {
        let Ok(_guard) = open() else {
            return false;
        };
        let expected = Zeroizing::new(secret.encode_utf16().collect::<Vec<u16>>());
        let matches = unsafe { clipboard_equals(&expected) };
        if matches {
            unsafe { EmptyClipboard() };
        }
        true
    }

    fn open() -> Result<ClipboardGuard, String> {
        for _ in 0..8 {
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return Ok(ClipboardGuard);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err("couldn't open the clipboard".to_owned())
    }

    fn encode_wide(text: &str) -> Vec<u16> {
        OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn register_format(name: &str) -> u32 {
        let wide = encode_wide(name);
        unsafe { RegisterClipboardFormatW(wide.as_ptr()) }
    }

    /// # Safety
    /// The clipboard must be open. On success the system owns `bytes`.
    unsafe fn set_bytes(format: u32, bytes: &[u8]) -> Result<(), String> {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if handle.is_null() {
            return Err("not enough memory for the clipboard".to_owned());
        }
        let pointer = GlobalLock(handle) as *mut u8;
        if pointer.is_null() {
            GlobalFree(handle);
            return Err("not enough memory for the clipboard".to_owned());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len());
        GlobalUnlock(handle);
        if SetClipboardData(format, handle).is_null() {
            GlobalFree(handle);
            return Err("couldn't copy".to_owned());
        }
        Ok(())
    }

    /// # Safety
    /// The clipboard must be open.
    unsafe fn clipboard_equals(expected: &[u16]) -> bool {
        let handle = GetClipboardData(CF_UNICODETEXT);
        if handle.is_null() {
            return false;
        }
        let bytes = GlobalSize(handle);
        let units = bytes / 2;
        if units < expected.len().saturating_add(1) {
            return false;
        }
        let pointer = GlobalLock(handle) as *const u16;
        if pointer.is_null() {
            return false;
        }
        let mut same = true;
        for (index, unit) in expected.iter().enumerate() {
            if *pointer.add(index) != *unit {
                same = false;
                break;
            }
        }
        if same && *pointer.add(expected.len()) != 0 {
            same = false;
        }
        GlobalUnlock(handle);
        same
    }
}

#[cfg(not(windows))]
mod other {
    use super::Zeroize;

    pub fn copy(text: &str) -> Result<(), String> {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|_| "couldn't open the clipboard".to_owned())?;
        clipboard
            .set_text(text.to_owned())
            .map_err(|_| "couldn't copy".to_owned())
    }

    pub fn clear_if_matches(secret: &str) -> bool {
        let Ok(mut clipboard) = arboard::Clipboard::new() else {
            return false;
        };
        let Ok(mut current) = clipboard.get_text() else {
            return true;
        };
        if current == secret {
            let _ = clipboard.clear();
        }
        current.zeroize();
        true
    }
}
