//! Explicit per-user source grants. Authentication lives in auth; ownership is
//! required to edit grants. Possession of a source ID or an admin role is not enough.
use crate::{auth, AppError, AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionPolicy {
    #[default]
    AutoDeny,
    ManualApproval,
    AutoAccept,
}
impl AcquisitionPolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::AutoDeny => "auto_deny",
            Self::ManualApproval => "manual_approval",
            Self::AutoAccept => "auto_accept",
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Grant {
    pub can_discover: bool,
    pub can_access_media: bool,
    pub acquisition_policy: AcquisitionPolicy,
}
impl Grant {
    fn validate(&self) -> Result<(), AppError> {
        if !self.can_discover
            && (self.can_access_media || self.acquisition_policy != AcquisitionPolicy::AutoDeny)
        {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                "Source visibility is required before granting media or acquisition permission."
                    .into(),
            ));
        }
        Ok(())
    }
}
#[derive(Serialize, FromRow)]
pub struct Permission {
    pub user_id: Uuid,
    pub username: String,
    pub can_discover: bool,
    pub can_access_media: bool,
    pub acquisition_policy: String,
}

fn not_found() -> AppError {
    AppError(StatusCode::NOT_FOUND, "Source not found.".into())
}
async fn lock_owner(
    tx: &mut Transaction<'_, Postgres>,
    source: &str,
    actor: Uuid,
) -> Result<(), AppError> {
    let owner: Option<Uuid> =
        sqlx::query_scalar("SELECT owner_id FROM sources WHERE id=$1 FOR UPDATE")
            .bind(source)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| auth::unavailable())?;
    if owner != Some(actor) {
        return Err(not_found());
    }
    Ok(())
}

pub async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
) -> Result<Json<Vec<Permission>>, AppError> {
    let owned: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sources WHERE id=$1 AND owner_id=$2)")
            .bind(&source)
            .bind(identity.user.id)
            .fetch_one(&state.db)
            .await
            .map_err(|_| auth::unavailable())?;
    if !owned {
        return Err(not_found());
    }
    let grants = sqlx::query_as::<_, Permission>("SELECT p.user_id,u.username,p.can_discover,p.can_access_media,p.acquisition_policy FROM source_permissions p JOIN users u ON u.id=p.user_id WHERE p.source_id=$1 ORDER BY u.username")
        .bind(source).fetch_all(&state.db).await.map_err(|_| auth::unavailable())?;
    Ok(Json(grants))
}

pub async fn save(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, username)): Path<(String, String)>,
    Json(grant): Json<Grant>,
) -> Result<StatusCode, AppError> {
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    lock_owner(&mut tx, &source, identity.user.id).await?;
    grant.validate()?;
    let username = auth::username(&username)?;
    let target: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE username=$1")
        .bind(username)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    let target = target.ok_or_else(|| {
        AppError(
            StatusCode::BAD_REQUEST,
            "No account has that username.".into(),
        )
    })?;
    // Owners retain visibility, media access, and management. An explicit owner
    // row may configure their own acquisition policy, which otherwise defaults to deny.
    if target == identity.user.id && (!grant.can_discover || !grant.can_access_media) {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Owners retain source visibility and media access.".into(),
        ));
    }
    let before: Option<String> = sqlx::query_scalar("SELECT json_build_object('can_discover',can_discover,'can_access_media',can_access_media,'acquisition_policy',acquisition_policy)::text FROM source_permissions WHERE source_id=$1 AND user_id=$2")
        .bind(&source).bind(target).fetch_optional(&mut *tx).await.map_err(|_| auth::unavailable())?;
    sqlx::query("INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media,acquisition_policy) VALUES ($1,$2,$3,$4,$5) ON CONFLICT (source_id,user_id) DO UPDATE SET can_discover=EXCLUDED.can_discover,can_access_media=EXCLUDED.can_access_media,acquisition_policy=EXCLUDED.acquisition_policy,updated_at=now()")
        .bind(&source).bind(target).bind(grant.can_discover).bind(grant.can_access_media).bind(grant.acquisition_policy.as_str())
        .execute(&mut *tx).await.map_err(|_| auth::unavailable())?;
    let after = serde_json::to_string(&grant).map_err(|_| auth::unavailable())?;
    sqlx::query("INSERT INTO source_permission_audit(source_id,actor_id,target_id,action,before_state,after_state) VALUES ($1,$2,$3,'updated',$4,$5)")
        .bind(source).bind(identity.user.id).bind(target).bind(before).bind(after).execute(&mut *tx).await.map_err(|_| auth::unavailable())?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn revoke(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, username)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    lock_owner(&mut tx, &source, identity.user.id).await?;
    let username = auth::username(&username)?;
    let target: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE username=$1")
        .bind(username)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    if let Some(target) = target {
        let before: Option<String> = sqlx::query_scalar("DELETE FROM source_permissions WHERE source_id=$1 AND user_id=$2 RETURNING json_build_object('can_discover',can_discover,'can_access_media',can_access_media,'acquisition_policy',acquisition_policy)::text")
            .bind(&source).bind(target).fetch_optional(&mut *tx).await.map_err(|_| auth::unavailable())?;
        if before.is_some() {
            sqlx::query("INSERT INTO source_permission_audit(source_id,actor_id,target_id,action,before_state) VALUES ($1,$2,$3,'revoked',$4)")
                .bind(source).bind(identity.user.id).bind(target).bind(before).execute(&mut *tx).await.map_err(|_| auth::unavailable())?;
        }
    }
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grants_default_to_deny_and_require_visibility() {
        let grant: Grant = serde_json::from_str("{}").unwrap();
        assert!(!grant.can_discover && !grant.can_access_media);
        assert_eq!(grant.acquisition_policy, AcquisitionPolicy::AutoDeny);
        assert!(grant.validate().is_ok());
        assert!(Grant {
            can_access_media: true,
            ..Grant::default()
        }
        .validate()
        .is_err());
        assert!(Grant {
            acquisition_policy: AcquisitionPolicy::AutoAccept,
            ..Grant::default()
        }
        .validate()
        .is_err());
        assert!(serde_json::from_str::<Grant>(r#"{"acquisition_policy":"unknown"}"#).is_err());
    }
}
