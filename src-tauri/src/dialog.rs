use anyhow::{bail, Result};
use std::path::PathBuf;

/// OS-owned file selection. No shell, current-directory mutation or frontend path access.
#[cfg(windows)]
pub fn choose(save: bool, import: bool) -> Result<Option<PathBuf>> {
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    let mut filename = vec![0u16; 32768];
    if save {
        let initial: Vec<u16> = "Untitled.linkscope".encode_utf16().collect();
        filename[..initial.len()].copy_from_slice(&initial);
    }
    let filter: Vec<u16> = if import {
        "Datasets (*.json;*.csv;*.txt)\0*.json;*.csv;*.txt\0All files\0*.*\0\0"
    } else {
        "LinkScope project (*.linkscope;*.json)\0*.linkscope;*.json\0All files\0*.*\0\0"
    }
    .encode_utf16()
    .collect();
    let extension: Vec<u16> = "linkscope\0".encode_utf16().collect();
    // OPENFILENAMEW is a C struct whose zero initialization is documented by Win32.
    let mut options: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    options.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    options.lpstrFile = filename.as_mut_ptr();
    options.nMaxFile = filename.len() as u32;
    options.lpstrFilter = filter.as_ptr();
    options.nFilterIndex = 1;
    options.lpstrDefExt = extension.as_ptr();
    options.Flags = OFN_EXPLORER
        | OFN_NOCHANGEDIR
        | OFN_PATHMUSTEXIST
        | if save {
            OFN_OVERWRITEPROMPT
        } else {
            OFN_FILEMUSTEXIST
        };
    let success = unsafe {
        if save {
            GetSaveFileNameW(&mut options)
        } else {
            GetOpenFileNameW(&mut options)
        }
    };
    if success == 0 {
        let code = unsafe { CommDlgExtendedError() };
        if code != 0 {
            bail!("File dialog failed (Windows error {code})");
        }
        return Ok(None);
    }
    let len = filename
        .iter()
        .position(|&x| x == 0)
        .unwrap_or(filename.len());
    use std::os::windows::ffi::OsStringExt;
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(
        &filename[..len],
    ))))
}
#[cfg(not(windows))]
pub fn choose(_save: bool, _import: bool) -> Result<Option<PathBuf>> {
    bail!("Native file dialogs currently support Windows. This build has not been ported to this platform.")
}
