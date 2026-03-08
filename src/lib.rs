#![no_std]
use aidoku::{
	alloc::{String, Vec},
	imports::net::Request,
	prelude::*,
	Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Listing, ListingProvider,
	Manga, MangaPageResult, MangaStatus, Page, PageContent, Result, Source,
};

const BASE_URL: &str = "https://3asq.org";

struct ThreeAsq;

impl Source for ThreeAsq {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		if let Some(query) = query {
			let url = format!(
				"{}/page/{}/?s={}&post_type=wp-manga",
				BASE_URL, page, query
			);
			let html = Request::get(&url)?.html()?;
			let mut entries: Vec<Manga> = Vec::new();

			if let Some(items) = html.select(".c-tabs-item .row.c-tabs-item__content") {
				for item in items {
					let title_el = item.select_first(".post-title a");
					let title = title_el.as_ref().and_then(|e| e.text()).unwrap_or_default();
					let href = title_el
						.as_ref()
						.and_then(|e| e.attr("href"))
						.unwrap_or_default();
					let key = extract_manga_key(&href);
					let cover = item
						.select_first(".tab-thumb img")
						.and_then(|e| e.attr("src"));

					if !key.is_empty() {
						entries.push(Manga {
							key,
							title,
							cover,
							..Default::default()
						});
					}
				}
			}

			let has_next = html.select_first(".wp-pagenavi .nextpostslink").is_some()
				|| html.select_first(".nav-previous a").is_some();

			Ok(MangaPageResult {
				entries,
				has_next_page: has_next,
			})
		} else {
			self.get_manga_list_page(page, "latest")
		}
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let url = format!("{}/manga/{}/", BASE_URL, manga.key);
		let html = Request::get(&url)?.html()?;

		if needs_details {
			manga.url = Some(url);

			// Title
			if let Some(title) = html.select_first(".post-title h1").and_then(|e| e.text()) {
				manga.title = title.trim().into();
			}

			// Cover
			manga.cover = html
				.select_first(".summary_image img")
				.and_then(|e| e.attr("src"));

			// Parse metadata from content items
			if let Some(content_items) = html.select(".post-content_item") {
				for item in content_items {
					let heading = item
						.select_first(".summary-heading h5")
						.and_then(|e| e.text())
						.unwrap_or_default();
					let heading = heading.trim();

					if heading.contains("الكاتب") {
						manga.authors = Some(
							item.select(".summary-content a")
								.into_iter()
								.flatten()
								.filter_map(|a| a.text())
								.collect(),
						);
					} else if heading.contains("الرسام") {
						manga.artists = Some(
							item.select(".summary-content a")
								.into_iter()
								.flatten()
								.filter_map(|a| a.text())
								.collect(),
						);
					} else if heading.contains("التصنيفات") {
						manga.tags = Some(
							item.select(".summary-content a")
								.into_iter()
								.flatten()
								.filter_map(|a| a.text())
								.collect(),
						);
					} else if heading.contains("الحالة") {
						let status_text = item
							.select_first(".summary-content")
							.and_then(|e| e.text())
							.unwrap_or_default();
						let status_text = status_text.trim();
						manga.status = if status_text.contains("مستمرة") {
							MangaStatus::Ongoing
						} else if status_text.contains("مكتملة") {
							MangaStatus::Completed
						} else if status_text.contains("متوقفة") {
							MangaStatus::Hiatus
						} else if status_text.contains("ملغاة") {
							MangaStatus::Cancelled
						} else {
							MangaStatus::Unknown
						};
					}
				}
			}

			// Description
			manga.description = html
				.select_first(".description-summary .summary__content")
				.or_else(|| html.select_first(".summary__content"))
				.and_then(|e| e.text())
				.map(|t| t.trim().into());
		}

		if needs_chapters {
			let mut chapters: Vec<Chapter> = Vec::new();

			if let Some(chapter_items) =
				html.select(".listing-chapters_wrap .wp-manga-chapter")
			{
				for ch in chapter_items {
					let link = ch.select_first("a");
					let ch_title = link.as_ref().and_then(|e| e.text()).unwrap_or_default();
					let ch_href = link
						.as_ref()
						.and_then(|e| e.attr("href"))
						.unwrap_or_default();
					let ch_key = extract_chapter_key(&ch_href, &manga.key);

					let chapter_number = parse_chapter_number(&ch_title, &ch_key);

					chapters.push(Chapter {
						key: ch_key,
						title: Some(ch_title.trim().into()),
						chapter_number: Some(chapter_number),
						..Default::default()
					});
				}
			}

			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let url = format!("{}/manga/{}/{}/", BASE_URL, manga.key, chapter.key);
		let html = Request::get(&url)?.html()?;
		let mut pages: Vec<Page> = Vec::new();

		if let Some(containers) = html.select(".reading-content .page-break") {
			for container in containers {
				let src: Option<String> = container
					.select_first("img")
					.and_then(|img| {
						img.attr("src").or_else(|| img.attr("data-src"))
					})
					.map(|s| {
						let trimmed: &str = s.trim();
						String::from(trimmed)
					});
				if let Some(src) = src {
					pages.push(Page {
						content: PageContent::url(&src),
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
			"latest" => "latest",
			"popular" => "views",
			_ => "latest",
		};
		self.get_manga_list_page(page, order)
	}
}

impl DeepLinkHandler for ThreeAsq {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let path = url
			.strip_prefix("https://3asq.org")
			.or_else(|| url.strip_prefix("http://3asq.org"))
			.unwrap_or(&url);

		let parts: Vec<&str> = path.trim_matches('/').split('/').collect();

		if parts.len() >= 2 && parts[0] == "manga" {
			let manga_key = String::from(parts[1]);
			if parts.len() >= 3 && !parts[2].is_empty() {
				return Ok(Some(DeepLinkResult::Chapter {
					manga_key,
					key: String::from(parts[2]),
				}));
			}
			return Ok(Some(DeepLinkResult::Manga { key: manga_key }));
		}

		Ok(None)
	}
}

impl ThreeAsq {
	fn get_manga_list_page(&self, page: i32, order: &str) -> Result<MangaPageResult> {
		let url = format!(
			"{}/manga/page/{}/?m_orderby={}",
			BASE_URL, page, order
		);
		let html = Request::get(&url)?.html()?;
		let mut entries: Vec<Manga> = Vec::new();

		if let Some(items) = html.select(".page-item-detail") {
			for item in items {
				let link = item.select_first(".post-title a");
				let title = link.as_ref().and_then(|e| e.text()).unwrap_or_default();
				let href = link
					.as_ref()
					.and_then(|e| e.attr("href"))
					.unwrap_or_default();
				let key = extract_manga_key(&href);
				let cover = item
					.select_first(".item-thumb img")
					.and_then(|e| e.attr("src"));

				if !key.is_empty() {
					entries.push(Manga {
						key,
						title: title.trim().into(),
						cover,
						..Default::default()
					});
				}
			}
		}

		let has_next = html
			.select_first(".wp-pagenavi .nextpostslink")
			.is_some()
			|| html.select_first("a.nextpostslink").is_some()
			|| html.select_first(".nav-previous a").is_some();

		Ok(MangaPageResult {
			entries,
			has_next_page: has_next,
		})
	}
}

fn extract_manga_key(url: &str) -> String {
	let stripped = url
		.strip_prefix("https://3asq.org/manga/")
		.or_else(|| url.strip_prefix("http://3asq.org/manga/"))
		.unwrap_or(url);
	let key = stripped.trim_matches('/');
	key.split('/').next().unwrap_or(key).into()
}

fn extract_chapter_key(url: &str, manga_key: &str) -> String {
	let prefix = format!("{}/manga/{}/", BASE_URL, manga_key);
	let stripped = url.strip_prefix(prefix.as_str()).unwrap_or(url);
	stripped.trim_matches('/').into()
}

fn parse_chapter_number(title: &str, key: &str) -> f32 {
	let key_num = key.replace('-', ".");
	if let Some(num) = try_parse_float(&key_num) {
		return num;
	}
	let title = title.trim();
	let num_str: String = title
		.chars()
		.take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
		.collect();
	let num_str = num_str.replace('-', ".");
	try_parse_float(&num_str).unwrap_or(0.0)
}

fn try_parse_float(s: &str) -> Option<f32> {
	if s.is_empty() {
		return None;
	}
	let parts: Vec<&str> = s.splitn(2, '.').collect();
	let integer: f32 = parts[0].parse::<i64>().ok()? as f32;
	if parts.len() == 2 {
		let frac_str = parts[1];
		let frac: f32 = frac_str.parse::<i64>().ok()? as f32;
		let mut divisor: f32 = 1.0;
		for _ in 0..frac_str.len() {
			divisor *= 10.0;
		}
		Some(integer + frac / divisor)
	} else {
		Some(integer)
	}
}

register_source!(ThreeAsq, ListingProvider, DeepLinkHandler);
