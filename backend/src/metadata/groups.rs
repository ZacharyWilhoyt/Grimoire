//! Source-scoped album/series overrides, inherited dynamically by current members.
use super::*;
use axum::extract::Request;

#[derive(Clone, Serialize)]
pub struct GroupView {
    pub id: String,
    pub label: String,
    pub can_edit: bool,
    pub has_custom_genres: bool,
    pub has_custom_artwork: bool,
}
#[derive(Clone, FromRow)]
pub(super) struct GroupRow {
    id: String,
    genres: Option<String>,
    artwork: Option<String>,
}
fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn identity(source: &str, kind: &str, fields: &Fields) -> Option<GroupView> {
    let collection = fields.collection.as_deref()?.trim();
    if collection.is_empty() {
        return None;
    }
    let creator = if kind == "video" {
        ""
    } else {
        fields.creator.as_deref().unwrap_or("").trim()
    };
    // Serialize a tuple to avoid delimiter collisions in owner-controlled labels.
    let tuple =
        serde_json::to_string(&(source, kind, normalize(creator), normalize(collection))).ok()?;
    Some(GroupView {
        id: auth::digest(&tuple),
        label: if creator.is_empty() {
            collection.into()
        } else {
            format!("{creator} — {collection}")
        },
        can_edit: false,
        has_custom_genres: false,
        has_custom_artwork: false,
    })
}
fn image_url(source: &str, group: &str, path: &str, hash: &str) -> String {
    let mut url = reqwest::Url::parse("https://grimoire.invalid/").expect("fixed URL");
    url.path_segments_mut()
        .expect("URL path")
        .extend(["api", "sources", source, "groups", group, "artwork"]);
    url.query_pairs_mut()
        .append_pair("path", path)
        .append_pair("v", hash);
    format!("{}?{}", url.path(), url.query().unwrap_or(""))
}
pub(super) async fn load_for_sources(
    state: &AppState,
    user: Uuid,
    items: &[catalog::Item],
) -> Result<HashMap<String, GroupRow>, AppError> {
    let sources: Vec<_> = items
        .iter()
        .map(|item| item.source_id.as_str())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    // Recheck access when loading group overrides, after the catalog scan.
    let rows = sqlx::query_as::<_, GroupRow>(
        "SELECT g.id,g.genres,g.artwork FROM media_groups g JOIN sources s ON s.id=g.source_id WHERE g.source_id=ANY($2) AND (s.owner_id=$1 OR EXISTS(SELECT 1 FROM source_permissions p WHERE p.source_id=s.id AND p.user_id=$1 AND p.can_discover AND p.can_access_media))",
    )
    .bind(user)
    .bind(sources)
    .fetch_all(&state.db)
    .await
    .map_err(|_| auth::unavailable())?;
    Ok(rows.into_iter().map(|row| (row.id.clone(), row)).collect())
}
pub(super) fn overlay(
    source: &str,
    kind: &str,
    path: &str,
    metadata: &mut View,
    record: Option<&Record>,
    rows: &HashMap<String, GroupRow>,
) {
    let Some(mut group) = identity(source, kind, &metadata.fields) else {
        return;
    };
    group.can_edit = metadata.can_edit;
    if let Some(row) = rows.get(&group.id) {
        if let Some(genres) = row
            .genres
            .as_deref()
            .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        {
            metadata.fields.genres = Some(genres);
            group.has_custom_genres = true;
        }
        if let Some(hash) = row.artwork.as_deref() {
            // The catalog caller clears URLs for unavailable personal-library items.
            metadata.artwork_url = Some(image_url(source, &group.id, path, hash));
            group.has_custom_artwork = true;
        }
    }
    metadata.credits = super::credits(
        record,
        !metadata.has_custom,
        group.has_custom_genres,
        group.has_custom_artwork,
    );
    metadata.group = Some(group);
}
pub(super) async fn enrich_one(
    state: &AppState,
    source: &str,
    kind: &str,
    path: &str,
    metadata: &mut View,
    record: Option<&Record>,
) -> Result<(), AppError> {
    let Some(group) = identity(source, kind, &metadata.fields) else {
        return Ok(());
    };
    let row = sqlx::query_as::<_, GroupRow>(
        "SELECT id,genres,artwork FROM media_groups WHERE source_id=$1 AND id=$2",
    )
    .bind(source)
    .bind(&group.id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| auth::unavailable())?;
    let rows = row.into_iter().map(|row| (row.id.clone(), row)).collect();
    overlay(source, kind, path, metadata, record, &rows);
    Ok(())
}
async fn member(
    state: &AppState,
    user: &auth::User,
    source: &str,
    id: &str,
    path: &str,
    owner: bool,
) -> Result<(Reference, GroupView), AppError> {
    let file = reference(state, user, source, path, owner).await?;
    let record = load(state, &file.key).await?;
    let metadata = view(path, &file.key, record.as_ref(), owner, true);
    let group = identity(source, file.kind, &metadata.fields)
        .filter(|group| group.id == id)
        .ok_or_else(|| {
            crate::bad_request("This file's album/series changed. Refresh and reopen the group.")
        })?;
    Ok((file, group))
}
async fn locked_member(
    tx: &mut Transaction<'_, Postgres>,
    source: &str,
    id: &str,
    path: &str,
    file: &Reference,
) -> Result<(), AppError> {
    let record = sqlx::query_as::<_, Record>("SELECT * FROM media_metadata WHERE file_key=$1")
        .bind(&file.key)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| auth::unavailable())?;
    let metadata = view(path, &file.key, record.as_ref(), true, true);
    if identity(source, file.kind, &metadata.fields).is_none_or(|group| group.id != id) {
        return Err(crate::bad_request(
            "Group membership changed. Refresh and try again.",
        ));
    }
    Ok(())
}
#[derive(Serialize)]
pub struct Settings {
    group: GroupView,
    genres: Option<Vec<String>>,
    artwork_url: Option<String>,
    effective_genres: Vec<String>,
    effective_artwork_url: Option<String>,
}
async fn settings(
    state: &AppState,
    user: &auth::User,
    source: &str,
    id: &str,
    path: &str,
) -> Result<Settings, AppError> {
    let (file, mut group) = member(state, user, source, id, path, false).await?;
    group.can_edit =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sources WHERE id=$1 AND owner_id=$2)")
            .bind(source)
            .bind(user.id)
            .fetch_one(&state.db)
            .await
            .map_err(|_| auth::unavailable())?;
    let row = sqlx::query_as::<_, GroupRow>(
        "SELECT id,genres,artwork FROM media_groups WHERE source_id=$1 AND id=$2",
    )
    .bind(source)
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| auth::unavailable())?;
    let genres = row
        .as_ref()
        .and_then(|row| row.genres.as_deref())
        .and_then(|value| serde_json::from_str(value).ok());
    let artwork_url = row
        .as_ref()
        .and_then(|row| row.artwork.as_ref())
        .map(|hash| image_url(source, id, path, hash));
    group.has_custom_genres = genres.is_some();
    group.has_custom_artwork = artwork_url.is_some();
    let effective = current_view(state, user, source, path, &file.key).await?;
    Ok(Settings {
        effective_genres: effective.fields.genres.unwrap_or_default(),
        effective_artwork_url: effective.artwork_url,
        group,
        genres,
        artwork_url,
    })
}
pub async fn details(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, id)): Path<(String, String)>,
    Query(query): Query<FileQuery>,
) -> Result<Json<Settings>, AppError> {
    Ok(Json(
        settings(&state, &identity.user, &source, &id, &query.path).await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Genres {
    path: String,
    genres: Vec<String>,
}
pub async fn set_genres(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, id)): Path<(String, String)>,
    Json(body): Json<Genres>,
) -> Result<Json<Settings>, AppError> {
    let (file, _) = member(&state, &identity.user, &source, &id, &body.path, true).await?;
    let genres = Fields {
        genres: Some(body.genres),
        ..Fields::default()
    }
    .normalize()?
    .genres;
    let data = serde_json::to_string(&genres).map_err(|_| auth::unavailable())?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    locked_member(&mut tx, &source, &id, &body.path, &file).await?;
    let before: Option<String> =
        sqlx::query_scalar("SELECT genres FROM media_groups WHERE source_id=$1 AND id=$2")
            .bind(&source)
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?
            .flatten();
    sqlx::query("INSERT INTO media_groups(id,source_id,genres) VALUES ($1,$2,$3) ON CONFLICT(id) DO UPDATE SET genres=$3,updated_at=now() WHERE media_groups.source_id=$2").bind(&id).bind(&source).bind(&data).execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
    audit(
        &mut tx,
        &source,
        &id,
        identity.user.id,
        "group_genres_edited",
        before,
        Some(data),
    )
    .await?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(
        settings(&state, &identity.user, &source, &id, &body.path).await?,
    ))
}
pub async fn upload(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, id)): Path<(String, String)>,
    Query(query): Query<FileQuery>,
    request: Request,
) -> Result<Json<Settings>, AppError> {
    let (file, _) = member(&state, &identity.user, &source, &id, &query.path, true).await?;
    auth::rate_limit(
        &state.db,
        format!("group-artwork:user:{}", identity.user.id),
        30,
        300,
    )
    .await?;
    let _permit = state
        .metadata
        .work
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            AppError(
                StatusCode::TOO_MANY_REQUESTS,
                "Image processing is busy. Try again shortly.".into(),
            )
        })?;
    let bytes = tokio::time::timeout(
        Duration::from_secs(30),
        axum::body::to_bytes(request.into_body(), 8 * 1024 * 1024),
    )
    .await
    .map_err(|_| {
        AppError(
            StatusCode::REQUEST_TIMEOUT,
            "Image upload timed out.".into(),
        )
    })?
    .map_err(|_| {
        AppError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "The image must be at most 8 MiB.".into(),
        )
    })?;
    let image = tokio::task::spawn_blocking(move || super::local::jpeg(&bytes))
        .await
        .map_err(|_| auth::unavailable())?
        .map_err(|_| {
            crate::bad_request(
                "Use a valid JPEG, PNG, WebP or GIF, at most 8 MiB and 4096 pixels per side.",
            )
        })?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    locked_member(&mut tx, &source, &id, &query.path, &file).await?;
    let hash = store_art(&mut tx, Some(image)).await?;
    let before: Option<String> =
        sqlx::query_scalar("SELECT artwork FROM media_groups WHERE source_id=$1 AND id=$2")
            .bind(&source)
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?
            .flatten();
    sqlx::query("INSERT INTO media_groups(id,source_id,artwork) VALUES ($1,$2,$3) ON CONFLICT(id) DO UPDATE SET artwork=$3,updated_at=now() WHERE media_groups.source_id=$2").bind(&id).bind(&source).bind(&hash).execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
    audit(
        &mut tx,
        &source,
        &id,
        identity.user.id,
        "group_artwork_uploaded",
        before,
        hash,
    )
    .await?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(
        settings(&state, &identity.user, &source, &id, &query.path).await?,
    ))
}
async fn reset(
    state: &AppState,
    user: &auth::User,
    source: &str,
    id: &str,
    path: &str,
    art: bool,
) -> Result<Settings, AppError> {
    let (file, _) = member(state, user, source, id, path, true).await?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, source, user.id).await?;
    locked_member(&mut tx, source, id, path, &file).await?;
    let sql = if art {
        "UPDATE media_groups SET artwork=NULL,updated_at=now() WHERE id=$1 AND source_id=$2"
    } else {
        "UPDATE media_groups SET genres=NULL,updated_at=now() WHERE id=$1 AND source_id=$2"
    };
    sqlx::query(sql)
        .bind(id)
        .bind(source)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    audit(
        &mut tx,
        source,
        id,
        user.id,
        if art {
            "group_artwork_reset"
        } else {
            "group_genres_reset"
        },
        None,
        None,
    )
    .await?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    settings(state, user, source, id, path).await
}
pub async fn reset_genres(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, id)): Path<(String, String)>,
    Query(query): Query<FileQuery>,
) -> Result<Json<Settings>, AppError> {
    Ok(Json(
        reset(&state, &identity.user, &source, &id, &query.path, false).await?,
    ))
}
pub async fn reset_artwork(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, id)): Path<(String, String)>,
    Query(query): Query<FileQuery>,
) -> Result<Json<Settings>, AppError> {
    Ok(Json(
        reset(&state, &identity.user, &source, &id, &query.path, true).await?,
    ))
}
pub async fn artwork(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, id)): Path<(String, String)>,
    Query(query): Query<FileQuery>,
) -> Result<Response, AppError> {
    member(&state, &identity.user, &source, &id, &query.path, false).await?;
    let bytes:Option<Vec<u8>>=sqlx::query_scalar("SELECT a.jpeg FROM media_groups g JOIN metadata_artwork a ON a.id=g.artwork WHERE g.source_id=$1 AND g.id=$2").bind(source).bind(id).fetch_optional(&state.db).await.map_err(|_|auth::unavailable())?;
    Ok((
        [
            ("content-type", "image/jpeg"),
            ("cache-control", "private, no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        bytes.ok_or_else(not_found)?,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_is_source_scoped_and_independent_of_season_and_tags() {
        let mut fields = Fields {
            creator: Some("  An  Artist ".into()),
            collection: Some(" An Album ".into()),
            season: Some(1),
            ..Fields::default()
        };
        let first = identity("source", "music", &fields).unwrap();
        fields.creator = Some("an artist".into());
        fields.collection = Some("an   album".into());
        fields.season = Some(2);
        fields.genres = Some(vec!["jazz".into()]);
        assert_eq!(first.id, identity("source", "music", &fields).unwrap().id);
        assert_ne!(first.id, identity("other", "music", &fields).unwrap().id);
        assert_ne!(first.id, identity("source", "book", &fields).unwrap().id);
        fields.collection = None;
        assert!(identity("source", "music", &fields).is_none());
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Join {
    path: String,
    target_path: String,
    expected_group_id: String,
}
/// Copy group identity, never file identity or per-item numbering/provider IDs.
pub async fn join(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Json(body): Json<Join>,
) -> Result<Json<View>, AppError> {
    let item = reference(&state, &identity.user, &source, &body.path, true).await?;
    let target = reference(&state, &identity.user, &source, &body.target_path, true).await?;
    if item.kind != target.kind || item.key == target.key {
        return Err(crate::bad_request(
            "Choose a different item of the same media type in this source.",
        ));
    }
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    let record = sqlx::query_as::<_, Record>("SELECT * FROM media_metadata WHERE file_key=$1")
        .bind(&target.key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    let fields = view(&body.target_path, &target.key, record.as_ref(), true, true).fields;
    let group = self::identity(&source, target.kind, &fields)
        .ok_or_else(|| crate::bad_request("The target must belong to an album or series."))?;
    if group.id != body.expected_group_id {
        return Err(AppError(
            StatusCode::CONFLICT,
            "The target group changed. Refresh and try again.".into(),
        ));
    }
    ensure_row(&mut tx, &source, &body.path, &item.key).await?;
    let previous =
        sqlx::query_as::<_, Record>("SELECT * FROM media_metadata WHERE file_key=$1 FOR UPDATE")
            .bind(&item.key)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
    let before = previous.custom_data.clone();
    let mut custom = view(&body.path, &item.key, Some(&previous), true, true).fields;
    custom.collection = fields.collection;
    if item.kind != "video" {
        custom.creator = Some(fields.creator.unwrap_or_default());
    }
    custom.genres = Some(fields.genres.unwrap_or_default());
    if fields.year.is_some() {
        custom.year = fields.year;
    }
    let data = serde_json::to_string(&custom.normalize()?).map_err(|_| auth::unavailable())?;
    sqlx::query("UPDATE media_metadata SET custom_data=$2,updated_at=now() WHERE file_key=$1")
        .bind(&item.key)
        .bind(&data)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    audit(
        &mut tx,
        &source,
        &item.key,
        identity.user.id,
        "group_joined",
        before,
        Some(data),
    )
    .await?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(
        current_view(&state, &identity.user, &source, &body.path, &item.key).await?,
    ))
}
