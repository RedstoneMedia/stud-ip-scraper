use anyhow::{anyhow, Context};
use chrono::NaiveDate;
use scraper::{Element, ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use url::Url;
use crate::StudIpClient;
use crate::user::{parse_simple_user, User};
use crate::ref_source::ReferenceSource;

/// A comment below a news article \
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsComment {
    /// The id of the comment
    pub id: String,
    /// The author of the comment
    pub author: User,
    /// The raw html content of the comment
    pub html_content: String,
    /// This is localized. Example: "vor 3 Tagen"
    pub time_since_string: String,
}

impl PartialEq for NewsComment {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

/// A news article, found in a news box \
/// Can be parsed with [parse_news_box()](parse_news_box())
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsArticle {
    /// The id of the article
    pub id: String,
    /// Extra information about the articles source
    pub source: ReferenceSource,
    /// The title of the article
    pub title: String,
    /// The raw html content of the article
    pub html_content: String,
    /// The author of the article
    pub author: User,
    /// The creation date of the article
    pub date: NaiveDate,
    /// How many times the article has been visited
    pub visits: usize,
    /// The number of comments of the article
    pub n_comments: usize,
    /// If the article was viewed for first time
    pub is_new: bool,
    /// The comments of the article \
    /// Is only filled by calling [NewsArticle::query_comments()](NewsArticle::query_comments())
    pub comments: Vec<NewsComment>
}

impl NewsArticle {

    /// Queries the comments of the news article \
    /// *Note: This is not done automatically*
    pub fn query_comments(&mut self, stud_ip_client: &StudIpClient) -> anyhow::Result<()> {
        // Make request to open comment content box
        let mut url : Url = (&self.source).try_into()?;
        url.set_fragment(Some(&self.id));
        let response = stud_ip_client.get(url)
            .query(&[("comments", "1"), ("contentbox_open", &self.id)])
            .send()?;
        // Find article by id in html
        let html = Html::parse_document(&response.text()?);
        let comment_elements = Selector::parse(&format!("article[id=\"{}\"] .comments .comment", self.id))
            .map_err(|_| anyhow!("Failed to parse comments selector"))?;
        // Parse comments
        let time_selector = Selector::parse("time").unwrap();
        let author_selector = Selector::parse("h1 > a").unwrap();
        let content_selector = Selector::parse(".formatted-content").unwrap();
        for comment_element in html.select(&comment_elements) {
            let mut new_comments = vec![];
            let comment_id = comment_element.attr("id")
                .context("Expected comment id")?
                .replace("newscomment-", "");
            // We can't easily parse this into a date, because it looks could look like this: "vor einem Monat"
            // This is localized and hard to parse
            let time_since_string = comment_element.select(&time_selector)
                .next()
                .context("Expected comment time")?
                .text()
                .collect::<String>()
                .trim()
                .to_string();
            let author_link = comment_element.select(&author_selector)
                .next()
                .context("Expected comment author a tag")?;
            let author = parse_simple_user(author_link)?;
            let content_html = comment_element.select(&content_selector)
                .next()
                .context("Expected comment content")?
                .inner_html();
            let comment = NewsComment {
                id: comment_id,
                author,
                html_content: content_html,
                time_since_string
            };
            // Overwrite comments with new comments
            new_comments.push(comment);
            if !new_comments.is_empty() {
                self.n_comments = new_comments.len();
                self.comments = new_comments;
            }
        }
        Ok(())
    }

}

impl PartialEq for NewsArticle {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

/// Parse a news box into a list of [news articles](NewsArticle) \
/// These boxes appear all over the site, including on profile pages, start page and courses pages
pub fn parse_news_box(element: ElementRef, reference_source: &ReferenceSource) -> anyhow::Result<Vec<NewsArticle>> {
    let articles_selector = Selector::parse("article[id].studip").unwrap();
    let title_selector = Selector::parse("header h1").unwrap();
    let news_author_selector = Selector::parse("header .news_user").unwrap();
    let news_creation_date_selector = Selector::parse("header .news_date").unwrap();
    let news_visits_selector = Selector::parse("header .news_visits").unwrap();
    let news_n_comments_selector = Selector::parse("header .news_comments_indicator").unwrap();
    let news_content_selector = Selector::parse("section > article .formatted-content").unwrap();
    let mut news_articles = vec![];
    for article_elem in element.select(&articles_selector) {
        // Parse header
        let article_id = article_elem.attr("id").unwrap().to_string();
        let title = article_elem.select(&title_selector)
            .next()
            .context("Expected news title")?
            .text()
            .collect::<String>()
            .trim()
            .to_string();
        let author_elem = article_elem.select(&news_author_selector)
            .next()
            .context("Expected news author")?;
        let author = parse_simple_user(author_elem)?;
        let news_date_string = article_elem.select(&news_creation_date_selector)
            .next()
            .context("Expected news creation date")?
            .text()
            .collect::<String>()
            .trim()
            .to_string();
        let news_date = NaiveDate::parse_from_str(&news_date_string, "%d.%m.%Y")
            .or_else(|_| NaiveDate::parse_from_str(&news_date_string, "%d/%m/%Y"))?;
        let visits: usize = article_elem.select(&news_visits_selector)
            .next()
            .context("Expected news visits")?
            .text()
            .collect::<String>()
            .trim()
            .replace('.', "")
            .parse()?;
        let n_comments: usize = article_elem.select(&news_n_comments_selector).next().and_then(|e| e.text()
            .collect::<String>()
            .trim()
            .replace('.', "")
            .parse()
            .ok()
        ).unwrap_or(0);
        let is_new = article_elem.has_class(&"new".into(), scraper::CaseSensitivity::CaseSensitive);
        // Parse content
        let content_html = article_elem.select(&news_content_selector)
            .next()
            .context("Expected news content")?
            .inner_html();
        news_articles.push(NewsArticle {
            id: article_id,
            source: reference_source.clone(),
            title,
            html_content: content_html,
            author,
            date: news_date,
            visits,
            n_comments,
            is_new,
            comments: vec![],
        });
    }
    Ok(news_articles)
}