#![no_std]

use aidoku::{
    alloc::{String, Vec},
    helpers::uri::encode_uri_component,
    imports::{
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
const USER_AGENT: &str = "ktor-client";

// ---------------------------------------------------------------------------
// API data models
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ApiResponse<T> {
    #[allow(dead_code)]
    count: i64,
    next: Option<String>,
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
    #[serde(default)]
    author: Option<AuthorData>,
}

#[derive(Deserialize)]
struct Poster {
    thumbnail: Option<String>,
    #[allow(dead_code)]
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
    chapter: String,
    #[serde(rename = "created_at")]
    created_at: String,
}

#[derive(Deserialize)]
struct ChapterDetail {
    #[serde(default)]
    images: Vec<ChapterImage>,
}

#[derive(Deserialize)]
struct ChapterImage {
    image: String,
    #[allow(dead_code)]
    order: i64,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

        let resp: ApiResponse<SeriesData> = Request::get(&url)?
            .header("User-Agent", USER_AGENT)
            .json_owned()?;
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

        if needs_details {
            let url = format!("{}/series/{}/", API_URL, series_id);
            let resp: SeriesData = Request::get(&url)?
                .header("User-Agent", USER_AGENT)
                .json_owned()?;

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

        if needs_chapters {
            let mut all_chapters: Vec<Chapter> = Vec::new();
            let mut current_page = 1;
            let mut has_more = true;

            while has_more {
                let url = format!(
                    "{}/series/{}/chapters/?page={}&page_size=50&order_by=-order",
                    API_URL, series_id, current_page
                );
                let resp: ApiResponse<ChapterListItem> = Request::get(&url)?
                    .header("User-Agent", USER_AGENT)
                    .json_owned()?;

                for ch in resp.results {
                    let date = parse_date(&ch.created_at, "yyyy-MM-dd'T'HH:mm:ss");
                    let chapter_number = parse_chapter_number(&ch.chapter);

                    all_chapters.push(Chapter {
                        key: format!("{}", ch.id),
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

        // Fetch chapter detail — ktor-client User-Agent bypasses authentication
        let url = format!("{}/chapters/{}/", API_URL, chapter_id);
        let detail: ChapterDetail = Request::get(&url)?
            .header("User-Agent", USER_AGENT)
            .json_owned()?;

        let mut pages: Vec<Page> = Vec::new();
        for img in detail.images {
            let url_str = img.image.trim();
            if !url_str.is_empty() {
                pages.push(Page {
                    content: PageContent::Url(ensure_absolute_url(url_str), None),
                    ..Default::default()
                });
            }
        }

        Ok(pages)
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
        let resp: ApiResponse<SeriesData> = Request::get(&url)?
            .header("User-Agent", USER_AGENT)
            .json_owned()?;
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
        let resp: ApiResponse<SeriesData> = Request::get(&api_url)?
            .header("User-Agent", USER_AGENT)
            .json_owned()?;

        if let Some(series) = resp.results.into_iter().find(|s| s.slug == series_slug) {
            return Ok(Some(DeepLinkResult::Manga {
                key: format!("{}", series.id),
            }));
        }

        Ok(None)
    }
}

register_source!(MeshManga, ListingProvider, DeepLinkHandler);
