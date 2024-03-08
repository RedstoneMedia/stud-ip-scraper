use std::any::Any;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use anyhow::Context;
use chrono::{DateTime, Utc};
use reqwest::Url;
use scraper::{Html, Selector};
use serde::{Deserialize, Deserializer, Serialize};
use chrono::serde::ts_seconds;
use crate::common_data::User;
use crate::course_modules::{CourseModule, CourseModuleData};

const FILE_MODULE_URL : &str = "https://studip.example.com/dispatch.php/course/files";
const DOWNLOAD_URL : &str = "https://studip.example.com/sendfile.php";

/// Module, that enables operating on the files and folders of a course
#[derive(Debug)]
pub struct FileModule {
    module_data: Arc<CourseModuleData>
}

impl CourseModule for FileModule {
    fn new(data: Arc<CourseModuleData>) -> Self {
        Self {
            module_data: data,
        }
    }

    fn name() -> &'static str {
        "files"
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl FileModule {

    fn parse_into_folder_contents(response_text: &str) -> anyhow::Result<FolderContents> {
        let html = Html::parse_document(response_text);
        let files_form = html.select(&Selector::parse("#files_table_form").unwrap())
            .next()
            .context("Could not find files table form")?;
        let file_form_element = files_form.value();
        let data_files = file_form_element.attr("data-files")
            .context("Could not get files")?;
        let data_folders = file_form_element.attr("data-folders")
            .context("Could not get folders")?;

        let their_files: Vec<TheirFile> = serde_json::from_str(data_files)?;
        let their_folders: Vec<TheirFolder> = serde_json::from_str(data_folders)?;
        Ok(FolderContents {
            folders: their_folders.into_iter().map(|f| f.into()).collect(),
            files: their_files.into_iter().map(|f| f.into()).collect(),
        })
    }

    /// Returns the courses root [`FolderContents`].
    pub fn get_root(&self) -> anyhow::Result<FolderContents> {
        let response = self.module_data.client.get(FILE_MODULE_URL)
            .query(&[("cid", &self.module_data.course_id)])
            .send()?;
        Self::parse_into_folder_contents(&response.text()?)
    }

    /// Returns the [`FolderContents`] of a specific folder. \
    /// The `folder_id` parameter specifies the ID of the folder.
    pub fn get_folder(&self, folder_id: &str) -> anyhow::Result<FolderContents> {
        let response = self.module_data.client.get(format!("{}/index/{}", FILE_MODULE_URL, folder_id))
            .query(&[("cid", &self.module_data.course_id)])
            .send()?;
        Self::parse_into_folder_contents(&response.text()?)
    }

    /// Downloads a [`File`] and returns its bytes
    pub fn download_file(&self, file: &File) -> anyhow::Result<Vec<u8>> {
        let response = self.module_data.client.get(DOWNLOAD_URL)
            .query(&[("type", "0")])
            .query(&[("file_id", &file.object.id)])
            .query(&[("file_name", &file.object.name)])
            .send()?;
        Ok(response.bytes()?.to_vec())
    }

    /// Saves a [`File`] to a specified location. \
    /// The `file` parameter specifies the file to be saved. \
    /// The `to` parameter specifies the location where the file will be saved. \
    /// Note: The file is not streamed to disk, which could lead to memory issues for very large files.
    pub fn save_file_to(&self, file: &File, to: impl AsRef<Path>) -> anyhow::Result<()> {
        let bytes = self.download_file(file)?;
        let path = to.as_ref().join(&file.object.name);
        std::fs::write(path, bytes)?;
        Ok(())
    }

    // TODO: Add file upload option

}

/// Contains common data for [`Folder`]s and [`File`]s
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesObject {
    pub id: String,
    pub name: String,
    pub change_date: DateTime<Utc>,
    pub author: User,
    pub icon: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct File {
    pub object: FilesObject,
    pub size: usize,
    pub downloads: usize,
    pub restricted_terms_of_use: bool,
    pub new: bool,
    pub is_editable: bool,
    pub is_accessible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub object: FilesObject,
    pub object_count: usize,
    pub permissions: String
}

/// Combines the [`File`]s and [`Folder`]s inside a Folder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderContents {
    pub folders: Vec<Folder>,
    pub files: Vec<File>
}

fn from_str<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: FromStr,
        T::Err: std::fmt::Display,
{
    let s = String::deserialize(deserializer)?;
    T::from_str(&s).map_err(serde::de::Error::custom)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TheirFile {
    pub id: String,
    pub name: String,
    #[serde(rename = "download_url")]
    pub download_url: Option<String>,
    #[serde(deserialize_with = "from_str")]
    pub downloads: usize,
    #[serde(rename = "mime_type")]
    pub mime_type: String,
    pub icon: String,
    #[serde(deserialize_with = "from_str")]
    pub size: usize,
    #[serde(rename = "author_url")]
    pub author_url: String,
    #[serde(rename = "author_name")]
    pub author_name: String,
    #[serde(rename = "author_id")]
    pub author_id: String,
    #[serde(with = "ts_seconds")]
    pub chdate: DateTime<Utc>,
    pub additional_columns: Vec<serde_json::Value>,
    #[serde(rename = "details_url")]
    pub details_url: String,
    pub restricted_terms_of_use: bool,
    pub actions: String,
    pub new: bool,
    pub is_editable: bool,
    pub is_accessible: bool,
}

impl From<TheirFile> for File {
    fn from(value: TheirFile) -> Self {
        Self {
            object: FilesObject {
                id: value.id,
                name: value.name,
                change_date: value.chdate,
                author: User {
                    display_name: value.author_name,
                    username: username_from_author_url(&value.author_url),
                    avatar_src: None,
                },
                icon: value.icon,
                mime_type: value.mime_type,
            },
            size: value.size,
            downloads: value.downloads,
            restricted_terms_of_use: value.restricted_terms_of_use,
            new: value.new,
            is_editable: value.is_editable,
            is_accessible: value.is_accessible,
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TheirFolder {
    pub id: String,
    pub icon: String,
    pub name: String,
    pub url: String,
    #[serde(rename = "user_id")]
    pub user_id: String,
    #[serde(rename = "object_count")]
    pub object_count: usize,
    #[serde(rename = "author_name")]
    pub author_name: String,
    #[serde(rename = "author_url")]
    pub author_url: String,
    #[serde(with = "ts_seconds")]
    pub chdate: DateTime<Utc>,
    pub actions: String,
    #[serde(rename = "mime_type")]
    pub mime_type: String,
    pub permissions: String,
    pub additional_columns: Vec<serde_json::Value>,
}

impl From<TheirFolder> for Folder {
    fn from(value: TheirFolder) -> Self {
        Self {
            object: FilesObject {
                id: value.id,
                name: value.name,
                change_date: value.chdate,
                author: User {
                    display_name: value.author_name,
                    username: username_from_author_url(&value.author_url),
                    avatar_src: None,
                },
                icon: value.icon,
                mime_type: value.mime_type,
            },
            object_count: value.object_count,
            permissions: value.permissions,
        }
    }
}

fn username_from_author_url(author_url: &str) -> String {
    let author_url = Url::parse(author_url).unwrap();
    author_url.query_pairs()
        .find_map(|(key, value)| (key == "username").then(|| value.to_string()))
        .unwrap()
}