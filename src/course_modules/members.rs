use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use anyhow::bail;
use chrono::{DateTime, NaiveDateTime, Utc};
use chrono::serde::ts_seconds;
use reqwest::Url;
use scraper::{Element, ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use crate::course_modules::{CourseModule, CourseModuleData};
use crate::user::{get_username_from_link_element, User};
use crate::ref_source::ReferenceSource;

const MEMBERS_URL : &str = "https://studip.example.com/dispatch.php/course/members";
const GROUPS_URL : &str = "https://studip.example.com/dispatch.php/course/statusgroups";

/// Module, that enables querying the members of a course and operating on the courses groups
#[derive(Debug)]
pub struct MembersModule {
    course_module_data: Arc<CourseModuleData>
}

impl CourseModule for MembersModule {
    fn new(data: Arc<CourseModuleData>) -> Self {
        Self {course_module_data: data}
    }

    fn name() -> &'static str {
        "members"
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl MembersModule {

    /// Returns the members of the course. \
    /// This includes the lecturers, tutors, and students.
    pub fn get_members(&self) -> anyhow::Result<CourseMembers> {
        let response = self.course_module_data.client.get(MEMBERS_URL)
            .query(&[("cid", &self.course_module_data.course_id)])
            .send()?;
        let html = Html::parse_document(&response.text()?);
        let table_selector = Selector::parse("#content table").unwrap();
        let mut tables_members : HashMap<_, _> = html.select(&table_selector)
            .filter_map(|table| parse_member_table(table, ReferenceSource::Course(self.course_module_data.course_id.to_string())).ok())
            .collect();
        Ok(CourseMembers {
            lecturers: tables_members.remove(&Some("dozierende".to_string()))
                .or_else(|| tables_members.remove(&Some("lecturers".to_string())))
                .unwrap_or_default(),
            tutors: tables_members.remove(&Some("tutor*innen".to_string()))
                .or_else(|| tables_members.remove(&Some("tutors".to_string())))
                .unwrap_or_default(),
            students: tables_members.remove(&Some("studierende".to_string()))
                .or_else(|| tables_members.remove(&Some("students".to_string())))
                .unwrap_or_default(),
        })
    }

    /// Returns the groups within the course.
    pub fn get_groups(&self) -> anyhow::Result<Vec<Group>> {
        let response = self.course_module_data.client.get(GROUPS_URL)
            .query(&[("cid", &self.course_module_data.course_id)])
            .send()?;
        let html = Html::parse_document(&response.text()?);
        let group_selector= Selector::parse("div#content article > header").unwrap();
        let h1_selector = Selector::parse("h1").unwrap();
        let disabled_entry_selector = Selector::parse("img.icon-shape-door-enter").unwrap();
        Ok(html.select(&group_selector).map(|group_ref| {
            let raw_name = group_ref.select(&h1_selector).next()
                .unwrap()
                .text()
                .collect::<String>()
                .trim()
                .to_string();

            let name_captures = regex::Regex::new(r"(?P<name>.+) \((?P<members>\d+)(/(?P<max_members>\d+))?\)").unwrap()
                .captures(&raw_name)
                .unwrap();

            let name = name_captures.name("name").unwrap().as_str().to_string();
            let members = name_captures.name("members")
                .map(|re_match| re_match.as_str().parse().unwrap())
                .unwrap_or(0);
            let max_members = name_captures.name("max_members")
                .map(|re_match| re_match.as_str().parse().unwrap())
                .unwrap_or(0);

            let leave_selector = Selector::parse("a > img.icon-shape-door-leave").unwrap();
            let entered = group_ref.select(&leave_selector).next().is_some();

            let group_info_selector = Selector::parse("a > img.icon-shape-info-circle").unwrap();
            let id = group_ref.select(&group_info_selector)
                .next()
                .map(|elem| elem.parent_element().unwrap().value().attr("href").unwrap())
                .map(|group_info_link| {
                    let group_info_url = Url::parse(group_info_link).unwrap();
                    group_info_url.path_segments().unwrap().last().unwrap().to_string()
                })
                .unwrap_or_else(|| "nogroup".to_string());

            let mut group = Group {
                name,
                id,
                entered,
                enables_entry_at: None,
                members,
                max_members,
            };

            if let Some(disabled_entry_link) = group_ref.select(&disabled_entry_selector).next() {
                let title = disabled_entry_link.value().attr("title").unwrap();
                if let Some(re_match) = regex::Regex::new(r"\d{2}\.\d{2}\.\d{4} \d{2}:\d{2}").unwrap().find(title) {
                    let date_str = re_match.as_str();
                    let date = NaiveDateTime::parse_from_str(date_str, "%d.%m.%Y %H:%M")
                        .expect("Could not parse entry_enabled_at date time");
                    let enables_entry_at = date.and_local_timezone(chrono::Local)
                        .earliest()
                        .map(|local| local.to_utc());
                    group.enables_entry_at = enables_entry_at;
                }
            }
            group
        }).collect::<Vec<_>>())
    }

    /// Attempts to join a specifies [`Group`] within the course.
    pub fn try_join_group(&self, group: &Group) -> anyhow::Result<()> {
        let url = format!("{}/join/{}", GROUPS_URL, group.id);
        let response = self.course_module_data.client.get(url)
            .query(&[("cid", &self.course_module_data.course_id)])
            .send()?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            bail!("Could not join group. Status code: {}", status)
        }
    }

    /// Attempts to leave a specific [`Group`] within the course.
    pub fn try_leave_group(&self, group: &Group) -> anyhow::Result<()> {
        let url = format!("{}/leave/{}", GROUPS_URL, group.id);
        let response = self.course_module_data.client.get(url)
            .query(&[("cid", &self.course_module_data.course_id)])
            .send()?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            bail!("Could not leave group. Status code: {}", status)
        }
    }

    /// Returns the members of a specific [`Group`] within the course.
    pub fn get_group_members(&self, group: &Group) -> anyhow::Result<Vec<User>> {
        let url = format!("{}/getgroup/{}", GROUPS_URL, group.id);
        let response = self.course_module_data.client.get(url)
            .query(&[("cid", &self.course_module_data.course_id)])
            .header("X-Requested-With", "XMLHttpRequest")
            .send()?;
        let text = response.text()?;
        let html = Html::parse_fragment(&text);
        Ok(parse_member_table(html.root_element(), ReferenceSource::Course(self.course_module_data.course_id.to_string()))?.1)
    }

}

/// The members of a course
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseMembers {
    pub lecturers: Vec<User>,
    pub tutors: Vec<User>,
    pub students: Vec<User>
}

/// A group of members of a specific course
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    pub id: String,
    pub entered: bool,
    #[serde(with = "option_ts_seconds")]
    pub enables_entry_at: Option<DateTime<Utc>>,
    pub members: usize,
    pub max_members: usize
}

fn parse_member_table(table_ref: ElementRef, reference_source: ReferenceSource) -> anyhow::Result<(Option<String>, Vec<User>)> {
    let caption_selector = Selector::parse("caption").unwrap();
    let caption = table_ref.select(&caption_selector)
        .next()
        .map(|elem| elem.text().collect::<String>().trim().to_lowercase());
    let rows_selector = Selector::parse("tbody tr").unwrap();
    let main_a_selector = Selector::parse("td a").unwrap();
    let img_selector = Selector::parse("img").unwrap();
    Ok((caption, table_ref.select(&rows_selector).filter_map(|row| {
        let main_a_ref = row.select(&main_a_selector).next()?;
        let username = get_username_from_link_element(main_a_ref).ok()?;
        let avatar_img = main_a_ref.select(&img_selector).next()?.value();
        let avatar_src = avatar_img.attr("src")?;
        let display_name = main_a_ref.text().collect::<String>().trim().to_string();
        Some(User {
            display_name,
            username,
            avatar_src: Some(avatar_src.to_string()),
            source: reference_source.clone(),
        })
    }).collect::<Vec<_>>()))
}

pub mod option_ts_seconds {
    use serde::{Deserializer, Serializer};
    use super::*;

    pub fn serialize<S>(date: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
    {
        match date {
            Some(date) => ts_seconds::serialize(date, serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
        where
            D: Deserializer<'de>,
    {
        Ok(Option::deserialize(deserializer)?.and_then(|s: i64| {
            DateTime::from_timestamp(s, 0)
        }))
    }
}
