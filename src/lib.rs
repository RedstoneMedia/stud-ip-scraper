pub mod course_modules;
pub mod common_data;
pub mod course;

use std::cell::RefCell;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use anyhow::{bail, Context};
use reqwest::blocking::{Client, ClientBuilder};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use url::Url;
use crate::course::MyCourses;

const LOGIN_URL : &str = "https://studip.example.com/Shibboleth.sso/Login";
const SAML_RESPONSE_URL: &str = "https://studip.example.com/Shibboleth.sso/SAML2/POST";

const REQUEST_MAX_SPEED: Duration = Duration::from_millis(150);

/// The entry point into interacting with StudIp
pub struct StudIp {
    pub client: Arc<StudIpClient>,
    pub my_courses: MyCourses
}

impl StudIp {

    fn login_client<IdP: IdentityProvider>(&self, creds_path: &str) -> anyhow::Result<()> {
        // Sets some cookies
        let _ = self.client.get("https://studip.example.com/index.php?logout=true&set_language=de_DE&set_contrast=").send();
        // Read and parse credentials
        let creds = std::fs::read_to_string(creds_path)
            .context("Could not read from creds.txt")?;
        let (username, password) = creds.split_once('\n')
            .context("creds.txt did not have newline seperated username and password")?;

        let mut target_url = Url::parse(&format!("https://{}", self.client.host))?;
        target_url.query_pairs_mut()
            .append_pair("sso", "shib")
            .append_pair("again", "yes")
            .append_pair("cancel_login", "1");
        // Get LOGIN_URL to obtain redirected url (The url to the IdP)
        let redirected_url = self.client.get(LOGIN_URL)
            .query(&[
                ("target", target_url.as_str()),
                ("entityID", IdP::entity_url())
            ])
            .send()?
            .url()
            .clone();
        // Login with Identity Provider
        let saml_assertion = IdP::login(&self.client.client, redirected_url, username, password)?;
        // Send IdP's SAML response back to service provider (Stud Ip)
        let response = self.client.post(SAML_RESPONSE_URL)
            .form(&[("RelayState", saml_assertion.relay_state), ("SAMLResponse", saml_assertion.saml_response)])
            .send()
            .context("Could not send second login request. Are the credentials incorrect?")?;
        if !response.status().is_success() {
            bail!("Second login request had status code: {}", response.status());
        }
        Ok(())
    }

    fn make_client() -> anyhow::Result<Client> {
        // Setup client with headers
        let mut default_headers = HeaderMap::new();
        default_headers.insert("User-Agent", HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/118.0"));
        default_headers.insert("Accept", HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"));
        default_headers.insert("Accept-Language", HeaderValue::from_static("en-US,en;q=0.5"));
        default_headers.insert("Upgrade-Insecure-Requests", HeaderValue::from_static("1"));
        default_headers.insert("DNT", HeaderValue::from_static("1"));
        default_headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("document"));
        default_headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("navigate"));
        default_headers.insert("Sec-Fetch-Site", HeaderValue::from_static("cross-site"));
        ClientBuilder::new()
            .https_only(true)
            .cookie_store(true)
            .timeout(Duration::from_secs(8))
            .default_headers(default_headers)
            .gzip(true)
            .build()
            .context("Could not build reqwest client")
    }

    /// Attempts to log in into a  `[StudIp]` instance, specified by `host` (e.g. studip.example.com) \
    /// Uses the provided credentials and an [`IdentityProvider`], through which the user is authorized.
    pub fn login<IdP: IdentityProvider>(creds_path: &str, host: &'static str) -> anyhow::Result<Self> {
        let client = Arc::new(
            StudIpClient {
                client: Self::make_client()?,
                host,
                #[cfg(feature = "rate_limiting")]
                last_request_time: RefCell::new(SystemTime::UNIX_EPOCH),
            }
        );
        let stud_ip = Self {
            client: client.clone(),
            my_courses: MyCourses::from_client(client),
        };
        stud_ip.login_client::<IdP>(creds_path)?;
        Ok(stud_ip)
    }

}

/// The necessary data, that is sent back from the [`IdentityProvider`] to the Service Provider, to complete the authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SAMLAssertionData {
    pub relay_state: String,
    pub saml_response: String,
}

/// An Identity Provider is required to log in via SSO \
///
/// This is required as the login specifics might be drastically different for every institution.
/// Currently, this crate does not provide a specific Identity provider, meaning you will have to implement one yourself for your specific Educational institutions. \
///
/// Here is how an example provider could be defined:
/// ```
/// use stud_ip_scraper::{IdentityProvider, SAMLAssertionData};
/// use reqwest::blocking::Client;
/// use anyhow::{bail, Context};
///
/// struct ExampleIdP;
///
/// impl IdentityProvider for ExampleIdP {
///
///         fn login(client: &Client, redirect_url: impl reqwest::IntoUrl, username: &str, password: &str) -> anyhow::Result<SAMLAssertionData> {
///             // Send credentials
///             let response = client.post(redirect_url)
///                 .form(&[("username", username), ("password", password)])
///                 .send()?;
///             if response.status() != 200 {
///                 bail!("Could not login. Are the credentials incorrect?");
///             }
///             // Parse out Assertion data from response
///             // NOTE: This will probably be more involved for an actual IdP
///             let text =  response.text()?;
///             let (relay_state, saml_response) = text
///                 .split_once("\n")
///                 .context("Could not parse SAML assertion data")?;
///
///             Ok(SAMLAssertionData {
///                 relay_state: relay_state.to_string(),
///                 saml_response: saml_response.to_string(),
///             })
///         }
///
///         fn entity_url() -> &'static str {
///             "https://sso.example.com/idp/shibboleth"
///         }
///     }
/// ```
pub trait IdentityProvider {

    /// Attempts to Log in the client with a username and password. \
    /// Also accepts a `url`, that is derived from the [`IdentityProvider::entity_url()`], but with potentially more data, from the Service Provider \
    /// Returns the [`SAMLAssertionData`], if successful.
    fn login(client: &Client, url: impl reqwest::IntoUrl, username: &str, password: &str) -> anyhow::Result<SAMLAssertionData>;

    /// The entity url of the Identify Provider, also sometimes called `entityID`
    fn entity_url() -> &'static str;

}

/// A wrapped reqwest [`Client`], that automatically replaces the host of every request
#[derive(Debug)]
pub struct StudIpClient {
    pub client: Client,
    pub host: &'static str,
    #[cfg(feature = "rate_limiting")]
    last_request_time: RefCell<SystemTime>,
}

impl Default for StudIpClient {
    fn default() -> Self {
        Self {
            client: Default::default(),
            host: "",
            #[cfg(feature = "rate_limiting")]
            last_request_time: RefCell::new(SystemTime::UNIX_EPOCH),
        }
    }
}

impl StudIpClient {

    #[cfg(feature = "rate_limiting")]
    fn before_request(&self) {
        // Rate limits on request creation
        // Any requests that are created, but not sent, will still be rate limited
        let mut last_request_time = self.last_request_time.borrow_mut();
        let elapsed = last_request_time.elapsed().unwrap_or(Duration::from_secs(0));
        if elapsed > REQUEST_MAX_SPEED {
            *last_request_time = SystemTime::now();
            return;
        }
        let wait_time = REQUEST_MAX_SPEED - elapsed;
        std::thread::sleep(wait_time);
        *last_request_time = SystemTime::now();
    }

    #[cfg(not(feature = "rate_limiting"))]
    fn before_request(&self) {}
}

macro_rules! impl_client_wrap {
    ($($method:ident),+) => {
        impl StudIpClient {
            $(
                pub fn $method(&self, url: impl reqwest::IntoUrl) -> reqwest::blocking::RequestBuilder {
                    self.before_request();
                    let mut url : Url = url.into_url().unwrap();
                    url.set_host(Some(self.host)).unwrap();
                    #[cfg(feature = "verbose")]
                    {
                        println!("{}: {}", stringify!($method), url.as_str());
                    }
                    self.client.$method(url)
                }
            )+
        }
    };
}

impl_client_wrap!(get, post, put, patch, delete, head);