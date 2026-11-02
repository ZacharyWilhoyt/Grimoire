//! Read-only provider adapters. Requests use fixed HTTPS hosts; file paths,
//! cookies, and credentials are never forwarded to metadata services.
use super::model::{self, Fields};
use crate::{auth, AppError, AppState};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    TmdbMovie,
    TmdbTv,
    Musicbrainz,
    Openlibrary,
}
impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Self::TmdbMovie => "tmdb_movie",
            Self::TmdbTv => "tmdb_tv",
            Self::Musicbrainz => "musicbrainz",
            Self::Openlibrary => "openlibrary",
        }
    }
    pub fn supports(self, kind: &str) -> bool {
        matches!(
            (self, kind),
            (Self::TmdbMovie | Self::TmdbTv, "video")
                | (Self::Musicbrainz, "music")
                | (Self::Openlibrary, "book")
        )
    }
    pub fn validate_id(self, id: &str) -> bool {
        match self {
            Self::TmdbMovie | Self::TmdbTv => id
                .parse::<u64>()
                .is_ok_and(|id| id > 0 && id < 10_000_000_000),
            Self::Musicbrainz => uuid::Uuid::parse_str(id).is_ok(),
            Self::Openlibrary => {
                id.starts_with("OL")
                    && id.ends_with('W')
                    && (4..=15).contains(&id.len())
                    && id[2..id.len() - 1]
                        .bytes()
                        .all(|byte| byte.is_ascii_digit())
            }
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Art {
    Tmdb(String),
    Musicbrainz(String),
    Openlibrary(u64),
}
#[derive(Serialize)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    pub creator: Option<String>,
    pub collection: Option<String>,
    pub year: Option<i32>,
}
pub struct Match {
    pub fields: Fields,
    pub art: Option<Art>,
}
#[derive(Serialize)]
pub struct Availability {
    pub provider: Provider,
    pub enabled: bool,
    pub message: Option<String>,
}
fn unavailable() -> AppError {
    AppError(
        StatusCode::BAD_GATEWAY,
        "The metadata provider is unavailable or rejected the request. Please try again later."
            .into(),
    )
}
pub fn enabled(state: &AppState, provider: Provider) -> bool {
    state.metadata.online
        && (!matches!(provider, Provider::TmdbMovie | Provider::TmdbTv)
            || state.metadata.tmdb_token.is_some())
}
pub fn availability(state: &AppState) -> Vec<Availability> {
    [
        Provider::TmdbMovie,
        Provider::TmdbTv,
        Provider::Musicbrainz,
        Provider::Openlibrary,
    ]
    .into_iter()
    .map(|provider| Availability {
        provider,
        enabled: enabled(state, provider),
        message: if !state.metadata.online {
            Some("Online metadata is disabled by the server operator.".into())
        } else if !enabled(state, provider) {
            Some("The server operator must configure GRIMOIRE_TMDB_TOKEN.".into())
        } else {
            None
        },
    })
    .collect()
}
fn allowed(url: &reqwest::Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return false;
    }
    url.host_str().is_some_and(|host| {
        matches!(
            host,
            "api.themoviedb.org"
                | "image.tmdb.org"
                | "musicbrainz.org"
                | "coverartarchive.org"
                | "openlibrary.org"
                | "covers.openlibrary.org"
                | "archive.org"
        ) || host.ends_with(".archive.org")
    })
}
async fn budget(state: &AppState) -> Result<(), AppError> {
    let wait:Option<f64>=sqlx::query_scalar("INSERT INTO metadata_provider_budget(id,next_request_at) VALUES (1,now()+interval '1.1 seconds') ON CONFLICT(id) DO UPDATE SET next_request_at=GREATEST(metadata_provider_budget.next_request_at,now())+interval '1.1 seconds' WHERE metadata_provider_budget.next_request_at<now()+interval '5 seconds' RETURNING GREATEST(extract(epoch from next_request_at-now())::float8-1.1,0)").fetch_optional(&state.db).await.map_err(|_|auth::unavailable())?;
    let wait = wait.ok_or_else(|| {
        AppError(
            StatusCode::TOO_MANY_REQUESTS,
            "Metadata lookups are busy. Try again shortly.".into(),
        )
    })?;
    tokio::time::sleep(Duration::from_secs_f64(wait.min(6.0))).await;
    Ok(())
}
pub async fn download(
    state: &AppState,
    mut url: reqwest::Url,
    limit: usize,
) -> Result<Vec<u8>, AppError> {
    for _ in 0..4 {
        if !allowed(&url) {
            return Err(unavailable());
        }
        budget(state).await?;
        let mut request = state.metadata.client.get(url.clone());
        if url.host_str() == Some("api.themoviedb.org") {
            request = request.bearer_auth(
                state
                    .metadata
                    .tmdb_token
                    .as_deref()
                    .ok_or_else(unavailable)?,
            );
        }
        let mut response = request.send().await.map_err(|_| unavailable())?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(unavailable)?;
            url = url.join(location).map_err(|_| unavailable())?;
            // Known cover services sometimes advertise HTTP redirects. Upgrade only
            // after retaining the same strict host allowlist on the next iteration.
            if url.scheme() == "http" {
                url.set_scheme("https").map_err(|_| unavailable())?;
            }
            continue;
        }
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > limit as u64)
        {
            return Err(unavailable());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if bytes.len() + chunk.len() > limit {
                return Err(unavailable());
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(bytes);
    }
    Err(unavailable())
}
async fn json(state: &AppState, url: reqwest::Url) -> Result<Value, AppError> {
    serde_json::from_slice(&download(state, url, 2 * 1024 * 1024).await?).map_err(|_| unavailable())
}
fn url(base: &str, query: &[(&str, &str)]) -> reqwest::Url {
    let mut url = reqwest::Url::parse(base).expect("fixed provider URL");
    url.query_pairs_mut().extend_pairs(query.iter().copied());
    url
}
fn string(value: &Value, key: &str, limit: usize) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|value| model::text(value, limit))
        .filter(|value| !value.is_empty())
}
fn year(value: &str) -> Option<i32> {
    value
        .chars()
        .take(4)
        .collect::<String>()
        .parse::<i32>()
        .ok()
        .filter(|year| (1..=9999).contains(year))
}
fn artist(value: &Value) -> Option<String> {
    value
        .get("artist-credit")
        .and_then(Value::as_array)
        .map(|credits| {
            credits
                .iter()
                .filter_map(|credit| {
                    string(credit, "name", 300).or_else(|| {
                        credit
                            .get("artist")
                            .and_then(|artist| string(artist, "name", 300))
                    })
                })
                .collect::<Vec<_>>()
                .join(" & ")
        })
        .filter(|name| !name.is_empty())
        .map(|name| model::text(&name, 300))
}
fn genres(value: &Value) -> Option<Vec<String>> {
    value
        .get("genres")
        .or_else(|| value.get("tags"))
        .and_then(Value::as_array)
        .map(|genres| {
            genres
                .iter()
                .filter_map(|genre| string(genre, "name", 60))
                .take(20)
                .collect()
        })
}
pub fn parse_search(provider: Provider, value: &Value) -> Vec<Candidate> {
    let values = value
        .get(match provider {
            Provider::TmdbMovie | Provider::TmdbTv => "results",
            Provider::Musicbrainz => "recordings",
            Provider::Openlibrary => "docs",
        })
        .and_then(Value::as_array);
    values
        .into_iter()
        .flatten()
        .take(6)
        .filter_map(|value| {
            let (id, title, creator, collection, date) = match provider {
                Provider::TmdbMovie | Provider::TmdbTv => (
                    value.get("id")?.as_u64()?.to_string(),
                    string(value, "title", 300).or_else(|| string(value, "name", 300))?,
                    None,
                    None,
                    string(value, "release_date", 20)
                        .or_else(|| string(value, "first_air_date", 20)),
                ),
                Provider::Musicbrainz => {
                    let release = value
                        .get("releases")
                        .and_then(Value::as_array)
                        .and_then(|items| items.first());
                    (
                        string(value, "id", 64)?,
                        string(value, "title", 300)?,
                        artist(value),
                        release.and_then(|release| string(release, "title", 300)),
                        release.and_then(|release| string(release, "date", 20)),
                    )
                }
                Provider::Openlibrary => (
                    string(value, "key", 80)?
                        .trim_start_matches("/works/")
                        .to_string(),
                    string(value, "title", 300)?,
                    value
                        .get("author_name")
                        .and_then(Value::as_array)
                        .map(|names| {
                            model::text(
                                &names
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .take(3)
                                    .collect::<Vec<_>>()
                                    .join(" & "),
                                300,
                            )
                        }),
                    None,
                    value
                        .get("first_publish_year")
                        .and_then(Value::as_i64)
                        .map(|year| year.to_string()),
                ),
            };
            if !provider.validate_id(&id) {
                return None;
            }
            Some(Candidate {
                id,
                title,
                creator,
                collection,
                year: date.as_deref().and_then(year),
            })
        })
        .collect()
}
pub async fn search(
    state: &AppState,
    provider: Provider,
    query: &str,
) -> Result<Vec<Candidate>, AppError> {
    let address = match provider {
        Provider::TmdbMovie => url(
            "https://api.themoviedb.org/3/search/movie",
            &[("query", query), ("include_adult", "false")],
        ),
        Provider::TmdbTv => url(
            "https://api.themoviedb.org/3/search/tv",
            &[("query", query), ("include_adult", "false")],
        ),
        Provider::Musicbrainz => {
            let escaped = query.replace('\\', "\\\\").replace('"', "\\\"");
            url(
                "https://musicbrainz.org/ws/2/recording",
                &[
                    ("query", &format!("\"{escaped}\"")),
                    ("fmt", "json"),
                    ("limit", "6"),
                ],
            )
        }
        Provider::Openlibrary => url(
            "https://openlibrary.org/search.json",
            &[
                ("q", query),
                ("limit", "6"),
                ("fields", "key,title,author_name,first_publish_year"),
            ],
        ),
    };
    Ok(parse_search(provider, &json(state, address).await?))
}
fn tmdb_art(value: &Value) -> Option<Art> {
    string(value, "poster_path", 240)
        .filter(|path| {
            path.starts_with('/')
                && !path.contains("..")
                && path
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"/_-.".contains(&byte))
        })
        .map(Art::Tmdb)
}
pub fn tmdb_fields(value: &Value) -> Fields {
    Fields {
        title: string(value, "title", 300).or_else(|| string(value, "name", 300)),
        year: string(value, "release_date", 20)
            .or_else(|| string(value, "first_air_date", 20))
            .as_deref()
            .and_then(year),
        description: string(value, "overview", 4000),
        genres: genres(value),
        collection: value
            .get("belongs_to_collection")
            .and_then(|collection| string(collection, "name", 300)),
        ..Fields::default()
    }
}
pub async fn preview(
    state: &AppState,
    provider: Provider,
    id: &str,
    context: &Fields,
) -> Result<Match, AppError> {
    if !provider.validate_id(id) {
        return Err(crate::bad_request("Invalid provider identifier."));
    }
    let result = match provider {
        Provider::TmdbMovie | Provider::TmdbTv => {
            let kind = if provider == Provider::TmdbMovie {
                "movie"
            } else {
                "tv"
            };
            let value = json(
                state,
                url(&format!("https://api.themoviedb.org/3/{kind}/{id}"), &[]),
            )
            .await?;
            let mut fields = tmdb_fields(&value);
            let art = tmdb_art(&value);
            if provider == Provider::TmdbTv {
                fields.collection = string(&value, "name", 300);
                fields.creator = value
                    .get("created_by")
                    .and_then(Value::as_array)
                    .map(|items| {
                        model::text(
                            &items
                                .iter()
                                .filter_map(|item| string(item, "name", 300))
                                .take(3)
                                .collect::<Vec<_>>()
                                .join(" & "),
                            300,
                        )
                    });
                if let (Some(season), Some(episode)) = (context.season, context.episode) {
                    let detail=json(state,url(&format!("https://api.themoviedb.org/3/tv/{id}/season/{season}/episode/{episode}"),&[])).await?;
                    fields.title = string(&detail, "name", 300);
                    fields.description = string(&detail, "overview", 4000);
                    fields.season = Some(season);
                    fields.episode = Some(episode);
                }
            }
            Match { fields, art }
        }
        Provider::Musicbrainz => {
            let value = json(
                state,
                url(
                    &format!("https://musicbrainz.org/ws/2/recording/{id}"),
                    &[("fmt", "json"), ("inc", "artists+releases+genres+tags")],
                ),
            )
            .await?;
            let release = value
                .get("releases")
                .and_then(Value::as_array)
                .and_then(|items| items.first());
            Match {
                fields: Fields {
                    title: string(&value, "title", 300),
                    creator: artist(&value),
                    collection: release.and_then(|release| string(release, "title", 300)),
                    year: release
                        .and_then(|release| string(release, "date", 20))
                        .as_deref()
                        .and_then(year),
                    genres: genres(&value),
                    ..Fields::default()
                },
                art: release
                    .and_then(|release| string(release, "id", 64))
                    .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                    .map(Art::Musicbrainz),
            }
        }
        Provider::Openlibrary => {
            let value = json(
                state,
                url(&format!("https://openlibrary.org/works/{id}.json"), &[]),
            )
            .await?;
            let mut authors = Vec::new();
            for author in value
                .get("authors")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .take(2)
            {
                if let Some(key) = author
                    .get("author")
                    .and_then(|author| author.get("key"))
                    .and_then(Value::as_str)
                {
                    let id = key.trim_start_matches("/authors/");
                    if id.starts_with("OL")
                        && id.ends_with('A')
                        && (4..=15).contains(&id.len())
                        && id[2..id.len() - 1]
                            .bytes()
                            .all(|byte| byte.is_ascii_digit())
                    {
                        if let Ok(author) = json(
                            state,
                            url(&format!("https://openlibrary.org/authors/{id}.json"), &[]),
                        )
                        .await
                        {
                            if let Some(name) = string(&author, "name", 300) {
                                authors.push(name);
                            }
                        }
                    }
                }
            }
            let description = value
                .get("description")
                .and_then(|description| {
                    description
                        .as_str()
                        .or_else(|| description.get("value").and_then(Value::as_str))
                })
                .map(|text| model::text(text, 4000));
            let art = value
                .get("covers")
                .and_then(Value::as_array)
                .and_then(|covers| {
                    covers
                        .iter()
                        .filter_map(Value::as_u64)
                        .find(|id| *id > 0 && *id < 100_000_000_000)
                })
                .map(Art::Openlibrary);
            Match {
                fields: Fields {
                    title: string(&value, "title", 300),
                    creator: (!authors.is_empty()).then(|| model::text(&authors.join(" & "), 300)),
                    year: string(&value, "first_publish_date", 50)
                        .as_deref()
                        .and_then(year),
                    description,
                    genres: value
                        .get("subjects")
                        .and_then(Value::as_array)
                        .map(|subjects| {
                            subjects
                                .iter()
                                .filter_map(Value::as_str)
                                .take(20)
                                .map(|subject| model::text(subject, 60))
                                .collect()
                        }),
                    ..Fields::default()
                },
                art,
            }
        }
    };
    Ok(Match {
        fields: result.fields.normalize()?,
        art: result.art,
    })
}
pub async fn artwork(state: &AppState, art: &Art) -> Result<Vec<u8>, AppError> {
    let address = match art {
        Art::Tmdb(path) => format!("https://image.tmdb.org/t/p/w500{path}"),
        Art::Musicbrainz(id) => {
            if uuid::Uuid::parse_str(id).is_err() {
                return Err(unavailable());
            }
            format!("https://coverartarchive.org/release/{id}/front-500")
        }
        Art::Openlibrary(id) => {
            format!("https://covers.openlibrary.org/b/id/{id}-L.jpg?default=false")
        }
    };
    let bytes = download(
        state,
        reqwest::Url::parse(&address).map_err(|_| unavailable())?,
        8 * 1024 * 1024,
    )
    .await?;
    tokio::task::spawn_blocking(move || super::local::jpeg(&bytes))
        .await
        .map_err(|_| unavailable())?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_hosts_and_ids_are_bounded() {
        assert!(!allowed(
            &reqwest::Url::parse("http://image.tmdb.org/test").unwrap()
        ));
        for address in [
            "https://127.0.0.1/test",
            "https://archive.org.attacker.test/test",
            "https://user:secret@openlibrary.org/",
            "https://openlibrary.org:444/",
        ] {
            assert!(!allowed(&reqwest::Url::parse(address).unwrap()));
        }
        assert!(Provider::Openlibrary.validate_id("OL123W"));
        assert!(!Provider::Openlibrary.validate_id("OL123W/../../admin"));
        assert!(!Provider::TmdbTv.supports("music"));
    }
    #[test]
    fn provider_results_preserve_identity() {
        let candidates = parse_search(
            Provider::Openlibrary,
            &serde_json::json!({"docs":[{"key":"/works/OL123W","title":"Book","author_name":["Writer"],"first_publish_year":1980}]}),
        );
        assert_eq!(candidates[0].id, "OL123W");
        assert_eq!(candidates[0].creator.as_deref(), Some("Writer"));
        let fields = tmdb_fields(
            &serde_json::json!({"title":"Film","genres":[{"name":"Drama"}],"release_date":"2001-01-01"}),
        );
        assert_eq!(fields.year, Some(2001));
    }
}
