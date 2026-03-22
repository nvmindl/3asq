#![no_std]

use aidoku::{
    alloc::{string::ToString, String, Vec},
    imports::{net::Request, std::parse_date},
    prelude::*,
    Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Listing, ListingProvider, Manga,
    MangaPageResult, MangaStatus, Page, PageContent, Result, Source, Viewer,
};
use serde::Deserialize;

const API_URL: &str = "https://api.swatmanga.com";
const BASE_URL: &str = "https://meshmanga.com";
const PAGE_SIZE: usize = 20;

#[derive(Deserialize)]
struct ApiResponse<T> {
    data: ApiData<T>,
}

#[derive(Deserialize)]
struct ApiData<T> {
    results: Vec<T>,
    #[serde(rename = "next")]
    next_page: Option<usize>,
}

#[derive(Deserialize)]
struct SeriesResponse {
    data: SeriesData,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SeriesData {
    id: i64,
    title: String,
    story: Option<String>,
    poster: Option<Poster>,
    #[serde(rename = "alternative-titles")]
    alternative_titles: Option<Vec<String>>,
    #[serde(rename = "type")]
    series_type: Option<TypeData>,
    status: Option<StatusData>,
    author: Option<AuthorData>,
    artist: Option<AuthorData>,
    genres: Option<Vec<GenreData>>,
    #[serde(rename = "chapters_count")]
    chapters_count: i64,
}

#[derive(Deserialize)]
struct Poster {
    thumbnail: Option<String>,
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
#[allow(dead_code)]
struct AuthorData {
    name: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct GenreData {
    id: i64,
    name: String,
}

#[derive(Deserialize)]
struct ChaptersResponse {
    data: ChaptersData,
}

#[derive(Deserialize)]
struct ChaptersData {
    results: Vec<ChapterData>,
    #[serde(rename = "next")]
    next_page: Option<usize>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ChapterData {
    id: i64,
    chapter: String,
    title: Option<String>,
    #[serde(rename = "created_at")]
    created_at: String,
    #[serde(rename = "views_count")]
    views_count: i64,
    #[serde(rename = "images_count")]
    images_count: i64,
}

#[derive(Deserialize)]
struct ChapterPagesResponse {
    data: ChapterPagesData,
}

#[derive(Deserialize)]
struct ChapterPagesData {
    images: Vec<ImageData>,
}

#[derive(Deserialize)]
struct ImageData {
    url: String,
}

struct MeshManga;

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
            } else if name.contains("cancelled") || name.contains("ملغية") {
                MangaStatus::Cancelled
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
            if name.contains("manhwa") || name.contains("webtoon") {
                Viewer::Webtoon
            } else {
                Viewer::RightToLeft
            }
        })
        .unwrap_or(Viewer::RightToLeft);

    let tags: Vec<String> = series
        .genres
        .as_ref()
        .map(|genres| {
            genres
                .iter()
                .map(|g| g.name.clone())
                .filter(|n| !n.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let cover = series.poster.as_ref().and_then(|p| p.thumbnail.clone());
    let description = series.story.clone();

    Manga {
        key: series.id.to_string(),
        title: series.title.clone(),
        cover,
        description,
        status,
        viewer,
        tags: if tags.is_empty() { None } else { Some(tags) },
        url: Some(format!("{}/series/{}", BASE_URL, series.id)),
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
        let url = format!(
            "{}/series/?search={}&page={}&page_size={}",
            API_URL,
            urlencoding_encode(&search_term),
            page,
            PAGE_SIZE
        );
        let resp: ApiResponse<SeriesData> = Request::get(&url)?.json_owned()?;

        let entries: Vec<Manga> = resp.data.results.iter().map(series_to_manga).collect();
        let has_next_page = resp.data.next_page.is_some();

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
            let resp: SeriesResponse = Request::get(&detail_url)?.json_owned()?;
            let series = &resp.data;

            manga.title = series.title.clone();
            manga.description = series.story.clone();
            manga.cover = series.poster.as_ref().and_then(|p| p.thumbnail.clone());

            manga.status = series
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
                    } else if name.contains("cancelled") || name.contains("ملغية") {
                        MangaStatus::Cancelled
                    } else {
                        MangaStatus::Unknown
                    }
                })
                .unwrap_or(MangaStatus::Unknown);

            manga.viewer = series
                .series_type
                .as_ref()
                .map(|t| {
                    let name = t.name.to_lowercase();
                    if name.contains("manhwa") || name.contains("webtoon") {
                        Viewer::Webtoon
                    } else {
                        Viewer::RightToLeft
                    }
                })
                .unwrap_or(Viewer::RightToLeft);

            let tags: Vec<String> = series
                .genres
                .as_ref()
                .map(|genres| {
                    genres
                        .iter()
                        .map(|g| g.name.clone())
                        .filter(|n| !n.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            if !tags.is_empty() {
                manga.tags = Some(tags);
            }

            manga.url = Some(format!("{}/series/{}", BASE_URL, series_id));
        }

        if needs_chapters {
            let mut all_chapters: Vec<Chapter> = Vec::new();
            let mut current_page = 1;
            let mut has_more = true;

            while has_more {
                let url = format!(
                    "{}/chapters/?serie={}&page={}&page_size=50&order_by=-order",
                    API_URL, series_id, current_page
                );
                let resp: ChaptersResponse = Request::get(&url)?.json_owned()?;

                for ch in resp.data.results {
                    let date = parse_date(&ch.created_at, "yyyy-MM-dd'T'HH:mm:ss");

                    all_chapters.push(Chapter {
                        key: ch.id.to_string(),
                        title: ch.title.filter(|s| !s.is_empty()),
                        chapter_number: parse_chapter_number(&ch.chapter),
                        date_uploaded: date,
                        url: Some(format!("{}/chapter/{}", BASE_URL, ch.id)),
                        language: Some(String::from("ar")),
                        ..Default::default()
                    });
                }

                has_more = resp.data.next_page.is_some();
                current_page += 1;
            }

            manga.chapters = Some(all_chapters);
        }

        Ok(manga)
    }

    fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
        let chapter_id: i64 = chapter.key.parse().unwrap_or(0);
        let url = format!("{}/chapters/{}/", API_URL, chapter_id);
        let resp: ChapterPagesResponse = Request::get(&url)?.json_owned()?;

        let pages: Vec<Page> = resp
            .data
            .images
            .into_iter()
            .filter(|img| !img.url.is_empty())
            .map(|img| Page {
                content: PageContent::url(img.url),
                ..Default::default()
            })
            .collect();

        Ok(pages)
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

        let entries: Vec<Manga> = resp.data.results.iter().map(series_to_manga).collect();
        let has_next_page = resp.data.next_page.is_some();

        Ok(MangaPageResult {
            entries,
            has_next_page,
        })
    }
}

impl DeepLinkHandler for MeshManga {
    fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
        let series_id = url
            .strip_prefix("https://meshmanga.com/series/")
            .or_else(|| url.strip_prefix("http://meshmanga.com/series/"))
            .unwrap_or("")
            .split('/')
            .next()
            .unwrap_or("")
            .trim_matches('/');

        if series_id.is_empty() {
            return Ok(None);
        }

        if series_id.parse::<i64>().is_ok() {
            return Ok(Some(DeepLinkResult::Manga {
                key: String::from(series_id),
            }));
        }

        Ok(None)
    }
}

fn parse_chapter_number(chapter_str: &str) -> Option<f32> {
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

fn urlencoding_encode(input: &str) -> String {
    let mut encoded = String::new();
    for c in input.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                encoded.push(c);
            }
            ' ' => encoded.push_str("%20"),
            _ => {
                for byte in c.to_string().as_bytes() {
                    encoded.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    encoded
}

register_source!(MeshManga, ListingProvider, DeepLinkHandler);
