//! Provider boundary for source authorization and read-only catalog listing.
use crate::{auth, scan_directory, AppError, AppState, MediaFile};
use axum::http::StatusCode;
use std::path::PathBuf;
use uuid::Uuid;

pub(crate) struct LocalSource {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
}

impl LocalSource {
    pub(crate) async fn authorized(
        state: &AppState,
        user: Uuid,
        source_id: &str,
    ) -> Result<Self, AppError> {
        let row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT s.id,s.name,s.path FROM sources s
             WHERE s.id=$1 AND s.kind='local_directory' AND (s.owner_id=$2 OR EXISTS(
                 SELECT 1 FROM source_permissions p WHERE p.source_id=s.id AND p.user_id=$2
                     AND p.can_discover AND p.can_access_media))",
        )
        .bind(source_id)
        .bind(user)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| auth::unavailable())?;
        let (id, name, path) = row.ok_or_else(|| {
            AppError(
                StatusCode::NOT_FOUND,
                "Source not found or media access not granted.".into(),
            )
        })?;
        Ok(Self {
            id,
            name,
            path: path.into(),
        })
    }

    pub(crate) fn list(&self) -> Result<Vec<MediaFile>, AppError> {
        if std::fs::read_dir(&self.path).is_err() {
            return Err(AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "This source is currently unavailable.".into(),
            ));
        }
        scan_directory(&self.path).map_err(|_| auth::unavailable())
    }
}
