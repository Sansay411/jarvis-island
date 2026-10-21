//! API keys in Windows Credential Manager (generic credentials, persisted
//! per user on this machine). Secrets never touch SQLite, logs or IPC
//! payloads; the UI only learns whether a key is set.

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_NOT_FOUND, FILETIME};
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
};

use crate::error::{AppError, AppResult};

pub const LLM_API_KEY: &str = "JARVIS/llm-api-key";
pub const VOICE_API_KEY: &str = "JARVIS/voice-api-key";

pub fn set(target: &str, secret: &str) -> AppResult<()> {
    let target_w = HSTRING::from(target);
    let user_w = HSTRING::from("jarvis");
    let mut blob = secret.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Flags: CRED_FLAGS(0),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target_w.as_ptr() as *mut _),
        Comment: PWSTR::null(),
        LastWritten: FILETIME::default(),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: PWSTR::null(),
        UserName: PWSTR(user_w.as_ptr() as *mut _),
    };
    let res = unsafe { CredWriteW(&cred, 0) };
    // Best effort: don't leave the plaintext copy lying around in our heap.
    blob.iter_mut().for_each(|b| *b = 0);
    res.map_err(|e| AppError::Storage(format!("Не удалось сохранить ключ в диспетчере учётных данных: {}", e.message())))
}

pub fn get(target: &str) -> AppResult<Option<String>> {
    let target_w = HSTRING::from(target);
    let mut out: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        if let Err(e) = CredReadW(PCWSTR(target_w.as_ptr()), CRED_TYPE_GENERIC, None, &mut out) {
            if e.code() == ERROR_NOT_FOUND.to_hresult() {
                return Ok(None);
            }
            return Err(AppError::Storage(format!("Не удалось прочитать ключ: {}", e.message())));
        }
        let cred = &*out;
        let bytes = std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize);
        let value = String::from_utf8_lossy(bytes).into_owned();
        CredFree(out as *const _);
        Ok(Some(value))
    }
}

pub fn exists(target: &str) -> bool {
    matches!(get(target), Ok(Some(ref s)) if !s.is_empty())
}

pub fn delete(target: &str) -> AppResult<()> {
    let target_w = HSTRING::from(target);
    match unsafe { CredDeleteW(PCWSTR(target_w.as_ptr()), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
        Err(e) => Err(AppError::Storage(format!("Не удалось удалить ключ: {}", e.message()))),
    }
}
