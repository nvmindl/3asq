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
struct ChapterImage {
	url: String,
	order: i64,
}

#[derive(Deserialize)]
struct ChapterContentResponse {
	chapter: ChapterContent,
}

#[derive(Deserialize)]
struct ChapterContent {
	#[serde(default)]
	images: Vec<ChapterImage>,
}

#[derive(Deserialize)]
struct PostDetailResponse {
	post: ApiPost,
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

		if needs_details {
			let detail_url = format!("{}/post?postId={}", API_URL, post_id);
			let resp: PostDetailResponse = Request::get(&detail_url)?.json_owned()?;
			let post = resp.post;

			manga.title = post.post_title;
			manga.description = post.post_content.as_deref().map(strip_html_tags);
			manga.cover = post.featured_image;
			manga.status = match post.series_status.as_deref() {
				Some("ONGOING") => MangaStatus::Ongoing,
				Some("COMPLETED") => MangaStatus::Completed,
				Some("HIATUS") => MangaStatus::Hiatus,
				Some("CANCELLED" | "DROPPED") => MangaStatus::Cancelled,
				_ => MangaStatus::Unknown,
			};
			manga.viewer = match post.series_type.as_deref() {
				Some("MANHWA" | "MANHUA") => Viewer::Webtoon,
				_ => Viewer::RightToLeft,
			};

			let tags: Vec<String> = post
				.genres
				.into_iter()
				.map(|g| g.name)
				.filter(|n| !n.is_empty())
				.collect();
			if !tags.is_empty() {
				manga.tags = Some(tags);
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

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let (ch_id, _ch_slug) = parse_key(&chapter.key);

		let url = format!("{}/chapter?chapterId={}", API_URL, ch_id);
		let resp: ChapterContentResponse = Request::get(&url)?.json_owned()?;

		let mut images = resp.chapter.images;
		images.sort_by_key(|img| img.order);

		let pages: Vec<Page> = images
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

impl ListingProvider for AzoraMoon {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let url = match listing.id.as_str() {
			"Popular" => format!(
				"{}/query?page={}&perPage={}&orderBy=totalViews&orderDirection=desc",
				API_URL, page, PAGE_SIZE
			),
			"New" => format!(
				"{}/posts?page={}&perPage={}&searchTerm=&isNovel=false&tag=new",
				API_URL, page, PAGE_SIZE
			),
			_ => format!(
				"{}/posts?page={}&perPage={}&searchTerm=&isNovel=false&tag=latestUpdate",
				API_URL, page, PAGE_SIZE
			),
		};
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
