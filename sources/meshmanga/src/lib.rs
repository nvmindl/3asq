#![no_std]

use aidoku::{
    alloc::{String, Vec},
    imports::{html::Element, net::Request},
    prelude::*,
    Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Listing, ListingProvider, Manga,
    MangaPageResult, MangaStatus, Page, PageContent, Result, Source, Viewer,
};

const BASE_URL: &str = "https://meshmanga.com";

struct MeshManga;

fn get_image_url(el: &Element) -> Option<String> {
    let url = el
        .attr("data-src")
        .or_else(|| el.attr("src"))
        .or_else(|| el.attr("srcset"))?;
    let trimmed = url.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(String::from(trimmed))
    }
}

fn extract_manga_key(url: &str) -> Option<String> {
    let stripped = url
        .strip_prefix("https://meshmanga.com/series/")
        .or_else(|| url.strip_prefix("http://meshmanga.com/series/"))
        .or_else(|| url.strip_prefix("/series/"))?;
    let key = stripped.trim_matches('/');
    let key = key.split('/').next().unwrap_or(key);
    if key.is_empty() || key.len() > 50 {
        None
    } else {
        Some(String::from(key))
    }
}

fn parse_manga_list_page(url: &str) -> Result<MangaPageResult> {
    let html = Request::get(url)?.html()?;

    let mut entries: Vec<Manga> = Vec::new();

    if let Some(items) = html.select("div.border-dark\\/20.rounded-sm") {
        for item in items {
            let link = item.select_first("a").and_then(|a| a.attr("href"));
            let href = match link {
                Some(s) => s,
                None => continue,
            };
            let key = match extract_manga_key(&href) {
                Some(k) => k,
                None => continue,
            };

            if key.is_empty() {
                continue;
            }

            let title = item
                .select_first("h3")
                .and_then(|h| h.text())
                .unwrap_or_default();

            let cover = item.select_first("img").and_then(|img| get_image_url(&img));

            entries.push(Manga {
                key,
                title,
                cover,
                ..Default::default()
            });
        }
    }

    let has_next_page = !entries.is_empty();
    Ok(MangaPageResult {
        entries,
        has_next_page,
    })
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
        let url = if let Some(ref q) = query {
            if !q.is_empty() {
                format!("{}/?s={}&post_type=wp-manga&page={}", BASE_URL, q, page)
            } else {
                format!("{}/?s&post_type=wp-manga&page={}", BASE_URL, page)
            }
        } else {
            format!("{}/?s&post_type=wp-manga&page={}", BASE_URL, page)
        };
        parse_manga_list_page(&url)
    }

    fn get_manga_update(
        &self,
        mut manga: Manga,
        needs_details: bool,
        needs_chapters: bool,
    ) -> Result<Manga> {
        let detail_url = format!("{}/series/{}/", BASE_URL, manga.key);

        if needs_details {
            let html = Request::get(&detail_url)?.html()?;

            if let Some(title_el) = html.select_first("h1.font-bold") {
                manga.title = title_el.text().unwrap_or_default();
            }

            manga.cover = html
                .select_first("img.rounded-sm")
                .and_then(|img| get_image_url(&img));

            if let Some(desc) = html.select_first("div.text-sm.text-gray-300") {
                manga.description = desc.text();
            }

            if let Some(status_el) = html.select_first("div.flex.items-center.gap-2.mt-4") {
                let status_text = status_el.text().unwrap_or_default().to_lowercase();
                manga.status = if status_text.contains("مستمرة") || status_text.contains("ongoing")
                {
                    MangaStatus::Ongoing
                } else if status_text.contains("مكتملة") || status_text.contains("completed")
                {
                    MangaStatus::Completed
                } else {
                    MangaStatus::Unknown
                };
            }

            manga.url = Some(detail_url.clone());
            manga.viewer = Viewer::RightToLeft;
        }

        if needs_chapters {
            let chapters_url = format!("{}/series/{}/", BASE_URL, manga.key);
            let html = Request::get(&chapters_url)?.html()?;

            let mut chapters: Vec<Chapter> = Vec::new();

            if let Some(items) = html.select("div.rounded-md.bg-\\[\\#22242a\\]") {
                for item in items {
                    let link = item.select_first("a").and_then(|a| a.attr("href"));
                    let ch_url = link.unwrap_or_default();

                    let ch_key = ch_url
                        .strip_prefix(BASE_URL)
                        .unwrap_or(&ch_url)
                        .trim_matches('/');

                    if ch_key.is_empty() {
                        continue;
                    }

                    let ch_text = item
                        .select_first("h4")
                        .and_then(|h| h.text())
                        .unwrap_or_default();

                    let chapter_number = parse_chapter_number(ch_key);

                    chapters.push(Chapter {
                        key: String::from(ch_key),
                        title: Some(ch_text),
                        chapter_number: Some(chapter_number),
                        url: Some(format!("{}{}", BASE_URL, ch_key)),
                        language: Some(String::from("ar")),
                        ..Default::default()
                    });
                }
            }

            manga.chapters = Some(chapters);
        }

        Ok(manga)
    }

    fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
        let url = if let Some(ref ch_url) = chapter.url {
            ch_url.clone()
        } else {
            format!("{}/{}", BASE_URL, chapter.key)
        };

        let html = Request::get(&url)?.html()?;

        let mut pages: Vec<Page> = Vec::new();

        if let Some(imgs) = html.select("img") {
            for img in imgs {
                if let Some(img_url) = get_image_url(&img) {
                    if img_url.contains("meshmanga") || img_url.contains("swat") {
                        pages.push(Page {
                            content: PageContent::url(img_url),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        Ok(pages)
    }
}

impl ListingProvider for MeshManga {
    fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
        let order = match listing.id.as_str() {
            "Popular" => "views",
            "Latest" | _ => "latest",
        };
        let url = format!(
            "{}/menu/type/manga/?m_orderby={}&page={}",
            BASE_URL, order, page
        );
        parse_manga_list_page(&url)
    }
}

impl DeepLinkHandler for MeshManga {
    fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
        let key = extract_manga_key(&url);
        if let Some(key) = key {
            Ok(Some(DeepLinkResult::Manga { key }))
        } else {
            Ok(None)
        }
    }
}

fn parse_chapter_number(slug: &str) -> f32 {
    let parts: Vec<&str> = slug.split('-').collect();
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
    number
}

register_source!(MeshManga, ListingProvider, DeepLinkHandler);
