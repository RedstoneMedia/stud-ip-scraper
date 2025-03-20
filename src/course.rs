use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Context;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use crate::course_modules::{COURSE_MODULE_REGISTRY, CourseModule, CourseModuleData, register_default_course_modules, REGISTERED_DEFAULT_COURSE_MODULES};
use crate::StudIpClient;

const MY_COURSES_URL: &str = "https://studip.example.com/dispatch.php/my_courses";
pub(crate) const COURSE_URL: &str = "https://studip.example.com/dispatch.php/course";
const MODULES_QUERY_URL : &str = "https://studip.example.com/seminar_main.php";

/// Represents a course and it's modules \
/// A singular module can be accessed, by type with the [get_module()](Course::get_module) function.
#[derive(Serialize, Deserialize, Debug)]
pub struct Course {
    // All sorts of data
    /// The course ID
    pub id: String,
    /// The courses Name
    pub name: String,
    #[serde(rename = "number")]
    _number: String, // No Idea what this is for
    /// The group index in which the current user has added this course \
    /// Corresponds to the `groups` filed of the [`MyCourses`] struct
    pub group: usize,
    /// The children of this course
    pub children: Vec<serde_json::Value>,
    /// The parent of this course
    pub parent: Option<serde_json::Value>,
    /// The icon url of the course
    #[serde(rename = "avatar")]
    pub icon_url: String,
    /// Navigation items of the course
    pub navigation: Vec<serde_json::Value>,
    // Flags
    /// If the admission to this course is binding
    pub admission_binding: bool,
    /// If you're a teacher of this course?
    pub is_teacher: bool,
    /// If the course is a study group
    pub is_studygroup: bool,
    /// If the course is hidden
    pub is_hidden: bool,
    /// If you are a "deputy" or moderator for this course.
    pub is_deputy: bool,
    /// If this course is a group
    pub is_group: bool,
    /// If there is extra navigation?
    pub extra_navigation: bool,

    // Custom data
    #[serde(skip)]
    /// The modules of the course \
    /// Needs to be queried with [`Course::query_modules()`]
    pub modules: Vec<Box<dyn CourseModule>>,
    #[serde(skip)]
    client: Arc<StudIpClient>
}

impl Course {


    /// Queries the available modules for this course and stores them in the `modules` field. \
    /// *Note: This is not done automatically*
    pub fn query_modules(&mut self) -> anyhow::Result<()> {
        REGISTERED_DEFAULT_COURSE_MODULES.get_or_init(register_default_course_modules);
        let module_reg = COURSE_MODULE_REGISTRY.lock().unwrap();
        let response = self.client.get(MODULES_QUERY_URL)
            .query(&[("auswahl", &self.id)])
            .send()?;
        let html = Html::parse_document(&response.text().unwrap());
        let tabs_selector = Selector::parse("#tabs li").unwrap();
        let module_data = Arc::new(CourseModuleData {
            course_id: self.id.clone(),
            client: self.client.clone(),
        });
        self.modules = html.select(&tabs_selector).filter_map(|tab_ref| {
            let tab = tab_ref.value();
            let module_name = tab.id().unwrap().replace("nav_course_", "");
            module_reg.get(module_name.as_str())
                .map(|module_constructor| module_constructor(module_data.clone()))
        }).collect();
        Ok(())
    }

    /// Gets a downcasted [`CourseModule`], by its Type on a [Course](crate::course::Course)
    pub fn get_module<Module: CourseModule>(&mut self) -> Option<&mut Module> {
        self.modules.iter_mut().find_map(|module| module.as_any().downcast_mut::<Module>())
    }

}

/// Contains all the courses, and some addition data, of the current user
#[derive(Serialize, Deserialize, Debug)]
pub struct MyCourses {
    #[serde(rename = "setCourses")]
    pub courses: HashMap<String, Course>,
    #[serde(rename = "setGroups")]
    pub groups: Vec<serde_json::Value>,
    #[serde(rename = "setUserId")]
    pub user_id: String,
    #[serde(rename = "setConfig")]
    pub config: HashMap<String, serde_json::Value>,
    #[serde(skip)]
    client: Arc<StudIpClient>
}

impl MyCourses {

    pub(crate) fn from_client(client: Arc<StudIpClient>) -> Self {
        Self {
            courses: Default::default(),
            groups: Default::default(),
            user_id: Default::default(),
            config: Default::default(),
            client,
        }
    }

    /// Queries the available courses of the current user. \
    /// *Note: This is not done automatically*
    pub fn query(&mut self) -> anyhow::Result<()> {
        // Find MyCoursesData json in html
        let r = self.client.get(MY_COURSES_URL).send()?;
        let text = r.text().context("failed to get response text")?;
        let html = Html::parse_document(&text);
        // I LOVE JAVASCRIPT! HAHAHHAH
        let script_tag_selector = Selector::parse("script#vue-vuex-store-data-mycourses").unwrap();
        let script = html.select(&script_tag_selector).next().context("Expected MyCoursesData to be present in html")?;
        let script_string = script.inner_html();
        // Parse MyCoursersData
        let mut new_my_courses: Self = serde_json::from_str(script_string.trim())
            .context("Could not parse MyCoursesData")?;
        // Copy api handle to courses
        for course in new_my_courses.courses.values_mut() {
            course.client = self.client.clone();
        }
        *self = new_my_courses;
        Ok(())
    }

    /// Finds a course, give its name. Returns an immutable reference to it
    pub fn get_course_by_name(&self, name: &str) -> Option<&Course> {
        self.courses.iter()
            .find(|(_, course)| course.name == name)
            .map(|(_, course)| course)
    }

    /// Finds a course, give its name. Returns a mutable reference to it
    pub fn mut_course_by_name(&mut self, name: &str) -> Option<&mut Course> {
        self.courses.iter_mut()
            .find(|(_, course)| course.name == name)
            .map(|(_, course)| course)
    }

}