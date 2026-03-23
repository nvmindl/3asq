#![no_std]

use aidoku::{
    alloc::{string::ToString, vec, String, Vec},
    helpers::uri::encode_uri_component,
    imports::{net::Request, std::parse_date},
    prelude::*,
    Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Listing, ListingProvider, Manga,
    MangaPageResult, MangaStatus, Page, PageContent, Result, Source, Viewer,
};
use serde::Deserialize;

const API_URL: &str = "https://appswat.com/v2/api/v2";
const BASE_URL: &str = "https://meshmanga.com";
const PAGE_SIZE: usize = 20;

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

    let cover = series.poster.as_ref().and_then(|p| p.thumbnail.clone());
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
            manga.cover = resp.poster.as_ref().and_then(|p| p.thumbnail.clone());

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

            manga.url = Some(format!("{}/series/{}", BASE_URL, series_id));
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
                        // MeshManga chapter pages are under `/chapter/{id}/` (plural `/chapters/` is 404).
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
        let url = if let Some(ref ch_url) = chapter.url {
            ch_url.clone()
        } else {
            // Prefer the public chapter page over the authenticated API.
            format!("{}/chapter/{}/", BASE_URL, chapter.key)
        };

        let html = Request::get(&url)?.html()?;

        let mut pages: Vec<Page> = Vec::new();

        if let Some(imgs) = html.select("div.chapter-content img") {
            for img in imgs {
                if let Some(img_url) = img.attr("data-src").or_else(|| img.attr("src")) {
                    let url_str = img_url.trim();
                    if !url_str.is_empty() && !url_str.contains("data:image") {
                        pages.push(Page {
                            content: PageContent::Url(url_str.to_string(), None),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        if pages.is_empty() {
            if let Some(imgs) = html.select("div.page-break img") {
                for img in imgs {
                    if let Some(img_url) = img.attr("data-src").or_else(|| img.attr("src")) {
                        let url_str = img_url.trim();
                        if !url_str.is_empty() && !url_str.contains("data:image") {
                            pages.push(Page {
                                content: PageContent::Url(url_str.to_string(), None),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }

        // MeshManga chapter HTML is a Next.js shell; images are typically loaded client-side.
        // As a fallback, return the chapter URL itself so the reader can at least render something.
        if pages.is_empty() {
            pages.push(Page {
                content: PageContent::Url(url.clone(), None),
                ..Default::default()
            });
        }

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
