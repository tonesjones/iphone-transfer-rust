use std::path::PathBuf;

use windows::Win32::{
    Foundation::{ERROR_CANCELLED, RPC_E_CHANGED_MODE, S_FALSE, S_OK},
    System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize,
    },
    UI::Shell::{
        FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
    },
};
use windows::core::HRESULT;

struct Com(bool);

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances this thread's successful CoInitializeEx call.
            unsafe { CoUninitialize() };
        }
    }
}

pub(crate) fn folder() -> anyhow::Result<Option<PathBuf>> {
    // SAFETY: initializes COM on the calling UI thread with no reserved parameter.
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let _com = Com(initialized == S_OK || initialized == S_FALSE);
    if initialized != RPC_E_CHANGED_MODE {
        initialized.ok()?;
    }
    // SAFETY: the standard FileOpenDialog class implements IFileOpenDialog; COM is initialized.
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)? };
    // SAFETY: valid dialog interface and documented folder/filesystem option flags.
    unsafe { dialog.SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)? };
    // SAFETY: the dialog is live; an ownerless modal dialog is supported.
    if let Err(error) = unsafe { dialog.Show(None) } {
        if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
            return Ok(None);
        }
        return Err(error.into());
    }
    // SAFETY: Show succeeded, so the dialog has a selected shell item.
    let item = unsafe { dialog.GetResult()? };
    // SAFETY: the filesystem-only picker provides a filesystem display name.
    let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH)? };
    // SAFETY: GetDisplayName returns a valid null-terminated UTF-16 allocation.
    let path = unsafe { name.to_string() };
    // SAFETY: the name was allocated by COM and is no longer used after conversion.
    unsafe { CoTaskMemFree(Some(name.0.cast())) };
    Ok(Some(PathBuf::from(path?)))
}
