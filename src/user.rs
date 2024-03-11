use std::collections::HashMap;
use anyhow::{anyhow, Context};
use itertools::Itertools;
use reqwest::IntoUrl;
use scraper::{Element, ElementRef, Html, Selector};
use scraper::selectable::Selectable;
use serde::{Deserialize, Serialize};
use url::Url;
use crate::institute::Institute;
use crate::news::{NewsArticle, parse_news_box};
use crate::questionnaire::{parse_questionnaire, Questionnaire};
use crate::ref_source::ReferenceSource;
use crate::StudIpClient;

pub(crate) const PROFILE_URL: &str = "https://studip.example.com/dispatch.php/profile";

/// Stores basic information about a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub display_name: String,
    pub username: String,
    pub avatar_src: Option<String>,
    pub source: ReferenceSource
}

/// A linked [`Institute`] on a profile \
/// Allows to infer affiliation of the user with the {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileInstituteData {
    /// The institute on the profile
    pub institute: Institute,
    /// Extra affiliation data
    pub extra_data: HashMap<String, String>,
    /// Flag affiliation data
    pub sub_flags: Vec<String>
}

/// A category on a profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileCategory {
    pub name: String,
    pub html_content: String,
}

/// The profile of a user \
/// Contains allot more information about the user then [`User`]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub display_name: String,
    pub username: String,
    pub avatar_src: String,
    pub visits: usize,
    pub points: Option<usize>,
    pub rank: Option<String>,
    pub email: Option<String>,
    pub mobile_phone_number: Option<String>,
    pub home_telephone_number: Option<String>,
    pub address: Option<String>,
    pub motto: Option<String>,
    pub homepage: Option<String>,
    pub study_institutes: Vec<ProfileInstituteData>,
    pub work_institute: Vec<ProfileInstituteData>,
    pub news: Vec<NewsArticle>,
    pub questionnaires: Vec<Questionnaire>,
    pub categories: Vec<ProfileCategory>
}

impl User {

    /// Queries a bunch of data about the [`User`], by parsing the profile page.
    pub fn query_profile(&self, stud_ip_client: &StudIpClient) -> anyhow::Result<Profile> {
        // Make request to profile
        let mut query_params = vec![("username", self.username.as_str())];
        query_params.extend(self.source.get_additional_query_params());
        let response = stud_ip_client.get(PROFILE_URL)
            .query(&query_params)
            .send()?;
        let response_text = response.text()?;

        // Grab base profile information
        let html = Html::parse_document(&response_text);
        // Parse avatar src
        let avatar_src_selector = Selector::parse("#sidebar .avatar-widget img").unwrap();
        let avatar_src = html.select(&avatar_src_selector)
            .next()
            .context("Expected avatar image")?
            .attr("src")
            .unwrap()
            .trim()
            .to_string();
        // Parse display name
        let display_name_selector = Selector::parse("#sidebar .sidebar-widget-header").unwrap();
        let display_name = html.select(&display_name_selector)
            .next()
            .context("Expected display name")?
            .text()
            .collect::<String>()
            .trim()
            .to_string();

        // Parse profile visits points and rank
        let key_value_regex = regex::Regex::new(r"(?m)^ *(?P<key>.+):\s*(?P<value>[._\- 0-9\w]+?) *$").unwrap();
        let minor_details_selector = Selector::parse("#sidebar .profile-sidebar-details .minor").unwrap();
        let mut minor_details = html.select(&minor_details_selector);
        // Profile visits
        let profile_visits_str = minor_details.next()
            .context("Expected profile visits")?
            .text()
            .collect::<String>()
            .trim()
            .to_string();
        let profile_visits_captures = key_value_regex.captures(&profile_visits_str)
            .context("Could not capture profile visits")?;
        let profile_visits : usize  = profile_visits_captures.name("value")
            .context("Expected profile visits capture")?
            .as_str()
            .replace('.', "")
            .parse()?;
        // Construct base profile, with only the required fields first
        let mut profile = Profile {
            display_name,
            username: self.username.clone(),
            avatar_src,
            visits: profile_visits,
            points: None,
            rank: None,
            email: None,
            mobile_phone_number: None,
            home_telephone_number: None,
            address: None,
            motto: None,
            homepage: None,
            study_institutes: vec![],
            work_institute: vec![],
            news: vec![],
            questionnaires: vec![],
            categories: vec![],
        };

        // Fill optional fields

        // Points and rank
        let source = ReferenceSource::Profile(self.username.clone());
        if let Some(element) = minor_details.next() {
            let rank_data = element.text().collect::<String>()
                .trim()
                .to_string();
            let captures : [_; 2]  = key_value_regex
                .captures_iter(&rank_data)
                .collect_vec()
                .try_into()
                .map_err(|_| anyhow!("Expected 2 captures"))?;
            profile.points = Some(captures[0].name("value")
                .context("Expected points")?
                .as_str()
                .replace('.', "")
                .parse()?
            );
            profile.rank = Some(captures[1].name("value")
                .context("Expected rank name")?
                .as_str()
                .to_string()
            )
        }

        // Motto
        let motto_selector = Selector::parse("#sidebar .sidebar-widget:nth-last-child(1)").unwrap();
        if let Some(motto_widget) = html.select(&motto_selector).next() {
            let header_selector = Selector::parse(".sidebar-widget-header").unwrap();
            let header_text = motto_widget.select(&header_selector)
                .next()
                .context("Expected widget header")?
                .text()
                .collect::<String>()
                .to_lowercase();
            if header_text.contains("motto") {
                let header_selector = Selector::parse(".sidebar-widget-content").unwrap();
                profile.motto = Some(motto_widget.select(&header_selector)
                    .next()
                    .context("Expected motto content")?
                    .text()
                    .collect::<String>()
                    .trim()
                    .to_string()
                );
            }
        }

        // General info
        let general_info_selector = Selector::parse("#content .contentbox section dl").unwrap();
        let general_info_elem = html.select(&general_info_selector).next()
            .context("Expected general information content box")?;
        let dt_dd_selector = Selector::parse("dt, dd").unwrap();
        for (key_elem, value_elem) in general_info_elem.select(&dt_dd_selector).tuples() {
            let key = key_elem.text().collect::<String>().trim().to_string().to_lowercase();
            if key.contains("e-mail") {
                profile.email = Some(value_elem.text().collect::<String>().trim().to_string());
            } else if key.contains("home telephone number") || key.contains("telefon (privat)") {
                profile.home_telephone_number = Some(value_elem.text().collect::<String>().trim().to_string());
            } else if key.contains("mobile telephone") || key.contains("mobiltelefon") {
                profile.mobile_phone_number = Some(value_elem.text().collect::<String>().trim().to_string());
            } else if key.contains("address") {
                profile.address = Some(value_elem.text().collect::<String>().trim().to_string());
            } else if key.contains("homepage") {
                profile.homepage = Some(value_elem.text().collect::<String>().trim().to_string());
            } else if key.contains("work") || key.contains("arbeite") {
                profile.work_institute = parse_profile_institutes(value_elem)?;
            } else if key.contains("study") || key.contains("studiere") {
                profile.study_institutes = parse_profile_institutes(value_elem)?;
            }
        }

        // News
        let article_selector = Selector::parse("#content > article.studip:not([id])").unwrap();
        let news_header_selector = Selector::parse("header .icon-shape-news").unwrap();
        let news_elem = html.select(&article_selector)
            .find(|elem| elem.select(&news_header_selector).next().is_some());
        if let Some(news_elem) = news_elem {
            profile.news = parse_news_box(news_elem, &source)?;
        }

        // Questionnaires
        let questionnaire_selector = Selector::parse("#questionnaire_area > article[data-questionnaire_id]").unwrap();
        for questionnaire_elem in html.select(&questionnaire_selector) {
            profile.questionnaires.push(parse_questionnaire(questionnaire_elem, source.clone())?);
        }

        // User custom categories
        let custom_category_abort_selector = Selector::parse("nav").unwrap();
        let article_header_selector = Selector::parse("#content > article.studip:not([id]) > header").unwrap();
        // Find articles, which headers descendants don't contain the abort selector (nav)
        let category_elements = html.select(&article_header_selector)
            .filter(|elem| elem.select(&custom_category_abort_selector).next().is_none())
            .map(|elem| elem.parent_element().unwrap());
        let category_name_selector = Selector::parse("header > h1").unwrap();
        let category_content_selector = Selector::parse("section .formatted-content").unwrap();
        for category_elem in category_elements {
            let name = category_elem
                .select(&category_name_selector)
                .next()
                .context("Expected category name")?
                .text()
                .collect::<String>()
                .trim()
                .to_string();
            let content = category_elem
                .select(&category_content_selector)
                .next()
                .context("Expected category content")?
                .inner_html();
            profile.categories.push(ProfileCategory { name, html_content: content });
        }

        Ok(profile)
    }

}

/// Parses the username from a url
pub fn get_username_from_url(user_url: impl IntoUrl) -> anyhow::Result<String> {
    let user_url = user_url.into_url()?;
    user_url.query_pairs()
        .find_map(|(key, value)| (key == "username").then(|| value.to_string()))
        .context("Expected username in user href")
}

/// Parses the username from a link (a tag) element
pub fn get_username_from_link_element(link_element: ElementRef) -> anyhow::Result<String> {
    let user_url = Url::parse(link_element.attr("href")
        .context("Expected user link href")?)?;
    get_username_from_url(user_url)
}

/// Parses a [`User`] from html. \
/// Accepts html in this format: <a href="https://studip.example.com/something?username={some-username}">display name</a>
pub fn parse_simple_user(link_element: ElementRef) -> anyhow::Result<User> {
    let display_name = link_element.text()
        .collect::<String>()
        .trim()
        .to_string();
    let username = get_username_from_link_element(link_element)?;
    Ok(User {
        display_name,
        username,
        avatar_src: None,
        source: ReferenceSource::Unspecified,
    })
}

// Helper function to parse profile institutes
fn parse_profile_institutes(element: ElementRef) -> anyhow::Result<Vec<ProfileInstituteData>> {
    let mut institutes = vec![];
    let list_item_selector = Selector::parse("li").unwrap();
    let a_tag_selector = Selector::parse("a").unwrap();
    let sub_flags_selector = Selector::parse("table td:nth-last-child(1)").unwrap();
    let strong_selector = Selector::parse("strong").unwrap();
    for profile_institute_elem in element.select(&list_item_selector) {
        let institute_link_elem = profile_institute_elem.select(&a_tag_selector)
            .next()
            .context("Expected institute link")?;
        let institute_name = institute_link_elem.text()
            .collect::<String>()
            .trim()
            .to_string();
        let institute_url = Url::parse(institute_link_elem.attr("href")
            .context("Expected institute href")?
        )?;
        let institute_id = institute_url.query_pairs()
            .find_map(|(key, value)| (key == "auswahl" || key == "selection")
            .then(|| value.to_string()))
            .context("Expected institute id")?;

        let sub_flags: Vec<String> = profile_institute_elem.select(&sub_flags_selector)
            .map(|sub_flag_elem| sub_flag_elem.text()
                .collect::<String>()
                .trim()
                .to_string()
            )
            .collect();

        // Get extra data, the format in the html is this: <strong>key:</strong> then a number of simple text elements
        let extra_data = profile_institute_elem.select(&strong_selector).map(|key_elem| {
            let mut value_text = String::new();
            for sibling in key_elem.next_siblings() {
                let sibling_value = sibling.value();
                let Some(sibling_text) = sibling_value.as_text() else {continue};
                value_text.push_str(sibling_text.trim());
                value_text.push('\n');
            }
            let key_text = key_elem.text()
                .collect::<String>()
                .trim()
                .trim_end_matches(':')
                .to_string();
            (key_text, value_text.trim().to_string())
        }).collect();

        institutes.push(ProfileInstituteData {
            institute: Institute {
                name: institute_name,
                id: institute_id
            },
            sub_flags,
            extra_data
        });
    }
    Ok(institutes)
}