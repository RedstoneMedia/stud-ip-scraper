use std::any::Any;
use std::path::Path;
use std::rc::Rc;
use std::str::FromStr;
use anyhow::Context;
use chrono::{DateTime, Utc};
use scraper::{Html, Selector};
use serde::{Deserialize, Deserializer, Serialize};
use chrono::serde::ts_seconds;
use crate::user::{get_username_from_url, User};
use crate::ref_source::ReferenceSource;
use crate::course_modules::{CourseModule, CourseModuleData};

const FILE_MODULE_URL : &str = "https://studip.example.com/dispatch.php/course/files";
const DOWNLOAD_URL : &str = "https://studip.example.com/sendfile.php";

/// Module, that enables operating on the files and folders of a course
#[derive(Debug)]
pub struct FileModule {
    module_data: Rc<CourseModuleData>
}

impl CourseModule for FileModule {
    fn new(data: Rc<CourseModuleData>) -> Self {
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

    fn parse_into_folder_contents(&self, response_text: &str) -> anyhow::Result<FolderContents> {
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
            folders: their_folders.into_iter()
                .map(|f| try_folder_from_their(f, &self.module_data.course_id))
                .collect::<Result<_, _>>()?,
            files: their_files.into_iter()
                .map(|f| try_file_from_their(f, &self.module_data.course_id))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Returns the courses root [`FolderContents`].
    pub fn get_root(&self) -> anyhow::Result<FolderContents> {
        let response = self.module_data.client.get(FILE_MODULE_URL)
            .query(&[("cid", &self.module_data.course_id)])
            .send()?;
        self.parse_into_folder_contents(&response.text()?)
    }

    /// Returns the [`FolderContents`] of a specific folder. \
    /// The `folder_id` parameter specifies the ID of the folder.
    pub fn get_folder(&self, folder_id: &str) -> anyhow::Result<FolderContents> {
        let response = self.module_data.client.get(format!("{}/index/{}", FILE_MODULE_URL, folder_id))
            .query(&[("cid", &self.module_data.course_id)])
            .send()?;
        self.parse_into_folder_contents(&response.text()?)
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

impl PartialEq for FilesObject {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
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

impl PartialEq for File {
    fn eq(&self, other: &Self) -> bool {
        self.object == other.object
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub object: FilesObject,
    pub object_count: usize,
    pub permissions: String
}

impl PartialEq for Folder {
    fn eq(&self, other: &Self) -> bool {
        self.object == other.object
    }
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

fn try_file_from_their(their: TheirFile, course_id: &str) -> anyhow::Result<File> {
    Ok(File {
        object: FilesObject {
            id: their.id,
            name: their.name,
            change_date: their.chdate,
            author: User {
                display_name: their.author_name,
                username: get_username_from_url(&their.author_url)?,
                avatar_src: None,
                source: ReferenceSource::Course(course_id.to_string()),
            },
            icon: their.icon,
            mime_type: their.mime_type,
        },
        size: their.size,
        downloads: their.downloads,
        restricted_terms_of_use: their.restricted_terms_of_use,
        new: their.new,
        is_editable: their.is_editable,
        is_accessible: their.is_accessible,
    })
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

fn try_folder_from_their(their: TheirFolder, course_id: &str) -> anyhow::Result<Folder> {
    Ok(Folder {
        object: FilesObject {
            id: their.id,
            name: their.name,
            change_date: their.chdate,
            author: User {
                display_name: their.author_name,
                username: get_username_from_url(&their.author_url)?,
                avatar_src: None,
                source: ReferenceSource::Course(course_id.to_string()),
            },
            icon: their.icon,
            mime_type: their.mime_type,
        },
        object_count: their.object_count,
        permissions: their.permissions,
    })
}
