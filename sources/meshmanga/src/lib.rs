#![no_std]

use aidoku::{
    alloc::String,
    alloc::Vec,
    helpers::uri::encode_uri_component,
    imports::net::Request,
    prelude::*,
    Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Listing, ListingProvider,
    Manga, MangaPageResult, MangaStatus, Page, PageContent, Result, Source, Viewer,
};
use serde::Deserialize;

const BASE_URL: &str = "https://meshmanga.com";

#[derive(Deserialize, Clone)]
pub struct SeriesData {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub series_type: Option<String>,
    #[serde(default, rename = "seriesType")]
    pub series_type_alt: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct ChapterData {
    pub id: i64,
    #[serde(default)]
    pub number: Option<f32>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub series_id: Option<i64>,
}

#[derive(Deserialize, Clone)]
pub struct PageData {
    pub url: String,
    #[serde(default)]
    pub order: Option<i32>,
}

struct MeshManga;

impl Source for MeshManga {
    fn new() -> Self {
        MeshManga
    }

    fn get_search_manga_list(
        &self,
        query: Option<String>,
        _page: i32,
        _filters: Vec<FilterValue>,
    ) -> Result<MangaPageResult> {
        let search_term = query.unwrap_or_default();

        let url = if search_term.is_empty() {
            format!("{}/api/v1/series", BASE_URL)
        } else {
            format!(
                "{}/api/v1/series/search?q={}",
                BASE_URL,
                encode_uri_component(&search_term)
            )
        };

        let mut entries: Vec<Manga> = Vec::new();

        if let Ok(data) = Request::get(&url)?.json_owned::<Vec<SeriesData>>() {
            for series in data.into_iter().take(20) {
                let status = match series.status.as_deref() {
                    Some("ongoing") => MangaStatus::Ongoing,
                    Some("completed") => MangaStatus::Completed,
                    Some("hiatus") => MangaStatus::Hiatus,
                    Some("dropped") => MangaStatus::Cancelled,
                    _ => MangaStatus::Unknown,
                };

                let series_type = series.series_type.or(series.series_type_alt);
                let viewer = match series_type.as_deref() {
                    Some("MANHWA") | Some("manhwa") => Viewer::Webtoon,
                    Some("MANHUA") | Some("manhua") => Viewer::Webtoon,
                    _ => Viewer::RightToLeft,
                };

                entries.push(Manga {
                    key: series.id.to_string(),
                    title: series.name,
                    cover: series.cover,
                    description: series.description,
                    status,
                    viewer,
                    url: Some(format!("{}/series/{}", BASE_URL, series.id)),
                    ..Default::default()
                });
            }
        }

        Ok(MangaPageResult {
            entries,
            has_next_page: false,
        })
    }

    fn get_manga_update(
        &self,
        mut manga: Manga,
        _needs_details: bool,
        needs_chapters: bool,
    ) -> Result<Manga> {
        let series_id = manga.key.clone();

        if needs_chapters {
            let url = format!("{}/api/v1/series/id/{}/chapters", BASE_URL, series_id);

            if let Ok(chapters_data) = Request::get(&url)?.json_owned::<Vec<ChapterData>>() {
                let mut chapters: Vec<Chapter> = Vec::new();

                for ch in chapters_data {
                    chapters.push(Chapter {
                        key: ch.id.to_string(),
                        title: ch.title,
                        chapter_number: ch.number,
                        url: Some(format!("{}/chapter/{}", BASE_URL, ch.id)),
                        language: Some(String::from("ar")),
                        ..Default::default()
                    });
                }

                manga.chapters = Some(chapters);
            }
        }

        Ok(manga)
    }

    fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
        let ch_id = chapter.key;

        let url = format!("{}/api/v1/chapter/id/{}/pages", BASE_URL, ch_id);

        if let Ok(pages_data) = Request::get(&url)?.json_owned::<Vec<PageData>>() {
            let pages: Vec<Page> = pages_data
                .into_iter()
                .filter(|p| !p.url.is_empty())
                .map(|p| Page {
                    content: PageContent::url(p.url),
                    ..Default::default()
                })
                .collect();

            return Ok(pages);
        }

        Ok(Vec::new())
    }
}

impl ListingProvider for MeshManga {
    fn get_manga_list(&self, listing: Listing, _page: i32) -> Result<MangaPageResult> {
        let url = match listing.id.as_str() {
            "latest" => format!("{}/api/v1/series?sort=latest", BASE_URL),
            "popular" => format!("{}/api/v1/series?sort=popular", BASE_URL),
            "manhwa" => format!("{}/api/v1/series?type=MANHWA", BASE_URL),
            _ => format!("{}/api/v1/series", BASE_URL),
        };

        let mut entries: Vec<Manga> = Vec::new();

        if let Ok(data) = Request::get(&url)?.json_owned::<Vec<SeriesData>>() {
            for series in data.into_iter().take(20) {
                let status = match series.status.as_deref() {
                    Some("ongoing") => MangaStatus::Ongoing,
                    Some("completed") => MangaStatus::Completed,
                    Some("hiatus") => MangaStatus::Hiatus,
                    Some("dropped") => MangaStatus::Cancelled,
                    _ => MangaStatus::Unknown,
                };

                let series_type = series.series_type.or(series.series_type_alt);
                let viewer = match series_type.as_deref() {
                    Some("MANHWA") | Some("manhwa") => Viewer::Webtoon,
                    Some("MANHUA") | Some("manhua") => Viewer::Webtoon,
                    _ => Viewer::RightToLeft,
                };

                entries.push(Manga {
                    key: series.id.to_string(),
                    title: series.name,
                    cover: series.cover,
                    description: series.description,
                    status,
                    viewer,
                    url: Some(format!("{}/series/{}", BASE_URL, series.id)),
                    ..Default::default()
                });
            }
        }

        Ok(MangaPageResult {
            entries,
            has_next_page: false,
        })
    }
}

impl DeepLinkHandler for MeshManga {
    fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
        let path = url
            .strip_prefix("https://meshmanga.com/")
            .or_else(|| url.strip_prefix("http://meshmanga.com/"))
            .unwrap_or("");

        if let Some(series_part) = path.strip_prefix("series/") {
            let series_id = series_part.split('/').next().unwrap_or("");
            if !series_id.is_empty() && series_id.chars().all(|c| c.is_ascii_digit()) {
                return Ok(Some(DeepLinkResult::Manga {
                    key: series_id.to_string(),
                }));
            }
        }

        Ok(None)
    }
}

register_source!(MeshManga, ListingProvider, DeepLinkHandler);
