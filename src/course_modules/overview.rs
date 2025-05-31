use std::any::Any;
use std::rc::Rc;
use anyhow::Context;
use itertools::Itertools;
use log::warn;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use crate::course_modules::{CourseModule, CourseModuleData};
use crate::institute::{parse_institute_link, Institute};
use crate::news::{parse_news_box, NewsArticle};
use crate::ref_source::ReferenceSource;
use crate::translations::local_to_key;

const OVERVIEW_MODULE_URL : &str = "https://studip.example.com/dispatch.php/course/overview";
const DETAILS_URL : &str = "https://studip.example.com/dispatch.php/course/details";

#[derive(Debug)]
pub struct OverviewModule {
    module_data: Rc<CourseModuleData>
}

impl CourseModule for OverviewModule {
    fn new(data: Rc<CourseModuleData>) -> Self {
        Self {
            module_data: data
        }
    }

    fn name() -> &'static str {
        "main"
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Various details about the [`crate::course::Course`]
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CourseDetails {
    pub name: String,
    pub subtitle: String,
    pub course_number: Option<u64>,
    pub semester: String,
    pub participants: u64,
    pub home_institute: Option<Institute>,
    pub participating_institutes: Vec<Institute>,
    pub course_type: String,
    pub first_date: Option<String>,
    pub type_form: String,
    pub language: Option<String>,
    pub sws: Option<u32>,
    pub ects_points: Option<u32>,
    pub description: String
}

impl OverviewModule {

    /// Queries the news articles on the overview page of the course
    pub fn get_announcements(&self) -> anyhow::Result<Vec<NewsArticle>> {
        let response = self.module_data.client.get(OVERVIEW_MODULE_URL)
            .query(&[("cid", &self.module_data.course_id)])
            .send()?;
        let html = Html::parse_document(&response.text()?);

        let announcements_selector = Selector::parse("#content article.studip").unwrap();
        let news_header_selector = Selector::parse("header > h1 > .icon-shape-news").unwrap();
        let Some(news_box) = html.select(&announcements_selector).next() else {
            return Ok(vec![]);
        };
        // Check if it is actually a news box (And not a schedule box)
        if news_box.select(&news_header_selector).next().is_none() {
            return Ok(vec![]);
        }
        let news = parse_news_box(news_box, &ReferenceSource::Course(self.module_data.course_id.clone()))?;
        Ok(news)
    }

    /// Queries various data points about the points, that are visible on the detailed overview page.
    pub fn get_course_details(&self) -> anyhow::Result<CourseDetails> {
        let response = self.module_data.client.get(DETAILS_URL)
            .query(&[("cid", &self.module_data.course_id)])
            .send()?;
        let html = Html::parse_document(&response.text()?);
        let mut course_details = CourseDetails::default();
        // Parse that big table
        let info_table_cell_selector = Selector::parse("#tablefix table tbody td").unwrap();
        let a_tag_selector = Selector::parse("a").unwrap();
        for (key_elem, value_elem) in html.select(&info_table_cell_selector).tuples() {
            let local = key_elem.text().collect::<String>();
            let key = local_to_key(&local);
            let text_value = value_elem.text().collect::<String>().trim().to_string();
            match key {
                "COURSE_NAME" => course_details.name = text_value,
                "SUBTITLE" => course_details.subtitle = text_value,
                "COURSE_NUMBER" => course_details.course_number = Some(text_value.parse().context("Could not parse course number")?),
                "SEMESTER" => course_details.semester = text_value,
                "NUMBER_OF_PARTICIPANTS" => course_details.participants = text_value.parse().context("Could not parse number of participants")?,
                "HOME_INSTITUTE" => {
                    let link_elem = value_elem.select(&a_tag_selector).next().context("Expected home institute link")?;
                    course_details.home_institute = Some(parse_institute_link(link_elem).context("Could not parse institute link")?);
                },
                "PARTICIPATING_INSTITUTES" => {
                    for link_elem in value_elem.select(&a_tag_selector) {
                        let institute = parse_institute_link(link_elem).context("Could not parse institute link")?;
                        course_details.participating_institutes.push(institute);
                    }
                },
                "COURSE_TYPE" => course_details.course_type = text_value,
                "FIRST_DATE" => course_details.first_date = Some(text_value),
                "TYPE_FORM" => course_details.type_form = text_value,
                "LANGUAGE" => course_details.language = Some(text_value),
                "SWS" => course_details.sws = Some(text_value.parse().context("Could not parse SWS")?),
                "ECTS_POINTS" => course_details.ects_points = Some(text_value.parse().context("Could not parse ECTS points")?),
                _ => {
                    warn!("Unknown course detail key: {:?}", key);
                }
            }
        }
        // Parse description, which is the only formated content article on the page (and we pray that it will stay tha way)
        let formatted_contend_selector = Selector::parse("#content article.studip .formatted-content").unwrap();
        if let Some(description_element) = html.select(&formatted_contend_selector).next() {
            course_details.description = description_element.inner_html();
        }
        Ok(course_details)
    }

}