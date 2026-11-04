use crate::{bad_request, AppError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Fields {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub collection: Option<String>,
    pub year: Option<i32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub volume: Option<f64>,
    pub duration: Option<f64>,
    pub genres: Option<Vec<String>>,
    pub description: Option<String>,
}
impl Fields {
    pub fn normalize(mut self) -> Result<Self, AppError> {
        for (field, limit) in [
            (&mut self.title, 300),
            (&mut self.creator, 300),
            (&mut self.collection, 300),
            (&mut self.description, 4000),
        ] {
            if let Some(value) = field {
                *value = value.trim().to_string();
                if value.chars().count() > limit {
                    return Err(bad_request("A metadata field exceeds its length limit."));
                }
            }
        }
        if self
            .duration
            .is_some_and(|value| !value.is_finite() || value < 0.0)
        {
            return Err(bad_request("Invalid media duration."));
        }
        if self.year.is_some_and(|year| !(1..=9999).contains(&year))
            || [self.season, self.episode, self.track, self.disc]
                .into_iter()
                .flatten()
                .any(|number| number > 100_000)
            || self
                .volume
                .is_some_and(|number| !number.is_finite() || !(0.0..=100_000.0).contains(&number))
        {
            return Err(bad_request("Invalid year or collection position."));
        }
        if let Some(genres) = &mut self.genres {
            if genres.len() > 20 {
                return Err(bad_request("Use at most 20 genres."));
            }
            let mut normalized = Vec::new();
            for genre in genres.iter() {
                let genre = genre
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase();
                if genre.chars().count() > 60 {
                    return Err(bad_request("Genre names must be at most 60 characters."));
                }
                if !genre.is_empty() && !normalized.contains(&genre) {
                    normalized.push(genre);
                }
            }
            *genres = normalized;
        }
        Ok(self)
    }
    pub fn overlay(&mut self, other: Self) {
        macro_rules! apply { ($($field:ident),*) => { $(if other.$field.is_some() { self.$field = other.$field; })* }; }
        apply!(
            title,
            creator,
            collection,
            year,
            season,
            episode,
            track,
            disc,
            volume,
            duration,
            genres,
            description
        );
    }
}
pub fn text(input: &str, limit: usize) -> String {
    input.trim().chars().take(limit).collect()
}
pub fn filename_title(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .replace('_', " ")
        .trim()
        .to_string()
}
pub fn fallback(path: &str) -> Fields {
    Fields {
        title: Some(filename_title(path)),
        ..Fields::default()
    }
}
pub fn effective(path: &str, local: &Fields, matched: &Fields, custom: Option<&Fields>) -> Fields {
    let mut fields = super::local::filename_fields(path);
    fields.overlay(local.clone());
    fields.overlay(matched.clone());
    if let Some(custom) = custom {
        fields = custom.clone();
    }
    if fields
        .title
        .as_deref()
        .is_none_or(|title| title.trim().is_empty())
    {
        fields.title = Some(filename_title(path));
    }
    fields
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ArtworkPreference {
    #[default]
    Auto,
    Local,
    Matched,
    None,
}
impl ArtworkPreference {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Local => "local",
            Self::Matched => "matched",
            Self::None => "none",
        }
    }
}
