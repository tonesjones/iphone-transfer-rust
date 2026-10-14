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
    // SAFETY: COM is initialized on this thread, every call uses the live dialog or the item it
    // returned, and the COM-allocated name is freed once after it is copied into a String.
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)?;
        if let Err(error) = dialog.Show(None) {
            if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                return Ok(None);
            }
            return Err(error.into());
        }
        let name = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = name.to_string();
        CoTaskMemFree(Some(name.0.cast()));
        Ok(Some(PathBuf::from(path?)))
    }
}
