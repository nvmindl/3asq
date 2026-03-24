#![no_std]

use aidoku::{
    alloc::{string::ToString, vec, String, Vec},
    helpers::uri::encode_uri_component,
    imports::{
        defaults::{defaults_get, defaults_set, DefaultValue},
        net::Request,
        std::parse_date,
    },
    prelude::*,
    Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Listing, ListingProvider, Manga,
    MangaPageResult, MangaStatus, Page, PageContent, Result, Source, Viewer,
};
use serde::Deserialize;

const API_URL: &str = "https://appswat.com/v2/api/v2";
const BASE_URL: &str = "https://meshmanga.com";
const PAGE_SIZE: usize = 20;

// ---------------------------------------------------------------------------
// API data models
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ApiResponse<T> {
    count: i64,
    next: Option<String>,
    previous: Option<String>,
    results: Vec<T>,
}

#[derive(Deserialize)]
struct SeriesData {
    id: i64,
    title: String,
    slug: String,
    #[serde(default)]
    story: Option<String>,
    #[serde(default)]
    poster: Option<Poster>,
    #[serde(rename = "type")]
    series_type: Option<TypeData>,
    status: Option<StatusData>,
    #[serde(default)]
    genres: Vec<GenreData>,
    #[serde(rename = "chapters_count")]
    chapters_count: Option<i64>,
    #[serde(default)]
    author: Option<AuthorData>,
    #[serde(rename = "created_at")]
    created_at: Option<String>,
}

#[derive(Deserialize)]
struct Poster {
    thumbnail: Option<String>,
    medium: Option<String>,
}

#[derive(Deserialize)]
struct TypeData {
    name: String,
}

#[derive(Deserialize)]
struct StatusData {
    name: String,
}

#[derive(Deserialize)]
struct GenreData {
    name: String,
}

#[derive(Deserialize)]
struct AuthorData {
    name: Option<String>,
}

#[derive(Deserialize)]
struct ChapterListItem {
    id: i64,
    title: String,
    slug: String,
    chapter: String,
    serie: i64,
    #[serde(rename = "views_count")]
    views_count: Option<i64>,
    #[serde(rename = "created_at")]
    created_at: String,
}

// ---------------------------------------------------------------------------
// Auth models
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct TokenResponse {
    access: String,
    refresh: String,
}

#[derive(Deserialize)]
struct RefreshResponse {
    access: String,
}

// ---------------------------------------------------------------------------
// Chapter detail model (returned by GET chapters/{id}/)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ChapterDetail {
    id: i64,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    chapter: Option<String>,
    #[serde(default)]
    pages: Vec<ChapterPage>,
}

#[derive(Deserialize)]
struct ChapterPage {
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    page_number: Option<i64>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ensure_absolute_url(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else if url.starts_with('/') {
        format!("https://appswat.com{}", url)
    } else {
        format!("https://appswat.com/{}", url)
    }
}

// ---------------------------------------------------------------------------
// Source implementation
// ---------------------------------------------------------------------------

struct MeshManga;

/// Obtain a valid access token. Tries (in order):
/// 1. Cached access token from defaults
/// 2. Refresh via cached refresh token
/// 3. Fresh login with username/password from settings
fn get_access_token() -> Option<String> {
    // 1) Try cached access token
    if let Some(token) = defaults_get::<String>("meshmanga.access_token") {
        if !token.is_empty() {
            // Quick check: try to use it. If it fails we'll fall through to refresh/login.
            return Some(token);
        }
    }

    // 2) Try refresh
    if let Some(refresh) = defaults_get::<String>("meshmanga.refresh_token") {
        if !refresh.is_empty() {
            if let Some(token) = try_refresh_token(&refresh) {
                return Some(token);
            }
        }
    }

    // 3) Fresh login
    try_login()
}

/// Attempt to refresh the access token using the stored refresh token.
fn try_refresh_token(refresh: &str) -> Option<String> {
    let body = format!("{{\"refresh\":\"{}\"}}", refresh);
    let url = format!("{}/token/refresh/", API_URL);
    let req = match Request::post(&url) {
        Ok(r) => r,
        Err(_) => return None,
    };
    let resp = req
        .header("Content-Type", "application/json")
        .body(body.as_bytes())
        .send();

    if let Ok(mut resp) = resp {
        if let Ok(data) = resp.get_json::<RefreshResponse>() {
            defaults_set(
                "meshmanga.access_token",
                DefaultValue::String(data.access.clone()),
            );
            return Some(data.access);
        }
    }

    // Refresh failed — clear stale tokens so next call does a fresh login
    defaults_set("meshmanga.access_token", DefaultValue::Null);
    defaults_set("meshmanga.refresh_token", DefaultValue::Null);
    None
}

/// Perform a fresh login using credentials from source settings.
fn try_login() -> Option<String> {
    let username: String = defaults_get("username").unwrap_or_default();
    let password: String = defaults_get("password").unwrap_or_default();

    if username.is_empty() || password.is_empty() {
        return None;
    }

    let body = format!(
        "{{\"username\":\"{}\",\"password\":\"{}\"}}",
        username, password
    );
    let url = format!("{}/token/", API_URL);
    let req = match Request::post(&url) {
        Ok(r) => r,
        Err(_) => return None,
    };
    let resp = req
        .header("Content-Type", "application/json")
        .body(body.as_bytes())
        .send();

    if let Ok(mut resp) = resp {
        if let Ok(data) = resp.get_json::<TokenResponse>() {
            defaults_set(
                "meshmanga.access_token",
                DefaultValue::String(data.access.clone()),
            );
            defaults_set(
                "meshmanga.refresh_token",
                DefaultValue::String(data.refresh.clone()),
            );
            return Some(data.access);
        }
    }

    None
}

/// Fetch chapter pages from the authenticated API.
/// If the cached token is rejected (401), retry once after refreshing/re-logging in.
fn fetch_chapter_pages(chapter_id: &str) -> Option<Vec<ChapterPage>> {
    // First attempt
    if let Some(token) = get_access_token() {
        let url = format!("{}/chapters/{}/", API_URL, chapter_id);
        let resp = match Request::get(&url) {
            Ok(r) => r.header("Authorization", &format!("Bearer {}", token)).send(),
            Err(_) => return None,
        };

        if let Ok(mut resp) = resp {
            if let Ok(detail) = resp.get_json::<ChapterDetail>() {
                return Some(detail.pages);
            }
            // If the request came back but deserialization failed or we got an error,
            // the token might be expired. Clear it and retry.
        }

        // Clear cached token and retry with a fresh one
        defaults_set("meshmanga.access_token", DefaultValue::Null);
    }

    // Second attempt: force re-auth
    // Try refresh first
    if let Some(refresh) = defaults_get::<String>("meshmanga.refresh_token") {
        if !refresh.is_empty() {
            if let Some(new_token) = try_refresh_token(&refresh) {
                let url = format!("{}/chapters/{}/", API_URL, chapter_id);
                let req = match Request::get(&url) {
                    Ok(r) => r,
                    Err(_) => return None,
                };
                if let Ok(mut resp) = req
                    .header("Authorization", &format!("Bearer {}", new_token))
                    .send()
                {
                    if let Ok(detail) = resp.get_json::<ChapterDetail>() {
                        return Some(detail.pages);
                    }
                }
            }
        }
    }

    // Last resort: full re-login
    if let Some(new_token) = try_login() {
        let url = format!("{}/chapters/{}/", API_URL, chapter_id);
        let req = match Request::get(&url) {
            Ok(r) => r,
            Err(_) => return None,
        };
        if let Ok(mut resp) = req
            .header("Authorization", &format!("Bearer {}", new_token))
            .send()
        {
            if let Ok(detail) = resp.get_json::<ChapterDetail>() {
                return Some(detail.pages);
            }
        }
    }

    None
}

fn series_to_manga(series: &SeriesData) -> Manga {
    let status = series
        .status
        .as_ref()
        .map(|s| {
            let name = s.name.to_lowercase();
            if name.contains("ongoing") || name.contains("مستمرة") {
                MangaStatus::Ongoing
            } else if name.contains("completed") || name.contains("مكتملة") {
                MangaStatus::Completed
            } else if name.contains("hiatus") || name.contains("متوقفة") {
                MangaStatus::Hiatus
            } else {
                MangaStatus::Unknown
            }
        })
        .unwrap_or(MangaStatus::Unknown);

    let viewer = series
        .series_type
        .as_ref()
        .map(|t| {
            let name = t.name.to_lowercase();
            if name.contains("manhwa") || name.contains("webtoon") || name.contains("manhua") {
                Viewer::Webtoon
            } else {
                Viewer::RightToLeft
            }
        })
        .unwrap_or(Viewer::RightToLeft);

    let tags: Vec<String> = series
        .genres
        .iter()
        .filter_map(|g| {
            let name = &g.name;
            if name.is_empty() {
                None
            } else {
                Some(name.clone())
            }
        })
        .collect();

    let cover = series
        .poster
        .as_ref()
        .and_then(|p| p.thumbnail.as_ref())
        .map(|s| ensure_absolute_url(s));
    let description = series.story.clone();

    let authors: Vec<String> = series
        .author
        .as_ref()
        .and_then(|a| a.name.clone())
        .map(|n| {
            if n.is_empty() {
                vec![]
            } else {
                vec![n.clone()]
            }
        })
        .unwrap_or_default();

    Manga {
        key: series.id.to_string(),
        title: series.title.clone(),
        cover,
        description,
        status,
        viewer,
        tags: if tags.is_empty() { None } else { Some(tags) },
        authors: if authors.is_empty() {
            None
        } else {
            Some(authors)
        },
        url: Some(format!("{}/series/{}", BASE_URL, series.slug)),
        ..Default::default()
    }
}

impl Source for MeshManga {
    fn new() -> Self {
        MeshManga
    }

    fn get_search_manga_list(
        &self,
        query: Option<String>,
        page: i32,
        _filters: Vec<FilterValue>,
    ) -> Result<MangaPageResult> {
        let search_term = query.unwrap_or_default();
        let url = if search_term.is_empty() {
            format!("{}/series/?page={}&page_size={}", API_URL, page, PAGE_SIZE)
        } else {
            format!(
                "{}/series/?search={}&page={}&page_size={}",
                API_URL,
                encode_uri_component(&search_term),
                page,
                PAGE_SIZE
            )
        };

        let resp: ApiResponse<SeriesData> = Request::get(&url)?.json_owned()?;

        let entries: Vec<Manga> = resp.results.iter().map(series_to_manga).collect();
        let has_next_page = resp.next.is_some();

        Ok(MangaPageResult {
            entries,
            has_next_page,
        })
    }

    fn get_manga_update(
        &self,
        mut manga: Manga,
        needs_details: bool,
        needs_chapters: bool,
    ) -> Result<Manga> {
        let series_id: i64 = manga.key.parse().unwrap_or(0);

        if needs_details {
            let detail_url = format!("{}/series/{}/", API_URL, series_id);
            let resp: SeriesData = Request::get(&detail_url)?.json_owned()?;

            manga.title = resp.title.clone();
            manga.description = resp.story.clone();
            manga.cover = resp
                .poster
                .as_ref()
                .and_then(|p| p.thumbnail.as_ref())
                .map(|s| ensure_absolute_url(s));

            manga.status = resp
                .status
                .as_ref()
                .map(|s| {
                    let name = s.name.to_lowercase();
                    if name.contains("ongoing") || name.contains("مستمرة") {
                        MangaStatus::Ongoing
                    } else if name.contains("completed") || name.contains("مكتملة") {
                        MangaStatus::Completed
                    } else if name.contains("hiatus") || name.contains("متوقفة") {
                        MangaStatus::Hiatus
                    } else {
                        MangaStatus::Unknown
                    }
                })
                .unwrap_or(MangaStatus::Unknown);

            manga.viewer = resp
                .series_type
                .as_ref()
                .map(|t| {
                    let name = t.name.to_lowercase();
                    if name.contains("manhwa")
                        || name.contains("webtoon")
                        || name.contains("manhua")
                    {
                        Viewer::Webtoon
                    } else {
                        Viewer::RightToLeft
                    }
                })
                .unwrap_or(Viewer::RightToLeft);

            let tags: Vec<String> = resp
                .genres
                .iter()
                .filter_map(|g| {
                    let name = &g.name;
                    if name.is_empty() {
                        None
                    } else {
                        Some(name.clone())
                    }
                })
                .collect();
            if !tags.is_empty() {
                manga.tags = Some(tags);
            }

            manga.url = Some(format!("{}/series/{}", BASE_URL, resp.slug));
        }

        if needs_chapters {
            let mut all_chapters: Vec<Chapter> = Vec::new();
            let mut current_page = 1;
            let mut has_more = true;

            while has_more {
                let url = format!(
                    "{}/series/{}/chapters/?page={}&page_size=50&order_by=-order",
                    API_URL, series_id, current_page
                );
                let resp: ApiResponse<ChapterListItem> = Request::get(&url)?.json_owned()?;

                for ch in resp.results {
                    let date = parse_date(&ch.created_at, "yyyy-MM-dd'T'HH:mm:ss");

                    let chapter_number = parse_chapter_number(&ch.chapter);

                    all_chapters.push(Chapter {
                        key: ch.id.to_string(),
                        title: Some(ch.title),
                        chapter_number,
                        date_uploaded: date,
                        url: Some(format!("{}/chapter/{}/", BASE_URL, ch.id)),
                        language: Some(String::from("ar")),
                        ..Default::default()
                    });
                }

                has_more = resp.next.is_some();
                current_page += 1;
            }

            manga.chapters = Some(all_chapters);
        }

        Ok(manga)
    }

    fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
        let chapter_id = &chapter.key;

        // Fetch chapter pages via the authenticated API
        if let Some(chapter_pages) = fetch_chapter_pages(chapter_id) {
            let mut pages: Vec<Page> = Vec::new();

            for cp in chapter_pages {
                if let Some(ref img_url) = cp.image {
                    let url_str = img_url.trim();
                    if !url_str.is_empty() {
                        pages.push(Page {
                            content: PageContent::Url(ensure_absolute_url(url_str), None),
                            ..Default::default()
                        });
                    }
                }
            }

            if !pages.is_empty() {
                return Ok(pages);
            }
        }

        // If API failed (no credentials / auth error), return an empty list.
        // The user needs to configure their login credentials in source settings.
        Ok(Vec::new())
    }
}

impl ListingProvider for MeshManga {
    fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
        let url = match listing.id.as_str() {
            "Popular" => {
                format!(
                    "{}/series/?ordering=-views_count&page={}&page_size={}",
                    API_URL, page, PAGE_SIZE
                )
            }
            _ => {
                format!(
                    "{}/series/?ordering=-created_at&page={}&page_size={}",
                    API_URL, page, PAGE_SIZE
                )
            }
        };
        let resp: ApiResponse<SeriesData> = Request::get(&url)?.json_owned()?;

        let entries: Vec<Manga> = resp.results.iter().map(series_to_manga).collect();
        let has_next_page = resp.next.is_some();

        Ok(MangaPageResult {
            entries,
            has_next_page,
        })
    }
}

impl DeepLinkHandler for MeshManga {
    fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
        let series_slug = url
            .strip_prefix("https://meshmanga.com/series/")
            .or_else(|| url.strip_prefix("http://meshmanga.com/series/"))
            .unwrap_or("")
            .split('/')
            .next()
            .unwrap_or("")
            .trim_matches('/');

        if series_slug.is_empty() {
            return Ok(None);
        }

        let url = format!(
            "{}/series/?search={}",
            API_URL,
            encode_uri_component(series_slug)
        );
        let resp: ApiResponse<SeriesData> = Request::get(&url)?.json_owned()?;

        if let Some(series) = resp.results.into_iter().find(|s| s.slug == series_slug) {
            return Ok(Some(DeepLinkResult::Manga {
                key: series.id.to_string(),
            }));
        }

        Ok(None)
    }
}

fn parse_chapter_number(chapter_str: &str) -> Option<f32> {
    let parts: Vec<&str> = chapter_str.split_whitespace().collect();
    for part in parts {
        if let Ok(val) = part.parse::<f32>() {
            return Some(val);
        }
    }
    let parts: Vec<&str> = chapter_str.split('-').collect();
    let mut number = 0.0_f32;
    let mut found = false;
    for part in parts {
        if let Ok(val) = part.parse::<f32>() {
            if found {
                number += val / 10.0;
                break;
            } else {
                number = val;
                found = true;
            }
        }
    }
    if found {
        Some(number)
    } else {
        Some(0.0)
    }
}

register_source!(MeshManga, ListingProvider, DeepLinkHandler);
