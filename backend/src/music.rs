//! Private playlists/favorites store opaque references, never media access grants.
use crate::{auth, AppError, AppState};
use axum::{extract::State, http::StatusCode, Extension, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    pub favorites: Vec<String>,
    pub playlists: Vec<Playlist>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Playlist {
    pub id: Uuid,
    pub name: String,
    pub tracks: Vec<String>,
}
#[derive(Serialize)]
pub struct Snapshot {
    revision: i64,
    library: Library,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Save {
    expected_revision: i64,
    library: Library,
}
fn validate(library: &Library) -> Result<(), AppError> {
    let mut ids = HashSet::new();
    if library.favorites.len() > 2000
        || library.playlists.len() > 100
        || library
            .playlists
            .iter()
            .map(|p| p.tracks.len())
            .sum::<usize>()
            > 5000
        || library
            .favorites
            .iter()
            .any(|key| key.is_empty() || key.len() > 4096)
        || library.favorites.iter().collect::<HashSet<_>>().len() != library.favorites.len()
        || library.playlists.iter().any(|p| {
            !ids.insert(p.id)
                || p.name.trim().is_empty()
                || p.name.chars().count() > 100
                || p.tracks.len() > 1000
                || p.tracks
                    .iter()
                    .any(|key| key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()))
        })
    {
        return Err(crate::bad_request(
            "Invalid music library or storage limits exceeded.",
        ));
    }
    Ok(())
}
pub async fn read(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
) -> Result<Json<Snapshot>, AppError> {
    let row: Option<(i64, serde_json::Value)> =
        sqlx::query_as("SELECT revision,data FROM user_music WHERE user_id=$1")
            .bind(identity.user.id)
            .fetch_optional(&state.db)
            .await
            .map_err(|_| auth::unavailable())?;
    let (revision, library) = if let Some((revision, data)) = row {
        (
            revision,
            serde_json::from_value(data).map_err(|_| auth::unavailable())?,
        )
    } else {
        (0, Library::default())
    };
    Ok(Json(Snapshot { revision, library }))
}
pub async fn save(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Json(mut body): Json<Save>,
) -> Result<Json<Snapshot>, AppError> {
    validate(&body.library)?;
    if body.expected_revision < 0 {
        return Err(crate::bad_request("Invalid music library revision."));
    }
    for playlist in &mut body.library.playlists {
        playlist.name = playlist.name.trim().to_owned();
    }
    auth::rate_limit(
        &state.db,
        format!("music-library:{}", identity.user.id),
        120,
        300,
    )
    .await?;
    let data = serde_json::to_value(&body.library).map_err(|_| auth::unavailable())?;
    let revision:Option<i64>=sqlx::query_scalar("INSERT INTO user_music(user_id,revision,data) SELECT $1,1,$3 WHERE $2=0 ON CONFLICT(user_id) DO UPDATE SET data=$3,revision=user_music.revision+1,updated_at=now() WHERE user_music.revision=$2 RETURNING revision")
        .bind(identity.user.id).bind(body.expected_revision).bind(data).fetch_optional(&state.db).await.map_err(|_|auth::unavailable())?;
    let revision = revision.ok_or_else(|| {
        AppError(
            StatusCode::CONFLICT,
            "Music library changed in another window. Reload before editing again.".into(),
        )
    })?;
    Ok(Json(Snapshot {
        revision,
        library: body.library,
    }))
}
