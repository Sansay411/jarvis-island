//! Speech models for the local voice engine, downloaded on demand from the
//! sherpa-onnx releases on GitHub (k2-fsa/sherpa-onnx, tags `asr-models` and
//! `tts-models`). Each file is checked against the size and SHA-256 that the
//! GitHub release lists for it; archives are unpacked into the models folder.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Stt,
    Vad,
    Tts,
}

pub struct Asset {
    pub id: &'static str,
    pub kind: Kind,
    pub label: &'static str,
    /// Release file name.
    pub file: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub release: &'static str,
}

impl Asset {
    /// Archives unpack into a folder named after the archive.
    pub fn is_archive(&self) -> bool {
        self.file.ends_with(".tar.bz2")
    }

    /// The model file, or the unpacked folder for archives.
    pub fn path(&self) -> PathBuf {
        models_dir().join(self.file.trim_end_matches(".tar.bz2"))
    }

    pub fn installed(&self) -> bool {
        let p = self.path();
        if self.is_archive() {
            p.join(".complete").is_file()
        } else {
            std::fs::metadata(&p).map(|m| m.len() == self.size).unwrap_or(false)
        }
    }
}

const RELEASES: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/";

/// Sizes and digests from the GitHub release API
/// (`repos/k2-fsa/sherpa-onnx/releases/tags/{asr,tts}-models`).
pub const ASSETS: &[Asset] = &[
    Asset {
        id: "gigaam-v3",
        kind: Kind::Stt,
        label: "GigaAM v3 — русская речь, с пунктуацией",
        file: "sherpa-onnx-nemo-transducer-punct-giga-am-v3-russian-2025-12-16.tar.bz2",
        size: 170_197_019,
        sha256: "f9620a0099019c6afcee26525ef9ed3297fa50dd5691c1902af0c948fc1a470b",
        release: "asr-models",
    },
    Asset {
        id: "silero-vad",
        kind: Kind::Vad,
        label: "Silero VAD",
        file: "silero_vad.onnx",
        size: 643_854,
        sha256: "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6",
        release: "asr-models",
    },
    Asset {
        id: "supertonic-3",
        kind: Kind::Tts,
        label: "Supertonic 3 — нейросетевые голоса",
        file: "sherpa-onnx-supertonic-3-tts-int8-2026-05-11.tar.bz2",
        size: 128_774_318,
        sha256: "82fa96f91c4ef8abaae3a14a3f4153facf88bed821d1f7331cec2700f432c427",
        release: "tts-models",
    },
    Asset {
        id: "piper-ruslan",
        kind: Kind::Tts,
        label: "Piper Руслан",
        file: "vits-piper-ru_RU-ruslan-medium.tar.bz2",
        size: 67_210_684,
        sha256: "0690b1cad01f86e8db9ba988af24898bdc1af774e23cb2e46b9c730269b6fd83",
        release: "tts-models",
    },
    Asset {
        id: "piper-dmitri",
        kind: Kind::Tts,
        label: "Piper Дмитрий",
        file: "vits-piper-ru_RU-dmitri-medium.tar.bz2",
        size: 67_188_551,
        sha256: "c86d0803737de13d441923ff3b3f309482fab8d7af3ec85949942809eb9a3660",
        release: "tts-models",
    },
    Asset {
        id: "piper-irina",
        kind: Kind::Tts,
        label: "Piper Ирина",
        file: "vits-piper-ru_RU-irina-medium.tar.bz2",
        size: 67_153_308,
        sha256: "1fc0f54e5e084fe287c07909f2f6e0ba6d857864cf800e3ab80286a4e8233008",
        release: "tts-models",
    },
];

pub fn asset(id: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|a| a.id == id)
}

pub fn models_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("dev.jarvis.desktop").join("models")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetInfo {
    pub id: &'static str,
    pub kind: Kind,
    pub label: &'static str,
    pub size: u64,
    pub installed: bool,
}

pub fn list() -> Vec<AssetInfo> {
    ASSETS
        .iter()
        .map(|a| AssetInfo { id: a.id, kind: a.kind, label: a.label, size: a.size, installed: a.installed() })
        .collect()
}

/// Download (and unpack) `a`, reporting `(downloaded, total)` bytes.
pub async fn download(a: &'static Asset, cancel: CancellationToken, progress: impl Fn(u64, u64)) -> AppResult<PathBuf> {
    if a.installed() {
        return Ok(a.path());
    }
    let dir = models_dir();
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Storage(format!("Папка моделей: {e}")))?;
    let part = dir.join(format!("{}.part", a.file));
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::internal(e.to_string()))?;
    let resp = client
        .get(format!("{RELEASES}{}/{}", a.release, a.file))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| AppError::Network(format!("Не удалось скачать модель: {}", e.without_url())))?;
    let mut file = std::fs::File::create(&part).map_err(|e| AppError::Storage(format!("Не удалось записать модель: {e}")))?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    let mut stream = resp.bytes_stream();
    let mut last_report = Instant::now();
    let result: AppResult<()> = loop {
        let next = tokio::select! {
            _ = cancel.cancelled() => break Err(AppError::Cancelled),
            n = tokio::time::timeout(std::time::Duration::from_secs(60), stream.next()) => n,
        };
        match next {
            Err(_) => break Err(AppError::Timeout("Загрузка модели остановилась: нет данных 60 с".into())),
            Ok(None) => break Ok(()),
            Ok(Some(Err(e))) => break Err(AppError::Network(format!("Загрузка прервалась: {}", e.without_url()))),
            Ok(Some(Ok(chunk))) => {
                hasher.update(&chunk);
                if let Err(e) = file.write_all(&chunk) {
                    break Err(AppError::Storage(format!("Не удалось записать модель: {e}")));
                }
                done += chunk.len() as u64;
                if last_report.elapsed().as_millis() >= 200 {
                    last_report = Instant::now();
                    progress(done, a.size);
                }
            }
        }
    };
    drop(file);
    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    progress(done, a.size);
    let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if done != a.size || digest != a.sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(AppError::Network(format!(
            "Файл модели повреждён (получено {done} байт, контрольная сумма не совпала) — попробуйте ещё раз"
        )));
    }
    if a.is_archive() {
        let (src, dest) = (part.clone(), dir.clone());
        let res = tokio::task::spawn_blocking(move || unpack(&src, &dest))
            .await
            .map_err(|e| AppError::internal(e.to_string()))?;
        let _ = std::fs::remove_file(&part);
        res?;
        std::fs::write(a.path().join(".complete"), a.sha256).map_err(|e| AppError::Storage(e.to_string()))?;
    } else {
        std::fs::rename(&part, a.path()).map_err(|e| AppError::Storage(format!("Не удалось сохранить модель: {e}")))?;
    }
    Ok(a.path())
}

/// Unpack a verified .tar.bz2 into `dest`, refusing entries that would
/// escape it.
fn unpack(archive: &Path, dest: &Path) -> AppResult<()> {
    let f = std::fs::File::open(archive).map_err(|e| AppError::Storage(e.to_string()))?;
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(std::io::BufReader::new(f)));
    for entry in tar.entries().map_err(|e| AppError::Storage(format!("Архив модели: {e}")))? {
        let mut entry = entry.map_err(|e| AppError::Storage(format!("Архив модели: {e}")))?;
        // `unpack_in` rejects absolute paths and `..` components.
        entry.unpack_in(dest).map_err(|e| AppError::Storage(format!("Не удалось распаковать модель: {e}")))?;
    }
    Ok(())
}

/// First file in `dir` whose name matches, preferring the int8 variant.
pub fn find_file(dir: &Path, prefix: &str, ext: &str) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(prefix) && n.ends_with(ext)))
        .collect();
    found.sort_by_key(|p| !p.to_string_lossy().contains(".int8."));
    found.into_iter().next()
}

/// A path string sherpa-onnx can open: it takes narrow (ANSI code page)
/// paths on Windows, so profile folders with Cyrillic names go through the
/// 8.3 short form.
pub fn native_path(path: &Path) -> AppResult<String> {
    let s = path.to_str().ok_or_else(|| AppError::Storage("Некорректный путь к модели".into()))?;
    if s.is_ascii() {
        return Ok(s.to_string());
    }
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetShortPathNameW;
    let wide = HSTRING::from(path.as_os_str());
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetShortPathNameW(&wide, Some(&mut buf)) } as usize;
    match String::from_utf16(buf.get(..n).unwrap_or_default()) {
        Ok(short) if n > 0 && short.is_ascii() => Ok(short),
        _ => Err(AppError::Storage(format!(
            "Путь к моделям содержит символы, которые не поддерживает движок речи: {}",
            path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_consistent() {
        for a in ASSETS {
            assert_eq!(a.sha256.len(), 64, "{}", a.id);
            assert!(a.size > 0);
        }
        assert!(asset("gigaam-v3").unwrap().is_archive());
        assert!(!asset("silero-vad").unwrap().is_archive());
        assert!(asset("silero-vad").unwrap().path().ends_with("silero_vad.onnx"));
    }

    #[test]
    fn find_prefers_int8() {
        let dir = std::env::temp_dir().join(format!("jarvis-find-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["encoder.onnx", "encoder.int8.onnx", "decoder.onnx"] {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        assert!(find_file(&dir, "encoder", ".onnx").unwrap().ends_with("encoder.int8.onnx"));
        assert!(find_file(&dir, "decoder", ".onnx").unwrap().ends_with("decoder.onnx"));
        assert!(find_file(&dir, "joiner", ".onnx").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
