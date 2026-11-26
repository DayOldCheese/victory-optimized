//! Windows' own Open and Save dialogs (no extra dependency).

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::UI::Controls::Dialogs::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    /// `filter`: pairs of (label, pattern), e.g. ("Programs", "*.exe").
    fn run(save: bool, title: &str, filter: &[(&str, &str)], default_name: &str) -> Option<String> {
        let mut filt: Vec<u16> = Vec::new();
        for (label, pat) in filter {
            filt.extend(wide(label));
            filt.push(0);
            filt.extend(wide(pat));
            filt.push(0);
        }
        filt.push(0);
        let title_w: Vec<u16> = wide(title).into_iter().chain([0]).collect();
        let mut buf = vec![0u16; 1024];
        for (i, c) in wide(default_name).into_iter().take(1000).enumerate() {
            buf[i] = c;
        }
        // SAFETY: plain data; the pointers below outlive the call.
        let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
        ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        ofn.lpstrFilter = filt.as_ptr();
        ofn.lpstrFile = buf.as_mut_ptr();
        ofn.nMaxFile = buf.len() as u32;
        ofn.lpstrTitle = title_w.as_ptr();
        ofn.Flags = if save { OFN_OVERWRITEPROMPT | OFN_NOCHANGEDIR } else { OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR };
        // SAFETY: an initialised description whose buffers are live.
        let ok = unsafe { if save { GetSaveFileNameW(&mut ofn) } else { GetOpenFileNameW(&mut ofn) } };
        if ok == 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }

    pub fn open(title: &str, filter: &[(&str, &str)]) -> Option<String> {
        run(false, title, filter, "")
    }

    pub fn save(title: &str, filter: &[(&str, &str)], default_name: &str) -> Option<String> {
        run(true, title, filter, default_name)
    }
}

#[cfg(windows)]
pub use imp::{open, save};

#[cfg(not(windows))]
pub fn open(_: &str, _: &[(&str, &str)]) -> Option<String> {
    None
}
#[cfg(not(windows))]
pub fn save(_: &str, _: &[(&str, &str)], _: &str) -> Option<String> {
    None
}
