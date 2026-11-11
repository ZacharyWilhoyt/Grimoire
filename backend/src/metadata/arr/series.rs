//! Reviewed Sonarr imports scoped to an existing video grouping.
use super::*;

const MAX_EPISODES: usize = 1000;

#[derive(Serialize, Deserialize)]
struct Episode {
    path: String,
    provider_id: String,
    fields: Fields,
}

#[derive(Serialize, Deserialize)]
struct SeriesReview {
    title: String,
    synopsis: Option<String>,
    episodes: Vec<Episode>,
}

fn local_path(row: &Connection, path: &str) -> Option<String> {
    let path = root(path).ok()?;
    let tail = path.strip_prefix(&format!("{}/", row.remote_root.trim_end_matches('/')))?;
    let relative = if row.source_subpath.is_empty() {
        tail.to_string()
    } else {
        format!("{}/{tail}", row.source_subpath)
    };
    (target(row, &relative).ok().as_deref() == Some(path.as_str())).then_some(relative)
}

async fn match_series(
    state: &AppState,
    row: &Connection,
    group: &str,
    anchor: &str,
    source_root: &std::path::Path,
) -> Result<(SeriesReview, Option<Cover>, i64), AppError> {
    let api = api(state, row).await?;
    let series = get(state, row, &format!("{api}/series"), &[]).await?;
    let series = parent(array(&series)?, &target(row, anchor)?)?;
    let series_id = id(series)?;
    let query = [("seriesId", series_id.to_string())];
    let files = get(state, row, &format!("{api}/episodefile"), &query).await?;
    let episodes = get(state, row, &format!("{api}/episode"), &query).await?;
    let mut by_file = HashMap::<i64, Vec<&Value>>::new();
    for episode in array(&episodes)? {
        if let Some(file) = episode["episodeFileId"].as_i64().filter(|id| *id > 0) {
            by_file.entry(file).or_default().push(episode);
        }
    }
    let mut paths = HashMap::<String, Option<i64>>::new();
    for file in array(&files)? {
        let full = file["path"].as_str().map(str::to_string).or_else(|| {
            Some(format!(
                "{}/{}",
                series["path"].as_str()?,
                file["relativePath"].as_str()?
            ))
        });
        if let Some(path) = full.as_deref().and_then(|path| local_path(row, path)) {
            paths
                .entry(path)
                .and_modify(|id| *id = None)
                .or_insert(Some(id(file)?));
        }
    }
    let keys: Vec<_> = paths
        .keys()
        .map(|path| catalog::file_key(&row.source_id, path))
        .collect();
    let records = sqlx::query_as::<_, Record>(
        "SELECT * FROM media_metadata WHERE source_id=$1 AND file_key=ANY($2)",
    )
    .bind(&row.source_id)
    .bind(keys)
    .fetch_all(&state.db)
    .await
    .map_err(|_| auth::unavailable())?;
    let records: HashMap<_, _> = records
        .iter()
        .map(|record| (record.file_key.as_str(), record))
        .collect();
    let mut matches = Vec::new();
    for (path, file_id) in paths {
        let Some(file_id) = file_id else { continue };
        let Some(episodes) = by_file.get(&file_id) else {
            continue;
        };
        let key = catalog::file_key(&row.source_id, &path);
        let existing = view(&path, &key, records.get(key.as_str()).copied(), true, true);
        if groups::identity(&row.source_id, "video", &existing.fields)
            .is_none_or(|value| value.id != group)
        {
            continue;
        }
        let Ok(resolved) = crate::resolve_media_path(source_root, &path, "video") else {
            continue;
        };
        if resolved
            .strip_prefix(source_root)
            .ok()
            .and_then(|path| path.to_str())
            != Some(path.as_str())
        {
            continue;
        }
        let mut fields = sonarr_episode_fields(series, episodes.clone())?;
        // Use the existing collection label so the import preserves the group ID and link.
        fields.collection = existing.fields.collection;
        matches.push(Episode {
            path,
            provider_id: format!("{series_id}:{file_id}"),
            fields: fields.normalize()?,
        });
        if matches.len() > MAX_EPISODES {
            return Err(crate::bad_request(
                "A series review supports up to 1,000 matched files.",
            ));
        }
    }
    if matches.is_empty() {
        return Err(crate::bad_request("Sonarr did not identify any exact episode files in this grouping. Check the connection’s folder mapping and imported library."));
    }
    matches.sort_by(|a, b| {
        (a.fields.season, a.fields.episode, &a.path).cmp(&(
            b.fields.season,
            b.fields.episode,
            &b.path,
        ))
    });
    let review = SeriesReview {
        title: string(series, "title", 300).ok_or_else(remote_error)?,
        synopsis: string(series, "overview", 4000),
        episodes: matches,
    };
    Ok((review, cover(series, "series", &api), series_id))
}

pub async fn preview(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, group, connection)): Path<(String, String, Uuid)>,
    Json(body): Json<PreviewFile>,
) -> Result<Json<Value>, AppError> {
    let (anchor, _) =
        groups::member(&state, &identity.user, &source, &group, &body.path, true).await?;
    let row = load_connection(&state, &source, connection).await?;
    if anchor.kind != "video" || row.kind()? != Kind::Sonarr {
        return Err(crate::bad_request(
            "Choose a Sonarr connection for this video series.",
        ));
    }
    let _permit = guard(&state, identity.user.id, &row).await?;
    let source_root = crate::media_root(&state, &identity.user, &source).await?;
    let (review, artwork, series_id) =
        match_series(&state, &row, &group, &body.path, &source_root).await?;
    let data = serde_json::to_string(&review).map_err(|_| auth::unavailable())?;
    if data.len() > 8 * 1024 * 1024 {
        return Err(crate::bad_request(
            "The series review exceeds its size limit.",
        ));
    }
    let token = Uuid::new_v4();
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    groups::locked_member(&mut tx, &source, &group, &body.path, &anchor).await?;
    check_review(&mut tx, &source, connection, Some(row.revision)).await?;
    sqlx::query("DELETE FROM metadata_reviews WHERE expires_at<now()")
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    sqlx::query("INSERT INTO metadata_reviews(id,owner_id,source_id,file_key,provider,provider_id,data,artwork_ref,arr_connection_id,arr_revision,group_id) VALUES ($1,$2,$3,$4,'arr_sonarr',$5,$6,$7,$8,$9,$10)")
        .bind(token).bind(identity.user.id).bind(&source).bind(anchor.key).bind(series_id.to_string()).bind(data)
        .bind(artwork.as_ref().map(serde_json::to_string).transpose().map_err(|_| auth::unavailable())?)
        .bind(connection).bind(row.revision).bind(group).execute(&mut *tx).await.map_err(|_| auth::unavailable())?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(
        json!({"review_token":token,"title":review.title,"synopsis":review.synopsis,"episodes":review.episodes,"has_artwork":artwork.is_some()}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplySeries {
    path: String,
    review_token: Uuid,
    synopsis: bool,
    episodes: bool,
}

pub async fn apply(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, group)): Path<(String, String)>,
    Json(body): Json<ApplySeries>,
) -> Result<Json<Value>, AppError> {
    let (anchor, _) =
        groups::member(&state, &identity.user, &source, &group, &body.path, true).await?;
    let row = sqlx::query_as::<_, ReviewRow>("SELECT provider,provider_id,data,artwork_ref,arr_connection_id,arr_revision FROM metadata_reviews WHERE id=$1 AND owner_id=$2 AND source_id=$3 AND file_key=$4 AND group_id=$5 AND expires_at>now()")
        .bind(body.review_token).bind(identity.user.id).bind(&source).bind(&anchor.key).bind(&group)
        .fetch_optional(&state.db).await.map_err(|_| auth::unavailable())?.ok_or_else(expired)?;
    let review: SeriesReview = serde_json::from_str(&row.data).map_err(|_| auth::unavailable())?;
    let synopsis = review.synopsis.as_deref().filter(|_| body.synopsis);
    let episodes = if body.episodes {
        review.episodes
    } else {
        Vec::new()
    };
    if synopsis.is_none() && episodes.is_empty() {
        return Err(crate::bad_request(
            "Select at least one available series update.",
        ));
    }
    let connection = row.arr_connection_id.ok_or_else(expired)?;
    let connection_row = load_connection(&state, &source, connection).await?;
    let _permit = guard(&state, identity.user.id, &connection_row).await?;
    let mut warnings = Vec::new();
    let artwork = if episodes.is_empty() {
        None
    } else {
        review_artwork(
            &state,
            &source,
            connection,
            row.arr_revision,
            row.artwork_ref.as_deref(),
            &mut warnings,
        )
        .await?
    };
    let root = crate::media_root(&state, &identity.user, &source).await?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    groups::locked_member(&mut tx, &source, &group, &body.path, &anchor).await?;
    check_review(&mut tx, &source, connection, row.arr_revision).await?;
    let removed = sqlx::query("DELETE FROM metadata_reviews WHERE id=$1 AND owner_id=$2 AND source_id=$3 AND file_key=$4 AND group_id=$5 AND expires_at>now()")
        .bind(body.review_token).bind(identity.user.id).bind(&source).bind(&anchor.key).bind(&group)
        .execute(&mut *tx).await.map_err(|_| auth::unavailable())?;
    if removed.rows_affected() != 1 {
        return Err(expired());
    }
    if let Some(synopsis) = synopsis {
        groups::write_synopsis(
            &mut tx,
            &source,
            &group,
            identity.user.id,
            Some(synopsis),
            Some("arr_sonarr"),
        )
        .await?;
    }
    let artwork = store_art(&mut tx, artwork).await?;
    for episode in &episodes {
        let path = crate::resolve_media_path(&root, &episode.path, "video")?;
        if path.strip_prefix(&root).ok().and_then(|path| path.to_str())
            != Some(episode.path.as_str())
        {
            return Err(expired());
        }
        let file = Reference {
            key: catalog::file_key(&source, &episode.path),
            path,
            kind: "video",
        };
        groups::locked_member(&mut tx, &source, &group, &episode.path, &file).await?;
        ensure_row(&mut tx, &source, &episode.path, &file.key).await?;
        let before: String =
            sqlx::query_scalar("SELECT matched_data FROM media_metadata WHERE file_key=$1")
                .bind(&file.key)
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| auth::unavailable())?;
        let data = serde_json::to_string(&episode.fields).map_err(|_| auth::unavailable())?;
        sqlx::query("UPDATE media_metadata SET matched_data=$2,provider='arr_sonarr',provider_id=$3,matched_artwork=$4,updated_at=now() WHERE file_key=$1")
            .bind(&file.key).bind(&data).bind(&episode.provider_id).bind(&artwork)
            .execute(&mut *tx).await.map_err(|_| auth::unavailable())?;
        audit(
            &mut tx,
            &source,
            &file.key,
            identity.user.id,
            "reviewed_series_episode_applied",
            Some(before),
            Some(data),
        )
        .await?;
    }
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(
        json!({"episodes_updated":episodes.len(),"synopsis_updated":synopsis.is_some(),"warnings":warnings}),
    ))
}

fn expired() -> AppError {
    crate::bad_request("This series review expired or no longer matches. Preview it again.")
}
