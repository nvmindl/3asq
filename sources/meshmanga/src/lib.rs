#![no_std]

use aidoku::{
    alloc::{String, Vec},
    helpers::uri::encode_uri_component,
    imports::{net::Request, std::parse_date},
    prelude::*,
    Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Listing, ListingProvider,
    Manga, MangaPageResult, MangaStatus, Page, PageContent, Result, Source, Viewer,
};
use serde::Deserialize;

const BASE_URL: &str = "https://meshmanga.com";
const SEARCH_URL: &str = "https://meshmanga.com";

#[derive(Deserialize, Clone)]
struct SearchResponse {
    series: Vec<SeriesItem>,
}

#[derive(Deserialize, Clone)]
struct SeriesItem {
    id: i64,
    name: String,
    slug: String,
    #[serde(default)]
    cover: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    seriesType: Option<String>,
}

#[derive(Deserialize)]
struct SeriesDetail {
    id: i64,
    name: String,
    slug: String,
    #[serde(default)]
    cover: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    seriesType: Option<String>,
    #[serde(default)]
    chapters: Vec<ChapterItem>,
}

#[derive(Deserialize, Clone)]
struct ChapterItem {
    id: i64,
    number: Option<f32>,
    title: Option<String>,
    slug: String,
    #[serde(default)]
    createdAt: Option<String>,
}

#[derive(Deserialize)]
struct ChapterContent {
    pages: Vec<ImageItem>,
}

#[derive(Deserialize, Clone)]
struct ImageItem {
    url: String,
    order: i32,
}

struct MeshManga;

fn parse_status(status_str: Option<&str>) -> MangaStatus {
    match status_str {
        Some("ongoing") | Some("ONGOING") => MangaStatus::Ongoing,
        Some("completed") | Some("COMPLETED") => MangaStatus::Completed,
        Some("hiatus") | Some("HIATUS") => MangaStatus::Hiatus,
        Some("dropped") | Some("DROPPED") | Some("cancelled") | Some("CANCELLED") => {
            MangaStatus::Cancelled
        }
        _ => MangaStatus::Unknown,
    }
}

fn parse_viewer(series_type: Option<&str>) -> Viewer {
    match series_type {
        Some("MANHWA") | Some("manhwa") => Viewer::Webtoon,
        Some("MANHUA") | Some("manhua") => Viewer::Webtoon,
        _ => Viewer::RightToLeft,
    }
}

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
            format!("{}/api/series?limit=20", BASE_URL)
        } else {
            format!(
                "{}/api/search?q={}",
                BASE_URL,
                encode_uri_component(&search_term)
            )
        };

        let resp: SearchResponse = Request::get(&url)?.json_owned()?;
        
        let mut entries: Vec<Manga> = Vec::new();
        for series in resp.series {
            entries.push(Manga {
                key: format!("{}", series.id),
                title: series.name,
                cover: series.cover,
                description: series.description,
                status: parse_status(series.status.as_deref()),
                viewer: parse_viewer(series.seriesType.as_deref()),
                url: Some(format!("{}/series/{}", BASE_URL, series.id)),
                ..Default::default()
            });
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
        let series_id = manga.key.parse::<i64>().unwrap_or(0);

        if needs_chapters {
            let url = format!("{}/api/series/{}", BASE_URL, series_id);
            if let Ok(detail) = Request::get(&url)?.json_owned::<SeriesDetail>() {
                manga.title = detail.name;
                manga.description = detail.description;
                manga.cover = detail.cover;
                manga.status = parse_status(detail.status.as_deref());
                manga.viewer = parse_viewer(detail.seriesType.as_deref());

                let mut chapters: Vec<Chapter> = Vec::new();
                for ch in detail.chapters {
                    let date = ch
                        .createdAt
                        .as_deref()
                        .and_then(|d| parse_date(d, "yyyy-MM-dd'T'HH:mm:ss.SSS'Z'"));

                    chapters.push(Chapter {
                        key: format!("{}", ch.id),
                        title: ch.title,
                        chapter_number: ch.number,
                        date_uploaded: date,
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
        let ch_id = chapter.key.parse::<i64>().unwrap_or(0);

        let url = format!("{}/api/chapter/{}", BASE_URL, ch_id);
        if let Ok(content) = Request::get(&url)?.json_owned::<ChapterContent>() {
            let mut images = content.pages;
            images.sort_by_key(|img| img.order);

            let pages: Vec<Page> = images
                .into_iter()
                .filter(|img| !img.url.is_empty())
                .map(|img| Page {
                    content: PageContent::url(img.url),
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
            "latest" => format!("{}/api/series?sort=latest&limit=20", BASE_URL),
            "popular" => format!("{}/api/series?sort=popular&limit=20", BASE_URL),
            "manhwa" => format!("{}/api/series?type=MANHWA&limit=20", BASE_URL),
            _ => format!("{}/api/series?limit=20", BASE_URL),
        };

        let resp: SearchResponse = Request::get(&url)?.json_owned()?;
        
        let mut entries: Vec<Manga> = Vec::new();
        for series in resp.series {
            entries.push(Manga {
                key: format!("{}", series.id),
                title: series.name,
                cover: series.cover,
                description: series.description,
                status: parse_status(series.status.as_deref()),
                viewer: parse_viewer(series.seriesType.as_deref()),
                url: Some(format!("{}/series/{}", BASE_URL, series.id)),
                ..Default::default()
            });
        }

        Ok(MangaPageResult {
            entries,
            has_next_page: false,
        })
    }
}

impl DeepLinkHandler for MeshManga {
    fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
        let series_path = url
            .strip_prefix("https://meshmanga.com/series/")
            .or_else(|| url.strip_prefix("http://meshmanga.com/series/"))
            .or_else(|| url.strip_prefix(BASE_URL).and_then(|s| s.strip_prefix("/series/")))
            .unwrap_or("");

        let series_id = series_path.split('/').next().unwrap_or("");

        if !series_id.is_empty() && series_id.chars().all(|c| c.is_ascii_digit()) {
            return Ok(Some(DeepLinkResult::Manga {
                key: series_id.to_string(),
            }));
        }

        Ok(None)
    }
}

register_source!(MeshManga, ListingProvider, DeepLinkHandler);
