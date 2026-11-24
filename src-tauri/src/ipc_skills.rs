//! IPC commands for Settings -> «Навыки».

use std::sync::Arc;

use tauri::State;
use tokio_util::sync::CancellationToken;

use crate::error::AppResult;
use crate::skills::{Skill, SkillInstallPreview, SkillSaveInput, SkillStore, SkillSummary};

type SkillsState<'a> = State<'a, Arc<SkillStore>>;

#[tauri::command]
pub fn skills_list(skills: SkillsState<'_>) -> AppResult<Vec<SkillSummary>> {
    skills.list()
}

#[tauri::command]
pub fn skills_read(skills: SkillsState<'_>, slug: String) -> AppResult<Skill> {
    skills.read(&slug, false)
}

#[tauri::command]
pub fn skills_save(skills: SkillsState<'_>, input: SkillSaveInput) -> AppResult<Skill> {
    skills.save(input)
}

#[tauri::command]
pub fn skills_set_enabled(skills: SkillsState<'_>, slug: String, enabled: bool) -> AppResult<SkillSummary> {
    skills.set_enabled(&slug, enabled)
}

#[tauri::command]
pub fn skills_delete(skills: SkillsState<'_>, slug: String) -> AppResult<()> {
    skills.delete(&slug)?;
    Ok(())
}

#[tauri::command]
pub async fn skills_install_preview(skills: SkillsState<'_>, url: String) -> AppResult<SkillInstallPreview> {
    let cancel = CancellationToken::new();
    skills.inner().clone().preview_install(&url, &cancel).await
}

#[tauri::command]
pub async fn skills_install(skills: SkillsState<'_>, url: String) -> AppResult<SkillSummary> {
    let cancel = CancellationToken::new();
    let saved = skills.inner().clone().commit_install(&url, &cancel).await?;
    Ok(SkillSummary::from(&saved))
}
