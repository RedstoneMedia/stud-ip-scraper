use anyhow::Context;
use scraper::ElementRef;
use serde::{Deserialize, Serialize};
use url::Url;

/// Represents basic information about an institute
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Institute {
    pub id: String,
    pub name: String,
}

impl PartialEq for Institute {

    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }

}

/// Tries to parse a link element (<a>) to a [`Institute`]
pub fn parse_institute_link(element: ElementRef) -> anyhow::Result<Institute> {
    let name = element.text()
        .collect::<String>()
        .trim()
        .to_string();
    let institute_url = Url::parse(element.attr("href")
        .context("Expected institute href")?
    )?;
    let id = institute_url.query_pairs()
        .find_map(|(key, value)| (key == "auswahl" || key == "selection")
            .then(|| value.to_string()))
        .context("Expected institute id")?;
    Ok(Institute {
        id,
        name
    })
}