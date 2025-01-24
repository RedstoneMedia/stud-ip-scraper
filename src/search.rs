use anyhow::{anyhow, bail, Context};
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize, Serializer};
use serde::ser::SerializeMap;
use serde_json::Value;
use crate::institute::Institute;
use crate::ref_source::ReferenceSource;
use crate::StudIpClient;
use crate::user::{get_username_from_url, User};

/// The different ways in witch a Semester can be filtered in the search
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub enum FilterSemester {
    All,
    #[default]
    Future,
    Specific {
        unix_timestamp: u64,
    }
}

impl Serialize for FilterSemester {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::All => serializer.serialize_str(""),
            Self::Future => serializer.serialize_str("future"),
            Self::Specific { unix_timestamp } => serializer.serialize_str(&unix_timestamp.to_string()),
        }
    }
}

/// Allows limiting the search to specific categories and sometimes items inside that category.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchFilter {
    All {
        semester: FilterSemester,
    },
    Courses {
        semester: FilterSemester,
        seminar_type_id: Option<String>,
        institute_id: Option<String>,
    },
    Users,
    Institutions,
    Messages
}

impl Default for SearchFilter {
    fn default() -> Self {
        SearchFilter::All {
            semester: Default::default(),
        }
    }
}

impl Serialize for SearchFilter {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        match self {
            SearchFilter::All { semester } => {
                map.serialize_entry("category", "show_all_categories")?;
                map.serialize_entry("semester", semester)?;
            }
            SearchFilter::Courses {
                semester,
                seminar_type_id,
                institute_id,
            } => {
                map.serialize_entry("category", "GlobalSearchCourses")?;
                map.serialize_entry("semester", semester)?;
                if let Some(seminar_type) = seminar_type_id {
                    map.serialize_entry("seminar_type", seminar_type)?;
                }
                if let Some(institute) = institute_id {
                    map.serialize_entry("institute", institute)?;
                }
            }
            SearchFilter::Users => {
                map.serialize_entry("category", "GlobalSearchUsers")?;
            }
            SearchFilter::Institutions => {
                map.serialize_entry("category", "GlobalSearchInstitutes")?;
            }
            SearchFilter::Messages => {
                map.serialize_entry("category", "GlobalSearchMessages")?;
            }
        }
        map.end()
    }
}


const GLOBAL_SEARCH_URL: &'static str = "https://studip.example.com/dispatch.php/globalsearch/find";

/// Represents the categorized results found by [`global_search()`]
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    #[serde(rename = "GlobalSearchCourses")]
    pub courses: Option<SearchResultCategory<SearchEntryCourse>>,
    #[serde(rename = "GlobalSearchUsers")]
    pub users: Option<SearchResultCategory<SearchEntryUser>>,
    #[serde(rename = "GlobalSearchInstitutes")]
    pub institutes: Option<SearchResultCategory<SearchEntryInstitute>>,
    #[serde(rename = "GlobalSearchMessages")]
    pub messages: Option<SearchResultCategory<SearchEntryMessage>>,
}

/// A generic search category. Contains the found entries in `content`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResultCategory<T> {
    /// The name of the category
    pub name: String,
    /// Url to the fullsearch
    pub fullsearch: String,
    /// The actual entries
    pub content: Vec<T>,
    /// If there is more content?
    pub more: bool,
    /// Honestly, idk.
    pub plus: bool,
}


/// A course entry returned by [`global_search()`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchEntryCourse {
    pub id: String,
    #[serde(rename = "number")]
    pub _number: String,
    pub name: String,
    pub url: String,
    pub date: String,
    pub dates: String,
    pub has_children: bool,
    pub children: Vec<Value>,
    pub additional: String,
    pub expand: String,
    pub admission_state: String,
    pub img: String,
}

/// A institute entry returned by [`global_search()`].
///
/// Can be converted to a normal [`Institute`] using [`From`]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchEntryInstitute {
    pub id: String,
    pub name: String,
    pub url: String,
    pub expand: String,
    pub img: String,
}

impl From<SearchEntryInstitute> for Institute {
    fn from(value: SearchEntryInstitute) -> Self {
        Institute {
            id: value.id,
            name: strip_markings(&value.name)
        }
    }
}

/// A user entry returned by [`global_search()`].
///
/// Can be converted to a normal [`User`] using [`From`]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchEntryUser {
    pub id: String,
    pub name: String,
    pub url: String,
    pub additional: String,
    pub expand: String,
    pub img: String,
}

impl From<SearchEntryUser> for User {
    fn from(value: SearchEntryUser) -> Self {
        User {
            display_name: strip_markings(&value.name),
            username: get_username_from_url(value.url).expect("Invalid User URL"),
            avatar_src: Some(value.img),
            source: ReferenceSource::Unspecified,
        }
    }
}

/// A message entry returned by [`global_search()`].
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchEntryMessage {
    pub name: String,
    pub url: String,
    pub img: String,
    pub date: String,
    pub description: String,
    pub additional: String,
    pub expand: String,
    #[serde(rename = "user")]
    pub user_name: String,
}

/// Does a global search for the given `text`, providing at most `max_results` results per category using the given [`SearchFilter`].
pub fn global_search(client: &StudIpClient, text: &str, max_results: usize, filter: &SearchFilter) -> anyhow::Result<SearchResult> {
    let filter_string = serde_json::to_string(filter).context("Cannot convert filter to json")?;

    let response = client.get(format!("{}/{}", GLOBAL_SEARCH_URL, max_results))
        .query(&[
            ("search", text),
            ("filter", filter_string.as_str()),
        ])
        .send()?;

    if !response.status().is_success() {
        bail!("Could not search. Status Code: {}", response.status());
    }
    // Check headers for json
    let content_type = response.headers().get(CONTENT_TYPE)
        .ok_or(anyhow!("Expected content-type"))?
        .to_str().context("Cannot convert content type to sting")?;
    if !content_type.starts_with("application/json") {
        bail!("Expected JSON. Got ContentType: {:?}", content_type);
    }
    // Check if the response is `[]` (WHY? WHY WOULD YOU RESPOND WITH THIS!?? You don't even return an array when you found something)
    let text = response.text()?;
    if text.trim() == "[]" {
        return Ok(Default::default());
    }
    Ok(serde_json::from_str(&text).context("Could not parse search response json")?)
}

/// Strips the html <mark> tag from the given string.
///
/// This exists, because [`SearchResult`] might contain this tag around text that matched the search text. \
/// Note: This does **NOT** handle nested <mark> tags.
pub fn strip_markings(str: &str) -> String {
    let regex = regex::Regex::new(r"(.*?)<mark>(.*?)</mark>(.*?)").unwrap();
    regex.replace_all(str, "$1$2$3").to_string()
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_strip_markings() {
        assert_eq!(strip_markings("Mark<mark> Ole</mark> Peter"), "Mark Ole Peter");
        assert_eq!(strip_markings("<mark>Mark</mark> Ole <mark>Peter</mark>"), "Mark Ole Peter");
        assert_eq!(strip_markings("Max Counterman"), "Max Counterman");
        assert_eq!(strip_markings("John <mark><mark>Connman</mark></mark>"), "John <mark>Connman</mark>");
    }

    #[test]
    fn test_courses_serialization() {
        let filter = SearchFilter::Courses {
            semester: FilterSemester::All,
            seminar_type_id: Some("1".to_string()),
            institute_id: Some("2123".to_string()),
        };
        let serialized = serde_json::to_string(&filter).unwrap();
        let expected = r#"{"category":"GlobalSearchCourses","semester":"","seminar_type":"1","institute":"2123"}"#;
        assert_eq!(serialized, expected);
    }
}