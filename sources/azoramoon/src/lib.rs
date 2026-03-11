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

const API_URL: &str = "https://api.azoramoon.com/api";
const BASE_URL: &str = "https://azoramoon.com";
const PAGE_SIZE: usize = 20;

// --- API response structs ---

#[derive(Deserialize)]
struct PostsResponse {
	posts: Vec<ApiPost>,
	#[serde(rename = "totalCount")]
	_total_count: i64,
}

#[derive(Deserialize)]
struct ApiPost {
	id: i64,
	slug: String,
	#[serde(rename = "postTitle")]
	post_title: String,
	#[serde(rename = "postContent")]
	post_content: Option<String>,
	#[serde(rename = "featuredImage")]
	featured_image: Option<String>,
	#[serde(rename = "seriesStatus")]
	series_status: Option<String>,
	#[serde(rename = "seriesType")]
	series_type: Option<String>,
	#[serde(default)]
	genres: Vec<ApiGenre>,
}

#[derive(Deserialize)]
struct ApiGenre {
	name: String,
}

#[derive(Deserialize)]
struct ChaptersResponse {
	post: ChaptersPost,
	#[serde(rename = "totalChapterCount")]
	total_chapter_count: i64,
}

#[derive(Deserialize)]
struct ChaptersPost {
	chapters: Vec<ApiChapter>,
}

#[derive(Deserialize)]
struct ApiChapter {
	id: i64,
	slug: String,
	number: Option<f64>,
	title: Option<String>,
	#[serde(rename = "createdAt")]
	created_at: Option<String>,
	#[serde(rename = "isLocked")]
	is_locked: Option<bool>,
	#[serde(rename = "isAccessible")]
	is_accessible: Option<bool>,
}

#[derive(Deserialize)]
struct RscImage {
	url: String,
	order: i64,
}

// --- Helpers ---

struct AzoraMoon;

fn posts_to_result(resp: PostsResponse) -> MangaPageResult {
	let mut entries: Vec<Manga> = Vec::new();

	for post in resp.posts {
		let status = match post.series_status.as_deref() {
			Some("ONGOING") => MangaStatus::Ongoing,
			Some("COMPLETED") => MangaStatus::Completed,
			Some("HIATUS") => MangaStatus::Hiatus,
			Some("CANCELLED" | "DROPPED") => MangaStatus::Cancelled,
			_ => MangaStatus::Unknown,
		};

		let viewer = match post.series_type.as_deref() {
			Some("MANHWA" | "MANHUA") => Viewer::Webtoon,
			_ => Viewer::RightToLeft,
		};

		let tags: Vec<String> = post
			.genres
			.into_iter()
			.map(|g| g.name)
			.filter(|n| !n.is_empty())
			.collect();

		let description = post.post_content.as_deref().map(strip_html_tags);

		entries.push(Manga {
			key: format!("{}/{}", post.id, post.slug),
			title: post.post_title,
			cover: post.featured_image,
			description,
			status,
			viewer,
			tags: if tags.is_empty() { None } else { Some(tags) },
			url: Some(format!("{}/series/{}", BASE_URL, post.slug)),
			..Default::default()
		});
	}

	let count = entries.len();
	MangaPageResult {
		entries,
		has_next_page: count >= PAGE_SIZE,
	}
}

fn strip_html_tags(html: &str) -> String {
	let mut result = String::new();
	let mut in_tag = false;
	for c in html.chars() {
		if c == '<' {
			in_tag = true;
		} else if c == '>' {
			in_tag = false;
		} else if !in_tag {
			result.push(c);
		}
	}
	result
}

fn parse_key(key: &str) -> (i64, &str) {
	if let Some(pos) = key.find('/') {
		let id = key[..pos].parse::<i64>().unwrap_or(0);
		let slug = &key[pos + 1..];
		(id, slug)
	} else {
		(0, key)
	}
}

/// Extract a string value for a given key from RSC-escaped text.
fn extract_rsc_string_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
	let pattern = format!("\\\"{}\\\":\\\"", key);
	if let Some(start) = text.find(&pattern) {
		let value_start = start + pattern.len();
		let remaining = &text[value_start..];
		if let Some(end) = remaining.find("\\\"") {
			return Some(&remaining[..end]);
		}
	}
	None
}

/// Extract a JSON array for a given key from RSC-escaped text, unescaping it.
fn extract_rsc_array(text: &str, key: &str) -> Option<String> {
	let pattern = format!("\\\"{}\\\":[", key);
	if let Some(start) = text.find(&pattern) {
		let arr_start = start + pattern.len() - 1;
		let remaining = &text[arr_start..];
		let mut depth = 0;
		for (i, c) in remaining.char_indices() {
			match c {
				'[' => depth += 1,
				']' => {
					depth -= 1;
					if depth == 0 {
						let raw = &remaining[..i + 1];
						return Some(raw.replace("\\\"", "\"").replace("\\\\", "\\"));
					}
				}
				_ => {}
			}
		}
	}
	None
}

/// Extract the first numeric ID from RSC-escaped JSON (pattern: \"id\":NNN).
fn extract_rsc_id(text: &str) -> Option<i64> {
	let pattern = "\\\"id\\\":";
	if let Some(start) = text.find(pattern) {
		let val_start = start + pattern.len();
		let remaining = &text[val_start..];
		let end = remaining.find(|c: char| !c.is_ascii_digit()).unwrap_or(remaining.len());
		if end > 0 {
			return remaining[..end].parse::<i64>().ok();
		}
	}
	None
}

/// Scrape the "Popular Today" carousel from the homepage.
fn get_popular_list(page: i32) -> Result<MangaPageResult> {
	if page > 1 {
		return Ok(MangaPageResult {
			entries: Vec::new(),
			has_next_page: false,
		});
	}
	let html = Request::get(BASE_URL)?.string()?;
	let mut entries: Vec<Manga> = Vec::new();
	let mut pos = 0;
	// Parse swiper-slide cards from the popular carousel (first section on page)
	while let Some(slide_off) = html[pos..].find("swiper-slide manga-swipe") {
		let abs = pos + slide_off;
		let slide_end = html[abs..].find("</a>").map(|e| abs + e + 4).unwrap_or(html.len());
		let slide = &html[abs..slide_end];

		// Stop if we've left the manga slider and hit the novels section
		if slide.contains("alt=\"NOVEL\"") {
			break;
		}

		let slug = extract_attr(slide, "href=\"/series/", '"');
		let title = extract_attr(slide, "title=\"", '"');
		let cover = extract_attr(slide, "src=\"", '"');

		if let (Some(slug), Some(title)) = (slug, title) {
			if !title.starts_with("Cover of") {
				entries.push(Manga {
					key: format!("0/{}", slug),
					title: String::from(title),
					cover: cover.map(|s| String::from(s)),
					url: Some(format!("{}/series/{}", BASE_URL, slug)),
					..Default::default()
				});
			}
		}
		pos = slide_end;
	}
	Ok(MangaPageResult {
		entries,
		has_next_page: false,
	})
}

/// Extract the value of an attribute from an HTML snippet.
fn extract_attr<'a>(html: &'a str, prefix: &str, end_char: char) -> Option<&'a str> {
	if let Some(start) = html.find(prefix) {
		let val_start = start + prefix.len();
		if let Some(end) = html[val_start..].find(end_char) {
			return Some(&html[val_start..val_start + end]);
		}
	}
	None
}

// --- Source implementation ---

impl Source for AzoraMoon {
	fn new() -> Self {
		AzoraMoon
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let search_term = query.unwrap_or_default();
		let url = format!(
			"{}/posts?page={}&perPage={}&searchTerm={}&isNovel=false",
			API_URL,
			page,
			PAGE_SIZE,
			encode_uri_component(&search_term)
		);
		let resp: PostsResponse = Request::get(&url)?.json_owned()?;
		Ok(posts_to_result(resp))
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let (post_id, slug) = parse_key(&manga.key);

		// Resolve post ID if unknown (from popular listing scrape)
		if post_id == 0 {
			let page_url = format!("{}/series/{}", BASE_URL, slug);
			let html = Request::get(&page_url)?.string()?;
			// Extract post ID from RSC data: \"id\":NNN (first occurrence)
			if let Some(resolved_id) = extract_rsc_id(&html) {
				let new_key = format!("{}/{}", resolved_id, slug);
				manga.key = new_key;
				return self.get_manga_update(manga, needs_details, needs_chapters);
			}
			// Fallback: search for the manga
			let search_url = format!(
				"{}/posts?page=1&perPage=5&searchTerm={}&isNovel=false",
				API_URL,
				encode_uri_component(&slug.replace('-', " "))
			);
			if let Ok(resp) = Request::get(&search_url)?.json_owned::<PostsResponse>() {
				for post in resp.posts {
					if post.slug == slug {
						let new_key = format!("{}/{}", post.id, slug);
						manga.key = new_key;
						return self.get_manga_update(manga, needs_details, needs_chapters);
					}
				}
			}
		}

		if needs_details {
			let page_url = format!("{}/series/{}", BASE_URL, slug);
			let html = Request::get(&page_url)?.string()?;

			if let Some(title) = extract_rsc_string_value(&html, "postTitle") {
				manga.title = String::from(title);
			}
			if let Some(content) = extract_rsc_string_value(&html, "postContent") {
				manga.description = Some(strip_html_tags(content));
			}
			if let Some(img) = extract_rsc_string_value(&html, "featuredImage") {
				manga.cover = Some(String::from(img));
			}
			if let Some(status_str) = extract_rsc_string_value(&html, "seriesStatus") {
				manga.status = match status_str {
					"ONGOING" => MangaStatus::Ongoing,
					"COMPLETED" => MangaStatus::Completed,
					"HIATUS" => MangaStatus::Hiatus,
					"CANCELLED" | "DROPPED" => MangaStatus::Cancelled,
					_ => MangaStatus::Unknown,
				};
			}
			if let Some(series_type) = extract_rsc_string_value(&html, "seriesType") {
				manga.viewer = match series_type {
					"MANHWA" | "MANHUA" => Viewer::Webtoon,
					_ => Viewer::RightToLeft,
				};
			}

			if let Some(genres_json) = extract_rsc_array(&html, "genres") {
				if let Ok(genres) = serde_json::from_str::<Vec<ApiGenre>>(&genres_json) {
					let tags: Vec<String> = genres
						.into_iter()
						.map(|g| g.name)
						.filter(|n| !n.is_empty())
						.collect();
					if !tags.is_empty() {
						manga.tags = Some(tags);
					}
				}
			}

			manga.url = Some(format!("{}/series/{}", BASE_URL, slug));
		}

		if needs_chapters {
			let mut all_chapters: Vec<Chapter> = Vec::new();
			let mut skip: i64 = 0;
			let take: i64 = 500;

			loop {
				let url = format!(
					"{}/chapters?postId={}&skip={}&take={}",
					API_URL, post_id, skip, take
				);
				let resp: ChaptersResponse = Request::get(&url)?.json_owned()?;
				let total = resp.total_chapter_count;
				let chapters = resp.post.chapters;
				let count = chapters.len() as i64;

				for ch in chapters {
					let ch_number = ch.number.map(|n| n as f32);
					let is_locked = ch.is_locked.unwrap_or(false);
					let is_accessible = ch.is_accessible.unwrap_or(true);

					let date = ch
						.created_at
						.as_deref()
						.and_then(|d| parse_date(d, "yyyy-MM-dd'T'HH:mm:ss.SSS'Z'"));

					all_chapters.push(Chapter {
						key: format!("{}/{}", ch.id, ch.slug),
						title: ch.title.filter(|s| !s.is_empty()),
						chapter_number: ch_number,
						date_uploaded: date,
						url: Some(format!("{}/series/{}/{}", BASE_URL, slug, ch.slug)),
						language: Some(String::from("ar")),
						locked: is_locked && !is_accessible,
						..Default::default()
					});
				}

				skip += count;
				if skip >= total || count == 0 {
					break;
				}
			}

			manga.chapters = Some(all_chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let (_post_id, manga_slug) = parse_key(&manga.key);
		let (_ch_id, ch_slug) = parse_key(&chapter.key);

		let url = format!("{}/series/{}/{}", BASE_URL, manga_slug, ch_slug);
		let html = Request::get(&url)?.string()?;

		let mut pages: Vec<Page> = Vec::new();

		if let Some(images_json) = extract_rsc_array(&html, "images") {
			if let Ok(mut images) = serde_json::from_str::<Vec<RscImage>>(&images_json) {
				images.sort_by_key(|img| img.order);
				for img in images {
					if !img.url.is_empty() {
						pages.push(Page {
							content: PageContent::url(img.url),
							..Default::default()
						});
					}
				}
			}
		}

		// Fallback: extract from <img data-image-index="N" src="..."> tags
		if pages.is_empty() {
			let mut search_pos = 0;
			while let Some(img_idx) = html[search_pos..].find("data-image-index=") {
				let abs_idx = search_pos + img_idx;
				if let Some(src_start) = html[abs_idx..].find("src=\"") {
					let url_start = abs_idx + src_start + 5;
					if let Some(url_end) = html[url_start..].find('"') {
						let img_url = &html[url_start..url_start + url_end];
						if !img_url.is_empty() {
							pages.push(Page {
								content: PageContent::url(String::from(img_url)),
								..Default::default()
							});
						}
					}
				}
				search_pos = abs_idx + 1;
			}
		}

		Ok(pages)
	}
}

impl ListingProvider for AzoraMoon {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		if listing.id.as_str() == "Popular" {
			return get_popular_list(page);
		}
		let tag = match listing.id.as_str() {
			"New" => "new",
			_ => "latestUpdate",
		};
		let url = format!(
			"{}/posts?page={}&perPage={}&searchTerm=&isNovel=false&tag={}",
			API_URL, page, PAGE_SIZE, tag
		);
		let resp: PostsResponse = Request::get(&url)?.json_owned()?;
		Ok(posts_to_result(resp))
	}
}

impl DeepLinkHandler for AzoraMoon {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let path = url
			.strip_prefix("https://azoramoon.com/series/")
			.or_else(|| url.strip_prefix("http://azoramoon.com/series/"))
			.unwrap_or("");
		let slug = path.split('/').next().unwrap_or("").trim_matches('/');

		if slug.is_empty() {
			return Ok(None);
		}

		let search_url = format!(
			"{}/posts?page=1&perPage=5&searchTerm={}",
			API_URL,
			encode_uri_component(&slug.replace('-', " "))
		);
		if let Ok(resp) = Request::get(&search_url)?.json_owned::<PostsResponse>() {
			for post in resp.posts {
				if post.slug == slug {
					let key = format!("{}/{}", post.id, slug);
					return Ok(Some(DeepLinkResult::Manga { key }));
				}
			}
		}

		Ok(None)
	}
}

register_source!(AzoraMoon, ListingProvider, DeepLinkHandler);
