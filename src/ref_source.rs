use anyhow::Context;
use serde::{Deserialize, Serialize};
use url::Url;
use crate::course::COURSE_URL;
use crate::user::PROFILE_URL;

pub(crate) const START_URL: &str = "https://studip.example.com/dispatch.php/start";

/// Stores source extra information for a piece of information \
/// Sometimes necessary to make correct queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReferenceSource {
    Unspecified,
    StartPage,
    Course(String),
    Profile(String),
}

impl ReferenceSource {

    /// Gets additional query parameters for the reference source
    pub fn get_additional_query_params(&self) -> Option<(&'static str, &str)> {
        match self {
            ReferenceSource::Unspecified | ReferenceSource::StartPage => None,
            ReferenceSource::Course(id) => Some(("cid", id)),
            ReferenceSource::Profile(id) => Some(("username", id))
        }
    }

    /// Constructs an url from the reference source
    pub fn try_get_url(&self) -> Option<Url> {
        match self {
            ReferenceSource::Unspecified => None,
            ReferenceSource::StartPage => Some(Url::parse(START_URL).unwrap()),
            ReferenceSource::Course(id) => {
                let mut url = Url::parse(COURSE_URL).unwrap();
                url.query_pairs_mut().append_pair("cid", id);
                Some(url)
            }
            ReferenceSource::Profile(username) => {
                let mut url = Url::parse(PROFILE_URL).unwrap();
                url.query_pairs_mut().append_pair("username", username);
                Some(url)
            }
        }
    }

}

impl TryFrom<&ReferenceSource> for Url {

    type Error = anyhow::Error;

    fn try_from(value: &ReferenceSource) -> Result<Self, Self::Error> {
        value.try_get_url()
            .context("Could not construct url from reference source")
    }
}