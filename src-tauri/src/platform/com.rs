//! Per-thread COM initialisation.

use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

struct ComGuard {
    initialised: bool,
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.initialised {
            unsafe { CoUninitialize() };
        }
    }
}

thread_local! {
    static COM: ComGuard = {
        // S_OK and S_FALSE both mean we own a reference; RPC_E_CHANGED_MODE
        // means the thread is already STA, which works for our calls too.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        ComGuard { initialised: hr.is_ok() }
    };
}

/// Ensure COM is initialised (multithreaded apartment) on the current thread.
/// Cheap after the first call on a given thread.
pub fn ensure_mta() {
    COM.with(|_| {});
}
