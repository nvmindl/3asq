#![no_std]
use aidoku::{
	error::Result,
	prelude::*,
	std::{
		html::Node,
		net::{HttpMethod, Request},
		String, Vec,
	},
	Chapter, DeepLink, Filter, FilterType, Listing, Manga, MangaPageResult,
	MangaStatus, MangaViewer, Page,
};

extern crate alloc;

const BASE_URL: &str = "https://3asq.org";

fn get_image_url(obj: Node) -> String {
	let mut img = obj.attr("data-src").read();
	if img.is_empty() {
		img = obj.attr("data-lazy-src").read();
	}
	if img.is_empty() {
		img = obj.attr("src").read();
	}
	if img.is_empty() {
		img = obj.attr("srcset").read();
	}
	String::from(img.trim())
}

fn extract_id_from_url(url: &str) -> String {
	let stripped = url
		.replace(BASE_URL, "")
		.replace("/manga/", "")
		.replace('/', "");
	stripped
}

fn get_listing_page(order: &str, page: i32) -> Result<MangaPageResult> {
	let url = alloc::format!(
		"{}/page/{}?s&post_type=wp-manga&m_orderby={}",
		BASE_URL, page, order
	);

	let html = Request::get(&url)
		.header("Cookie", "wpmanga-adault=1")
		.html()?;
	let mut manga: Vec<Manga> = Vec::new();
	let mut has_more = false;

	for item in html.select("div.c-tabs-item__content").array() {
		let obj = item.as_node().expect("node array");

		let id = extract_id_from_url(&obj.select("a").attr("href").read());
		let title = obj.select("a").attr("title").read();
		let cover = get_image_url(obj.select("img"));

		if !id.is_empty() {
			manga.push(Manga {
				id,
				cover,
				title,
				..Default::default()
			});
			has_more = true;
		}
	}

	Ok(MangaPageResult { manga, has_more })
}

#[get_manga_list]
fn get_manga_list(filters: Vec<Filter>, page: i32) -> Result<MangaPageResult> {
	let mut search_string = String::new();
	let mut is_searching = false;

	for filter in filters {
		match filter.kind {
			FilterType::Title => {
				if let Ok(filter_value) = filter.value.as_string() {
					search_string.push_str(&filter_value.read().to_lowercase());
					is_searching = true;
				}
			}
			_ => continue,
		}
	}

	if is_searching {
		let url = alloc::format!(
			"{}/page/{}/?s={}&post_type=wp-manga",
			BASE_URL, page, search_string
		);

		let html = Request::get(&url)
			.header("Cookie", "wpmanga-adault=1")
			.html()?;
		let mut manga: Vec<Manga> = Vec::new();
		let mut has_more = false;

		for item in html.select("div.c-tabs-item__content").array() {
			let obj = item.as_node().expect("node array");

			let id = extract_id_from_url(&obj.select("a").attr("href").read());
			let title = obj.select("a").attr("title").read();
			let cover = get_image_url(obj.select("img"));

			if !id.is_empty() {
				manga.push(Manga {
					id,
					cover,
					title,
					..Default::default()
				});
				has_more = true;
			}
		}

		Ok(MangaPageResult { manga, has_more })
	} else {
		get_listing_page("latest", page)
	}
}

#[get_manga_listing]
fn get_manga_listing(listing: Listing, page: i32) -> Result<MangaPageResult> {
	let order = match listing.name.as_str() {
		"Popular" => "views",
		"Trending" => "trending",
		_ => "latest",
	};
	get_listing_page(order, page)
}

#[get_manga_details]
fn get_manga_details(id: String) -> Result<Manga> {
	let url = alloc::format!("{}/manga/{}", BASE_URL, id);
	let html = Request::get(&url).html()?;

	// Title — strip badge text like "HOT", "3asq", etc.
	let title_badges = html.select("span.manga-title-badges").text().read();
	let mut title = html.select("div.post-title h1").text().read();
	if title.contains(&title_badges) && !title_badges.is_empty() {
		title = title.replace(&title_badges, "");
		title = String::from(title.trim());
	}

	let cover = get_image_url(html.select("div.summary_image img"));
	let author = html.select("div.author-content a").text().read();
	let artist = html.select("div.artist-content a").text().read();
	let description = html.select("div.manga-excerpt p").text().read();

	let mut categories: Vec<String> = Vec::new();
	for item in html.select("div.genres-content > a").array() {
		categories.push(item.as_node().expect("node array").text().read());
	}

	let status_str = html
		.select("div.post-content_item:contains(الحالة) div.summary-content")
		.text()
		.read()
		.to_lowercase();
	let status = if status_str.contains("مستمرة") || status_str.contains("ongoing") {
		MangaStatus::Ongoing
	} else if status_str.contains("مكتملة") || status_str.contains("completed") {
		MangaStatus::Completed
	} else if status_str.contains("متوقفة") || status_str.contains("hiatus") || status_str.contains("on hold") {
		MangaStatus::Hiatus
	} else if status_str.contains("ملغاة") || status_str.contains("canceled") {
		MangaStatus::Cancelled
	} else {
		MangaStatus::Unknown
	};

	Ok(Manga {
		id,
		cover,
		title,
		author,
		artist,
		description,
		url,
		categories,
		status,
		viewer: MangaViewer::Rtl,
		..Default::default()
	})
}

#[get_chapter_list]
fn get_chapter_list(id: String) -> Result<Vec<Chapter>> {
	let url = alloc::format!("{}/manga/{}/ajax/chapters", BASE_URL, id);

	let html = Request::new(&url, HttpMethod::Post)
		.header("Referer", BASE_URL)
		.header("Content-Type", "application/x-www-form-urlencoded")
		.html()?;

	let mut chapters: Vec<Chapter> = Vec::new();
	for item in html.select("li.wp-manga-chapter").array() {
		let obj = item.as_node().expect("node array");

		let ch_url = obj.select("a").attr("href").read();
		let ch_id = ch_url
			.replace(BASE_URL, "")
			.replacen("/manga/", "", 1);

		let ch_text = obj.select("a").text().read();
		let mut ch_title = String::new();
		if ch_text.contains('-') {
			if let Some(pos) = ch_text.find('-') {
				ch_title.push_str(ch_text[pos + 1..].trim());
			}
		}

		// Parse chapter number from URL slug
		let slash_vec = ch_id.split('/').collect::<Vec<&str>>();
		let chapter_slug = if slash_vec.len() >= 2 {
			slash_vec[slash_vec.len() - 2]
		} else {
			&ch_id
		};

		let dash_vec: Vec<&str> = chapter_slug.split('-').collect();
		let mut chapter = 0.0_f32;
		let mut found_num = false;
		for part in &dash_vec {
			let val = part.parse::<f32>().unwrap_or(-1.0);
			if val != -1.0 {
				if found_num {
					chapter += val / 10.0;
					break;
				} else {
					chapter = val;
					found_num = true;
				}
			}
		}

		chapters.push(Chapter {
			id: ch_id,
			title: ch_title,
			volume: -1.0,
			chapter,
			date_updated: -1.0,
			scanlator: String::new(),
			url: ch_url,
			lang: String::from("ar"),
		});
	}

	Ok(chapters)
}

#[get_page_list]
fn get_page_list(_manga_id: String, chapter_id: String) -> Result<Vec<Page>> {
	let url = alloc::format!("{}/{}", BASE_URL, chapter_id);
	let html = Request::get(&url).html()?;

	let mut pages: Vec<Page> = Vec::new();
	for (index, item) in html.select("div.page-break img").array().enumerate() {
		let obj = item.as_node().expect("node array");
		let img_url = get_image_url(obj);
		if !img_url.is_empty() {
			pages.push(Page {
				index: index as i32,
				url: img_url,
				..Default::default()
			});
		}
	}

	Ok(pages)
}

#[handle_url]
fn handle_url(url: String) -> Result<DeepLink> {
	let path = url.replace(BASE_URL, "");
	let parts: Vec<&str> = path.trim_matches('/').split('/').collect();

	let mut manga_id = String::new();
	if parts.len() >= 2 && parts[0] == "manga" {
		manga_id = String::from(parts[1]);
	}

	Ok(DeepLink {
		manga: Some(get_manga_details(manga_id)?),
		chapter: None,
	})
}
