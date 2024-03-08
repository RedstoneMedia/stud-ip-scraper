use std::any::Any;
use std::sync::Arc;
use anyhow::{bail, Context};
use chrono::{NaiveDateTime};
use reqwest::Url;
use scraper::{Element, ElementRef, Html, Selector};
use crate::course_modules::{CourseModule, CourseModuleData};
use crate::common_data::User;

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
        let mut tables_members = html.select(&table_selector).map(parse_member_table);
        Ok(CourseMembers {
            lecturers: tables_members.next().context("No lectures table found")?,
            tutors: tables_members.next().context("No tutors table found")?,
            students: tables_members.next().context("No students table found")?,
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
                    group.enables_entry_at = Some(date);
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
        Ok(parse_member_table(html.root_element()))
    }

}

/// The members of a course
#[derive(Debug, Clone)]
pub struct CourseMembers {
    pub lecturers: Vec<User>,
    pub tutors: Vec<User>,
    pub students: Vec<User>
}

/// A group of members of a specific course
#[derive(Debug, Clone)]
pub struct Group {
    pub name: String,
    pub id: String,
    pub entered: bool,
    pub enables_entry_at: Option<NaiveDateTime>,
    pub members: usize,
    pub max_members: usize
}

fn parse_member_table(table_ref: ElementRef) -> Vec<User> {
    let rows_selector = Selector::parse("tbody tr").unwrap();
    let main_a_selector = Selector::parse("td a").unwrap();
    let img_selector = Selector::parse("img").unwrap();
    table_ref.select(&rows_selector).filter_map(|row| {
        let main_a_ref = row.select(&main_a_selector).next()?;
        let main_a = main_a_ref.value();
        let profile_url = Url::parse(main_a.attr("href")?).ok()?;
        let username = profile_url.query_pairs()
            .find_map(|(key, value)| (key == "username")
                .then(|| value.to_string())
            )?;
        let avatar_img = main_a_ref.select(&img_selector).next()?.value();
        let avatar_src = avatar_img.attr("src")?;
        let display_name = main_a_ref.text().collect::<String>().trim().to_string();
        Some(User {
            display_name,
            username,
            avatar_src: Some(avatar_src.to_string()),
        })
    }).collect::<Vec<_>>()
}