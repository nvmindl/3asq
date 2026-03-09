#![no_std]

use aidoku::{
        alloc::{String, Vec},
        imports::{
                html::Element,
                net::Request,
        },
        prelude::*,
        Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Listing,
        ListingProvider, Manga, MangaPageResult, MangaStatus, Page, PageContent,
        Result, Source, Viewer,
};

const BASE_URL: &str = "https://3asq.org";

const SORT_OPTIONS: &[&str] = &["latest", "alphabet", "rating", "trending", "views"];

struct ThreeAsq;

fn get_image_url(el: &Element) -> Option<String> {
        let url = el
                .attr("data-src")
                .or_else(|| el.attr("data-lazy-src"))
                .or_else(|| el.attr("src"))
                .or_else(|| el.attr("srcset"))?;
        let trimmed = url.trim();
        if trimmed.is_empty() {
                None
        } else {
                Some(String::from(trimmed))
        }
}

fn extract_manga_key(url: &str) -> String {
        let stripped = url
                .strip_prefix("https://3asq.org/manga/")
                .or_else(|| url.strip_prefix("http://3asq.org/manga/"))
                .unwrap_or(url);
        let key = stripped.trim_matches('/');
        // Take only the first path segment (the manga slug)
        key.split('/').next().unwrap_or(key).into()
}

fn parse_manga_list_page(url: &str) -> Result<MangaPageResult> {
        let html = Request::get(url)?
                .header("Cookie", "wpmanga-adault=1")
                .html()?;

        let mut entries: Vec<Manga> = Vec::new();

        if let Some(items) = html.select("div.c-tabs-item__content") {
                for item in items {
                        let key = item
                                .select_first("a")
                                .and_then(|a| a.attr("href"))
                                .map(|href| extract_manga_key(&href))
                                .unwrap_or_default();

                        if key.is_empty() {
                                continue;
                        }

                        let title = item
                                .select_first("a")
                                .and_then(|a| a.attr("title"))
                                .unwrap_or_default();

                        let cover = item
                                .select_first("img")
                                .and_then(|img| get_image_url(&img));

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

impl Source for ThreeAsq {
        fn new() -> Self {
                ThreeAsq
        }

        fn get_search_manga_list(
                &self,
                query: Option<String>,
                page: i32,
                filters: Vec<FilterValue>,
        ) -> Result<MangaPageResult> {
                let mut order = "latest";

                for filter in &filters {
                        if let FilterValue::Sort { id, index, .. } = filter {
                                if id == "order" {
                                        if let Some(o) = SORT_OPTIONS.get(*index as usize) {
                                                order = o;
                                        }
                                }
                        }
                }

                let url = if let Some(ref q) = query {
                        if !q.is_empty() {
                                format!(
                                        "{}/page/{}/?s={}&post_type=wp-manga&m_orderby={}",
                                        BASE_URL, page, q, order
                                )
                        } else {
                                format!(
                                        "{}/page/{}/?s&post_type=wp-manga&m_orderby={}",
                                        BASE_URL, page, order
                                )
                        }
                } else {
                        format!(
                                "{}/page/{}/?s&post_type=wp-manga&m_orderby={}",
                                BASE_URL, page, order
                        )
                };

                parse_manga_list_page(&url)
        }

        fn get_manga_update(
                &self,
                mut manga: Manga,
                needs_details: bool,
                needs_chapters: bool,
        ) -> Result<Manga> {
                let detail_url = format!("{}/manga/{}/", BASE_URL, manga.key);

                if needs_details {
                        let html = Request::get(&detail_url)?
                                .header("Cookie", "wpmanga-adault=1")
                                .html()?;

                        // Title — strip badge text
                        if let Some(title_el) = html.select_first("div.post-title h1") {
                                let badge_text = title_el
                                        .select_first("span.manga-title-badges")
                                        .and_then(|e| e.text())
                                        .unwrap_or_default();
                                let mut title = title_el.text().unwrap_or_default();
                                if !badge_text.is_empty() {
                                        title = title.replace(&badge_text, "");
                                }
                                manga.title = String::from(title.trim());
                        }

                        // Cover
                        manga.cover = html
                                .select_first("div.summary_image img")
                                .and_then(|img| get_image_url(&img));

                        // Authors
                        if let Some(els) = html.select("div.author-content a") {
                                let authors: Vec<String> = els
                                        .filter_map(|a| a.text())
                                        .map(|s| String::from(s.trim()))
                                        .filter(|s| !s.is_empty())
                                        .collect();
                                if !authors.is_empty() {
                                        manga.authors = Some(authors);
                                }
                        }

                        // Artists
                        if let Some(els) = html.select("div.artist-content a") {
                                let artists: Vec<String> = els
                                        .filter_map(|a| a.text())
                                        .map(|s| String::from(s.trim()))
                                        .filter(|s| !s.is_empty())
                                        .collect();
                                if !artists.is_empty() {
                                        manga.artists = Some(artists);
                                }
                        }

                        // Description
                        manga.description = html
                                .select_first("div.manga-excerpt p")
                                .and_then(|e| e.text());

                        // Tags / Genres
                        if let Some(genre_links) = html.select("div.genres-content > a") {
                                let tags: Vec<String> = genre_links
                                        .filter_map(|a| a.text())
                                        .map(|s| String::from(s.trim()))
                                        .filter(|s| !s.is_empty())
                                        .collect();
                                if !tags.is_empty() {
                                        manga.tags = Some(tags);
                                }
                        }

                        // Status
                        let status_text = html
                                .select_first(
                                        "div.post-content_item:contains(\u{0627}\u{0644}\u{062D}\u{0627}\u{0644}\u{0629}) div.summary-content",
                                )
                                .and_then(|e| e.text())
                                .unwrap_or_default()
                                .to_lowercase();

                        manga.status =
                                if status_text.contains("\u{0645}\u{0633}\u{062A}\u{0645}\u{0631}\u{0629}")
                                        || status_text.contains("ongoing")
                                {
                                        MangaStatus::Ongoing
                                } else if status_text.contains("\u{0645}\u{0643}\u{062A}\u{0645}\u{0644}\u{0629}")
                                        || status_text.contains("completed")
                                {
                                        MangaStatus::Completed
                                } else if status_text.contains("\u{0645}\u{062A}\u{0648}\u{0642}\u{0641}\u{0629}")
                                        || status_text.contains("hiatus")
                                        || status_text.contains("on hold")
                                {
                                        MangaStatus::Hiatus
                                } else if status_text.contains("\u{0645}\u{0644}\u{063A}\u{0627}\u{0629}")
                                        || status_text.contains("canceled")
                                {
                                        MangaStatus::Cancelled
                                } else {
                                        MangaStatus::Unknown
                                };

                        manga.url = Some(detail_url.clone());
                        manga.viewer = Viewer::RightToLeft;
                }

                if needs_chapters {
                        let chapters_url =
                                format!("{}/manga/{}/ajax/chapters/", BASE_URL, manga.key);

                        let html = Request::post(&chapters_url)?
                                .header("Referer", &detail_url)
                                .header("Content-Type", "application/x-www-form-urlencoded")
                                .html()?;

                        let mut chapters: Vec<Chapter> = Vec::new();

                        if let Some(items) = html.select("li.wp-manga-chapter") {
                                for item in items {
                                        let ch_url = item
                                                .select_first("a")
                                                .and_then(|a| a.attr("href"))
                                                .unwrap_or_default();

                                        let ch_key = ch_url
                                                .replace(BASE_URL, "")
                                                .replacen("/manga/", "", 1);
                                        let ch_key = ch_key.trim_matches('/');

                                        if ch_key.is_empty() {
                                                continue;
                                        }

                                        let ch_text = item
                                                .select_first("a")
                                                .and_then(|a| a.text())
                                                .unwrap_or_default();

                                        // Extract chapter title after dash
                                        let ch_title = if let Some(pos) = ch_text.find('-') {
                                                let t = ch_text[pos + 1..].trim();
                                                if t.is_empty() { None } else { Some(String::from(t)) }
                                        } else {
                                                None
                                        };

                                        // Parse chapter number from slug
                                        let slug_parts: Vec<&str> = ch_key.split('/').collect();
                                        let chapter_slug = if slug_parts.len() >= 2 {
                                                slug_parts[slug_parts.len() - 1]
                                        } else {
                                                ch_key
                                        };

                                        let chapter_number = parse_chapter_number(chapter_slug);

                                        chapters.push(Chapter {
                                                key: String::from(ch_key),
                                                title: ch_title,
                                                chapter_number: Some(chapter_number),
                                                url: Some(ch_url),
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
                        format!("{}/manga/{}/", BASE_URL, chapter.key)
                };

                let html = Request::get(&url)?
                        .header("Cookie", "wpmanga-adault=1")
                        .html()?;

                let mut pages: Vec<Page> = Vec::new();

                if let Some(imgs) = html.select("div.page-break img") {
                        for img in imgs {
                                if let Some(img_url) = get_image_url(&img) {
                                        pages.push(Page {
                                                content: PageContent::url(img_url),
                                                ..Default::default()
                                        });
                                }
                        }
                }

                Ok(pages)
        }
}

impl ListingProvider for ThreeAsq {
        fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
                let order = match listing.id.as_str() {
                        "Popular" => "views",
                        "Trending" => "trending",
                        _ => "latest",
                };
                let url = format!(
                        "{}/page/{}/?s&post_type=wp-manga&m_orderby={}",
                        BASE_URL, page, order
                );
                parse_manga_list_page(&url)
        }
}

impl DeepLinkHandler for ThreeAsq {
        fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
                let key = extract_manga_key(&url);
                if key.is_empty() {
                        return Ok(None);
                }
                Ok(Some(DeepLinkResult::Manga { key }))
        }
}

fn parse_chapter_number(slug: &str) -> f32 {
        let parts: Vec<&str> = slug.split('-').collect();
        let mut number = 0.0_f32;
        let mut found = false;
        for part in &parts {
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

register_source!(ThreeAsq, ListingProvider, DeepLinkHandler);
