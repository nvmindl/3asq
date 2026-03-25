#![no_std]

use aidoku::{
    alloc::{String, Vec},
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
const MEDIA_URL: &str = "https://appswat.com/v2/media/series";
const BASE_URL: &str = "https://meshmanga.com";
const PAGE_SIZE: usize = 20;
const MAX_PAGES: i32 = 300;

// ---------------------------------------------------------------------------
// API data models
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ApiResponse<T> {
    #[allow(dead_code)]
    count: i64,
    next: Option<String>,
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    #[serde(rename = "chapters_count")]
    chapters_count: Option<i64>,
    #[serde(default)]
    author: Option<AuthorData>,
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
    #[allow(dead_code)]
    slug: String,
    chapter: String,
    #[allow(dead_code)]
    serie: i64,
    #[serde(rename = "created_at")]
    created_at: String,
}

// ---------------------------------------------------------------------------
// Auth models (for fallback)
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

#[derive(Deserialize)]
struct ChapterDetail {
    #[allow(dead_code)]
    id: i64,
    #[serde(default)]
    pages: Vec<ChapterPage>,
}

#[derive(Deserialize)]
struct ChapterPage {
    #[serde(default)]
    image: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the media folder name from the poster URL.
/// Newer series have poster URLs like .../series/{folder_name}/poster*
fn extract_media_folder(series: &SeriesData) -> Option<String> {
    let poster = series.poster.as_ref()?;
    let url = poster.thumbnail.as_ref().or(poster.medium.as_ref())?;
    let after = url.split("/series/").nth(1)?;
    let folder = after.split('/').next()?;
    if folder.is_empty() {
        None
    } else {
        Some(String::from(folder))
    }
}

/// Parse the integer chapter number from strings like "26", "10 Free", "267 FREE".
fn parse_chapter_int(chapter_str: &str) -> i32 {
    let mut num = String::new();
    let mut found = false;
    for c in chapter_str.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            found = true;
        } else if found {
            break;
        }
    }
    num.parse().unwrap_or(0)
}

/// Check if a URL returns HTTP 200 using a GET request.
fn url_exists(url: &str) -> bool {
    match Request::get(url) {
        Ok(req) => match req.send() {
            Ok(resp) => resp.status_code() == 200,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// Find the maximum page number by scanning sequentially.
/// Starts from page 1 and increments until a 404 is hit.
fn find_max_page(base_url: &str) -> i32 {
    let mut page = 1_i32;
    while page <= MAX_PAGES {
        let url = format!("{}/{:04}.webp", base_url, page);
        if !url_exists(&url) {
            break;
        }
        page += 1;
    }
    page - 1
}

fn ensure_absolute_url(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        String::from(url)
    } else if url.starts_with('/') {
        format!("https://appswat.com{}", url)
    } else {
        format!("https://appswat.com/{}", url)
    }
}

fn parse_status(name: &str) -> MangaStatus {
    let lower = name.to_lowercase();
    if lower.contains("ongoing") || lower.contains("\u{0645}\u{0633}\u{062a}\u{0645}\u{0631}\u{0629}") {
        MangaStatus::Ongoing
    } else if lower.contains("completed") || lower.contains("\u{0645}\u{0643}\u{062a}\u{0645}\u{0644}\u{0629}") {
        MangaStatus::Completed
    } else if lower.contains("hiatus") || lower.contains("\u{0645}\u{062a}\u{0648}\u{0642}\u{0641}\u{0629}") {
        MangaStatus::Hiatus
    } else {
        MangaStatus::Unknown
    }
}

fn parse_viewer(type_name: &str) -> Viewer {
    let lower = type_name.to_lowercase();
    if lower.contains("manhwa") || lower.contains("webtoon") || lower.contains("manhua") {
        Viewer::Webtoon
    } else {
        Viewer::RightToLeft
    }
}

fn parse_chapter_number(chapter_str: &str) -> Option<f32> {
    for part in chapter_str.split_whitespace() {
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
    if found { Some(number) } else { Some(0.0) }
}

fn series_to_manga(series: &SeriesData) -> Manga {
    let status = series
        .status
        .as_ref()
        .map(|s| parse_status(&s.name))
        .unwrap_or(MangaStatus::Unknown);

    let viewer = series
        .series_type
        .as_ref()
        .map(|t| parse_viewer(&t.name))
        .unwrap_or(Viewer::RightToLeft);

    let tags: Vec<String> = series
        .genres
        .iter()
        .filter(|g| !g.name.is_empty())
        .map(|g| g.name.clone())
        .collect();

    let cover = series
        .poster
        .as_ref()
        .and_then(|p| p.thumbnail.as_ref())
        .map(|s| ensure_absolute_url(s));

    let authors: Vec<String> = series
        .author
        .as_ref()
        .and_then(|a| a.name.clone())
        .filter(|n| !n.is_empty())
        .into_iter()
        .collect();

    Manga {
        key: format!("{}", series.id),
        title: series.title.clone(),
        cover,
        description: series.story.clone(),
        status,
        viewer,
        tags: if tags.is_empty() { None } else { Some(tags) },
        authors: if authors.is_empty() { None } else { Some(authors) },
        url: Some(format!("{}/series/{}", BASE_URL, series.slug)),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Auth helpers (fallback for older series without direct media URLs)
// ---------------------------------------------------------------------------

fn get_access_token() -> Option<String> {
    if let Some(token) = defaults_get::<String>("meshmanga.access_token") {
        if !token.is_empty() {
            return Some(token);
        }
    }
    if let Some(refresh) = defaults_get::<String>("meshmanga.refresh_token") {
        if !refresh.is_empty() {
            if let Some(token) = try_refresh_token(&refresh) {
                return Some(token);
            }
        }
    }
    try_login()
}

fn try_refresh_token(refresh: &str) -> Option<String> {
    let body = format!("{{\"refresh\":\"{}\"}}", refresh);
    let url = format!("{}/token/refresh/", API_URL);
    let req = Request::post(&url).ok()?;
    let mut resp = req
        .header("Content-Type", "application/json")
        .body(body.as_bytes())
        .send()
        .ok()?;
    let data = resp.get_json::<RefreshResponse>().ok()?;
    defaults_set("meshmanga.access_token", DefaultValue::String(data.access.clone()));
    Some(data.access)
}

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
    let req = Request::post(&url).ok()?;
    let mut resp = req
        .header("Content-Type", "application/json")
        .body(body.as_bytes())
        .send()
        .ok()?;
    let data = resp.get_json::<TokenResponse>().ok()?;
    defaults_set("meshmanga.access_token", DefaultValue::String(data.access.clone()));
    defaults_set("meshmanga.refresh_token", DefaultValue::String(data.refresh.clone()));
    Some(data.access)
}

fn fetch_chapter_pages_auth(chapter_id: &str) -> Option<Vec<Page>> {
    let token = get_access_token()?;
    let url = format!("{}/chapters/{}/", API_URL, chapter_id);
    let resp = Request::get(&url)
        .ok()?
        .header("Authorization", &format!("Bearer {}", token))
        .send();

    if let Ok(mut resp) = resp {
        if let Ok(detail) = resp.get_json::<ChapterDetail>() {
            let pages: Vec<Page> = detail
                .pages
                .iter()
                .filter_map(|cp| {
                    let url_str = cp.image.as_ref()?.trim();
                    if url_str.is_empty() { return None; }
                    Some(Page {
                        content: PageContent::Url(ensure_absolute_url(url_str), None),
                        ..Default::default()
                    })
                })
                .collect();
            if !pages.is_empty() {
                return Some(pages);
            }
        }
    }

    // Token might be stale, clear and retry once
    defaults_set("meshmanga.access_token", DefaultValue::Null);
    let token = get_access_token()?;
    let url = format!("{}/chapters/{}/", API_URL, chapter_id);
    let mut resp = Request::get(&url)
        .ok()?
        .header("Authorization", &format!("Bearer {}", token))
        .send()
        .ok()?;
    let detail = resp.get_json::<ChapterDetail>().ok()?;
    let pages: Vec<Page> = detail
        .pages
        .iter()
        .filter_map(|cp| {
            let url_str = cp.image.as_ref()?.trim();
            if url_str.is_empty() { return None; }
            Some(Page {
                content: PageContent::Url(ensure_absolute_url(url_str), None),
                ..Default::default()
            })
        })
        .collect();
    if pages.is_empty() { None } else { Some(pages) }
}

// ---------------------------------------------------------------------------
// Source implementation
// ---------------------------------------------------------------------------

struct MeshManga;

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

        Ok(MangaPageResult { entries, has_next_page })
    }

    fn get_manga_update(
        &self,
        mut manga: Manga,
        needs_details: bool,
        needs_chapters: bool,
    ) -> Result<Manga> {
        let series_id: i64 = manga.key.parse().unwrap_or(0);

        // Fetch series detail when needed (for details or to get poster URL for chapters)
        let series_resp: Option<SeriesData> = if needs_details || needs_chapters {
            let url = format!("{}/series/{}/", API_URL, series_id);
            Some(Request::get(&url)?.json_owned()?)
        } else {
            None
        };

        if needs_details {
            if let Some(ref resp) = series_resp {
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
                    .map(|s| parse_status(&s.name))
                    .unwrap_or(MangaStatus::Unknown);

                manga.viewer = resp
                    .series_type
                    .as_ref()
                    .map(|t| parse_viewer(&t.name))
                    .unwrap_or(Viewer::RightToLeft);

                let tags: Vec<String> = resp
                    .genres
                    .iter()
                    .filter(|g| !g.name.is_empty())
                    .map(|g| g.name.clone())
                    .collect();
                if !tags.is_empty() {
                    manga.tags = Some(tags);
                }

                manga.url = Some(format!("{}/series/{}", BASE_URL, resp.slug));
            }
        }

        if needs_chapters {
            // Extract media folder from poster URL for direct image access
            let media_folder = series_resp.as_ref().and_then(extract_media_folder);

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

                    // Build chapter URL: direct media path if available, else fallback
                    let ch_url = if let Some(ref folder) = media_folder {
                        let ch_num = parse_chapter_int(&ch.chapter);
                        format!("{}/{}/chapters/{:04}", MEDIA_URL, folder, ch_num)
                    } else {
                        format!("{}/chapter/{}/", BASE_URL, ch.id)
                    };

                    all_chapters.push(Chapter {
                        key: format!("{}", ch.id),
                        title: Some(ch.title),
                        chapter_number,
                        date_uploaded: date,
                        url: Some(ch_url),
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
        let chapter_url = chapter.url.as_deref().unwrap_or("");

        // Method 1: Direct image URLs (for series with known media folder)
        if chapter_url.starts_with(MEDIA_URL) {
            let max = find_max_page(chapter_url);
            if max > 0 {
                let mut pages = Vec::new();
                for i in 1..=max {
                    pages.push(Page {
                        content: PageContent::Url(
                            format!("{}/{:04}.webp", chapter_url, i),
                            None,
                        ),
                        ..Default::default()
                    });
                }
                return Ok(pages);
            }
        }

        // Method 2: Authenticated API fallback
        if let Some(pages) = fetch_chapter_pages_auth(&chapter.key) {
            return Ok(pages);
        }

        Ok(Vec::new())
    }
}

impl ListingProvider for MeshManga {
    fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
        let url = match listing.id.as_str() {
            "Popular" => format!(
                "{}/series/?ordering=-views_count&page={}&page_size={}",
                API_URL, page, PAGE_SIZE
            ),
            _ => format!(
                "{}/series/?ordering=-created_at&page={}&page_size={}",
                API_URL, page, PAGE_SIZE
            ),
        };
        let resp: ApiResponse<SeriesData> = Request::get(&url)?.json_owned()?;
        let entries: Vec<Manga> = resp.results.iter().map(series_to_manga).collect();
        let has_next_page = resp.next.is_some();

        Ok(MangaPageResult { entries, has_next_page })
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

        let api_url = format!(
            "{}/series/?search={}",
            API_URL,
            encode_uri_component(series_slug)
        );
        let resp: ApiResponse<SeriesData> = Request::get(&api_url)?.json_owned()?;

        if let Some(series) = resp.results.into_iter().find(|s| s.slug == series_slug) {
            return Ok(Some(DeepLinkResult::Manga {
                key: format!("{}", series.id),
            }));
        }

        Ok(None)
    }
}

register_source!(MeshManga, ListingProvider, DeepLinkHandler);
