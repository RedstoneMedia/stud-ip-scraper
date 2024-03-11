use anyhow::Context;
use chrono::NaiveDate;
use itertools::Itertools;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use crate::ref_source::ReferenceSource;
use crate::StudIpClient;
use crate::user::{get_username_from_link_element, parse_simple_user, User};

const QUESTIONER_RESULTS_URL: &str = "https://studip.example.com/dispatch.php/questionnaire/evaluate";

/// A single votable option in a questionnaire
/// Vote results have to be queried separately with [`Questionnaire::query_results`]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionnaireOption {
    pub text: String,
    pub value: usize,
    pub n_voters: usize,
    pub voters: Option<Vec<User>>
}

/// The kind of questionnaire
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuestionnaireKind {
    SingleChoice,
    MultipleChoice
}

/// A single questionnaire
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Questionnaire {
    /// The unique id of the questionnaire
    pub id: String,
    /// Extra information about the source of the questionnaire
    pub reference_source: ReferenceSource,
    /// The title of the questionnaire
    pub title: String,
    /// The description of the questionnaire
    pub description: String,
    /// The author of the questionnaire
    pub author: User,
    /// The type of the questionnaire
    pub kind: QuestionnaireKind,
    /// The localized terms of the questionnaire
    pub terms: String,
    /// The number of answers
    pub total_voters: usize,
    /// When the questionnaire was created
    pub creation_date: NaiveDate,
    /// The voting options
    pub options: Vec<QuestionnaireOption>,
}

impl Questionnaire {

    /// Query the results of the [`Questionnaire`]'s evaluation, so the votes for each option \
    /// *Note: This is not done automatically*
    pub fn query_results(&mut self, client: &StudIpClient) -> anyhow::Result<()> {
        // Send evaluation request with correct source
        let query_params = self.reference_source.get_additional_query_params()
            .into_iter()
            .collect_vec();
        let url = format!("{}/{}", QUESTIONER_RESULTS_URL, self.id);
        let response = client.get(url)
            .query(&query_params)
            .header("X-Requested-With", "XMLHttpRequest")
            .send()?;
        // Parse questionnaire results
        let text = response.text()?;
        let html = Html::parse_document(&text);
        // Parse the options, including the number of voters for each and if not anonymous the actual voters
        let options_counts_selector = Selector::parse("td:not([width])").unwrap();
        let options_text_selector = Selector::parse("td[width] > strong").unwrap();
        let result_options_selector = Selector::parse("table.default tr").unwrap();
        let voters_selector = Selector::parse("td[width] > a").unwrap();
        let avatar_selector = Selector::parse("img.avatar-small").unwrap();
        let n_voters_regex = Regex::new(r"\(\d+% \| (?P<voters>\d+)/(?P<total_voters>\d+)\)").unwrap();
        for (i, result_option_elem) in html.select(&result_options_selector).enumerate() {
            // Insert new options
            if i >= self.options.len() {
                self.options.push(QuestionnaireOption {
                    text: "".to_string(),
                    value: i,
                    n_voters: 0,
                    voters: None,
                });
            }
            // Parse option data
            let option = &mut self.options[i];
            let options_text = result_option_elem.select(&options_text_selector)
                .next()
                .context("Expected option text")?
                .text()
                .collect::<String>()
                .trim()
                .to_string();
            let option_counts_string = result_option_elem.select(&options_counts_selector)
                .next()
                .context("Expected option text")?
                .text()
                .collect::<String>()
                .trim()
                .to_string();
            let n_voters = n_voters_regex
                .captures(&option_counts_string)
                .context("Expected voters")?
                .name("voters")
                .unwrap()
                .as_str()
                .parse()?;
            let n_total_voters = n_voters_regex
                .captures(&option_counts_string)
                .context("Expected total voters")?
                .name("total_voters")
                .unwrap()
                .as_str()
                .parse()?;
            option.text = options_text;
            option.n_voters = n_voters;
            self.total_voters = n_total_voters;
            // If not anonymous, parse the voters too
            if n_voters > 0 {
                let mut found_voters = vec![];
                for voter_elem in result_option_elem.select(&voters_selector) {
                    let username = get_username_from_link_element(voter_elem)?;
                    let avatar_elem = voter_elem.select(&avatar_selector)
                        .next()
                        .context("Expected avatar")?;
                    let display_name = avatar_elem.attr("title")
                        .context("Expected avatar display name")?
                        .to_string();
                    let avatar_src = avatar_elem
                        .attr("src")
                        .context("Expected avatar src")?
                        .to_string();

                    found_voters.push(User {
                        username,
                        display_name,
                        avatar_src: Some(avatar_src),
                        source: ReferenceSource::Unspecified,
                    });
                }
                if !found_voters.is_empty() {
                    option.voters = Some(found_voters);
                }
            }
        }
        Ok(())
    }

}

/// Parses a single [`Questionnaire`] from html, using the given [`ReferenceSource`]
pub fn parse_questionnaire(element: ElementRef, reference_source: ReferenceSource) -> anyhow::Result<Questionnaire> {
    // Parse header
    let title_selector = Selector::parse("header > h1 > a").unwrap();
    let author_selector = Selector::parse("header > nav > a").unwrap();
    let creation_date_selector = Selector::parse("header > nav > span:not([title])").unwrap();
    let number_of_answers_selector = Selector::parse("header > nav > span[title*=\"antworten\" i], header > nav span[title*=\"answers\" i]").unwrap();
    let id = element
        .value()
        .attr("data-questionnaire_id")
        .context("Expected questionnaire id")?
        .to_string();
    let title = element
        .select(&title_selector)
        .next()
        .context("Expected title")?
        .text()
        .collect::<String>()
        .trim()
        .to_string();
    let author = parse_simple_user(
        element.select(&author_selector)
            .next()
            .context("Expected author")?
    )?;
    let creation_date_string = element.select(&creation_date_selector)
        .next()
        .context("Expected creation date")?
        .text()
        .collect::<String>()
        .trim()
        .to_string();
    let creation_date = NaiveDate::parse_from_str(&creation_date_string, "%d.%m.%Y")?;
    let number_of_answers = element
        .select(&number_of_answers_selector)
        .next()
        .context("Expected number of answers")?
        .text()
        .collect::<String>()
        .trim()
        .replace('.', "")
        .parse::<usize>()?;
    // Parse content (description, and options, also the questionnaire kind)
    let description_selector = Selector::parse("article .description").unwrap();
    let description = element
        .select(&description_selector)
        .next()
        .context("Expected description")?
        .text()
        .collect::<String>()
        .trim()
        .to_string();
    let options_selector = Selector::parse("article .questionnaire_answer ul.clean label").unwrap();
    let options_value_selector = Selector::parse("input[value]").unwrap();
    let mut options = vec![];
    let mut kind = QuestionnaireKind::SingleChoice;
    for option_elem in element.select(&options_selector) {
        let text = option_elem.text()
            .collect::<String>()
            .trim()
            .to_string();
        let input_elem = option_elem.select(&options_value_selector)
            .next()
            .context("Expected option input")?;
        let value : usize = input_elem.attr("value")
            .unwrap() // We can do this because the selector only selects elements with a value
            .to_string()
            .parse()?;
        // Figure out kind, based on input type
        if input_elem.attr("type") == Some("checkbox") {
            kind = QuestionnaireKind::MultipleChoice;
        }
        options.push(QuestionnaireOption {
            text,
            value,
            n_voters: 0,
            voters: None,
        });
    }
    // Sort options, because the order in the html is not guaranteed
    options.sort_by_key(|option| option.value);
    // Parse terms
    let terms_selector = Selector::parse("section .terms").unwrap();
    let terms = element
        .select(&terms_selector)
        .next()
        .context("Expected terms")?
        .text()
        .collect::<String>()
        .trim()
        .to_string();

    Ok(Questionnaire {
        id,
        reference_source,
        title,
        description,
        author,
        kind,
        terms,
        total_voters: number_of_answers,
        creation_date,
        options,
    })
}