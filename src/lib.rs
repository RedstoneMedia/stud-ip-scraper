pub mod course_modules;
pub mod user;
pub mod course;
pub mod news;
pub mod questionnaire;
pub mod ref_source;
pub mod institute;
pub mod search;

use std::cell::RefCell;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use anyhow::{bail, Context};
use reqwest::blocking::{Client, ClientBuilder};
use reqwest::cookie::Jar;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use url::Url;
use crate::course::MyCourses;
use crate::search::{SearchFilter, SearchResult};

#[cfg(feature = "verbose")]
use log::{trace, debug, info};

const LOGIN_URL : &str = "https://studip.example.com/Shibboleth.sso/Login";
const SAML_RESPONSE_URL: &str = "https://studip.example.com/Shibboleth.sso/SAML2/POST";
const START_URL: &str = "https://studip.example.com/dispatch.php/start";

/// The entry point into interacting with StudIp
pub struct StudIp {
    pub client: Arc<StudIpClient>,
    pub my_courses: MyCourses
}

impl StudIp {

    fn login_client<IdP: IdentityProvider>(&self, creds_path: &str) -> anyhow::Result<()> {
        #[cfg(feature = "verbose")]
        info!("Starting Stud.IP login process");

        // Sets some cookies
        let _ = self.client.get("https://studip.example.com").send();
        // Read and parse credentials
        let creds = std::fs::read_to_string(creds_path)
            .context("Could not read from creds.txt")?;
        let (username, password) = creds.split_once('\n')
            .context("creds.txt did not have newline seperated username and password")?;
        let username = username.trim();
        let password = password.trim();

        let mut target_url = Url::parse(&format!("https://{}/index.php", self.client.host))?;
        target_url.query_pairs_mut()
            .append_pair("sso", "shib")
            .append_pair("again", "yes")
            .append_pair("cancel_login", "1"); // I have no idea why this exists
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
        // Send request to start page, to check if we are really logged in (because we get a 200 even if it fails)
        let response = self.client.get(START_URL).send()?;
        if !response.status().is_success() {
            bail!("Could not access start page after login: {}", response.status());
        }
        let html = scraper::Html::parse_document(&response.text().context("Could not get start page text")?);
        let login_selector = scraper::Selector::parse("#login").unwrap();
        if html.select(&login_selector).next().is_some() {
            bail!("Failed to login, after sending SAML response to StudIP: Still on login page");
        }

        #[cfg(feature = "verbose")]
        info!("Successfully logged into Stud.IP");
        Ok(())
    }

    /// Attempts to log in into a [`StudIp`] instance, with the `client` specified by the [`StudIpClient`] \
    /// Uses the provided credentials and an [`IdentityProvider`], through which the user is authorized.
    pub fn login<IdP: IdentityProvider>(creds_path: &str, client: StudIpClient) -> anyhow::Result<Self> {
        let client = Arc::new(client);
        let stud_ip = Self {
            client: client.clone(),
            my_courses: MyCourses::from_client(client),
        };
        stud_ip.login_client::<IdP>(creds_path)?;
        Ok(stud_ip)
    }

    /// Does a global search for the given `text`, providing at most `max_results` results per category using the given [`SearchFilter`].
    pub fn global_search(&self, text: &str, max_results: usize, filter: &SearchFilter) -> anyhow::Result<SearchResult> {
        search::global_search(&self.client, text, max_results, filter)
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
    fn login(client: &Client, url: impl reqwest::IntoUrl + Clone, username: &str, password: &str) -> anyhow::Result<SAMLAssertionData>;

    /// The entity url of the Identify Provider, also sometimes called `entityID`
    fn entity_url() -> &'static str;

}

/// A builder to configure a [`StudIpClient`] used for operating within [`StudIP`]
///
/// This is a restricted wrapper of the [`reqwest::blocking::RequestBuilder`]
pub struct StudIpClientBuilder {
    host: &'static str,
    proxy: Option<reqwest::Proxy>,
    danger_accept_invalid_certs: bool,
    timeout: Duration,
    user_agent: &'static str,
    #[cfg(feature = "rate_limiting")]
    request_max_speed: Duration,
}

impl StudIpClientBuilder {

    /// Creates a new builder with the specific Stud.IP host (e.g. studip.example.com)
    pub fn new(host: &'static str) -> Self {
        Self {
            host,
            proxy: None,
            danger_accept_invalid_certs: false,
            timeout: Duration::from_secs(8),
            user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:135.0) Gecko/20100101 Firefox/135.0",
            request_max_speed: Duration::from_millis(150)
        }
    }

    /// Add a Proxy to the list of proxies the Client will use
    pub fn proxy(mut self, proxy: reqwest::Proxy) -> Self {
        self.proxy = Some(proxy);
        self
    }

    /// Controls the use of certificate validation.
    ///
    /// This introduces significant vulnerabilities, and should only be used as a last resort. \
    /// One such case is debugging traffic using a [`proxy()`] to inspect encrypted traffic.
    pub fn danger_accept_invalid_certs(mut self, danger_accept_invalid_certs: bool) -> Self {
        self.danger_accept_invalid_certs = danger_accept_invalid_certs;
        self
    }

    /// Set a timeout for connect, read and write operations of a Client
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets the User-Agent string, that should be sent along as a head with every request.
    pub fn user_agent(mut self, user_agent: &'static str) -> Self {
        self.user_agent = user_agent;
        self
    }

    /// Controls how fast requests can be **constructed**, by specifying a maximum time between requests.
    ///
    /// NOTE: This does **NOT** currently specify how fast requests can *actually* be sent, although it strongly correlates with it.
    #[cfg(feature = "rate_limiting")]
    pub fn request_max_speed(mut self, max_time_between: Duration) -> Self {
        self.request_max_speed = max_time_between;
        self
    }

    /// Builds a [`StudIpClient`] using the current configuration
    pub fn build(self) -> anyhow::Result<StudIpClient> {
        // Setup client with basic headers
        let mut default_headers = HeaderMap::new();
        default_headers.insert("User-Agent", HeaderValue::from_static(self.user_agent));
        default_headers.insert("Accept", HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"));
        default_headers.insert("Accept-Language", HeaderValue::from_static("en-US,en;q=0.5"));
        default_headers.insert("Upgrade-Insecure-Requests", HeaderValue::from_static("1"));
        default_headers.insert("DNT", HeaderValue::from_static("1"));
        default_headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("document"));
        default_headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("navigate"));
        default_headers.insert("Sec-Fetch-Site", HeaderValue::from_static("same-origin"));
        default_headers.insert("Sec-Fetch-User", HeaderValue::from_static("?1"));
        default_headers.insert("TE", HeaderValue::from_static("trailers"));
        default_headers.insert("Priority", HeaderValue::from_static("u=0, i"));

        let cookie_jar = Arc::new(Jar::default());
        let mut client_builder = ClientBuilder::new()
            .https_only(true)
            .danger_accept_invalid_certs(self.danger_accept_invalid_certs)
            .cookie_provider(cookie_jar.clone())
            .timeout(self.timeout)
            .use_rustls_tls()
            .default_headers(default_headers)
            .gzip(true);

        if let Some(proxy) = self.proxy {
            client_builder = client_builder.proxy(proxy);
        }

        let client = client_builder.build().context("Could not build reqwest client")?;
        Ok(StudIpClient {
            client,
            cookie_jar,
            host: self.host,
            #[cfg(feature = "rate_limiting")]
            last_request_time: RefCell::new(SystemTime::UNIX_EPOCH),
            #[cfg(feature = "rate_limiting")]
            request_max_speed: self.request_max_speed,
        })
    }

    /// Attempts to log in into a [`StudIp`] instance, with a client that has the current configuration \
    /// Uses the provided credentials and an [`IdentityProvider`], through which the user is authorized.
    pub fn login<IdP: IdentityProvider>(self, creds_path: &str) -> anyhow::Result<StudIp> {
        let client = self.build()?;
        StudIp::login::<IdP>(creds_path, client)
    }
}

/// A wrapped reqwest [`Client`], that automatically replaces the host of every request
#[derive(Debug)]
pub struct StudIpClient {
    pub client: Client,
    pub host: &'static str,
    pub cookie_jar: Arc<Jar>,
    #[cfg(feature = "rate_limiting")]
    last_request_time: RefCell<SystemTime>,
    #[cfg(feature = "rate_limiting")]
    request_max_speed: Duration
}

impl Default for StudIpClient {
    fn default() -> Self {
        Self {
            client: Default::default(),
            host: "studip.example.com",
            cookie_jar: Arc::new(Default::default()),
            #[cfg(feature = "rate_limiting")]
            last_request_time: RefCell::new(SystemTime::UNIX_EPOCH),
            #[cfg(feature = "rate_limiting")]
            request_max_speed: Duration::from_millis(150),
        }
    }
}

impl StudIpClient {

    #[cfg(feature = "rate_limiting")]
    fn before_request(&self) {
        // Rate limits on request creation
        // Any requests that are created, but not sent, will still be rate limited.
        let mut last_request_time = self.last_request_time.borrow_mut();
        let elapsed = last_request_time.elapsed().unwrap_or(Duration::from_secs(0));
        if elapsed > self.request_max_speed {
            *last_request_time = SystemTime::now();
            return; // Send the request immediately
        }
        // Wait remaining time
        let wait_time = self.request_max_speed - elapsed;
        #[cfg(feature = "verbose")]
        trace!("Waiting {}ms, before constructing next request", wait_time.as_millis());
        std::thread::sleep(wait_time);
        *last_request_time = SystemTime::now();
    }

    pub fn execute(&self, request: reqwest::blocking::Request) -> reqwest::Result<reqwest::blocking::Response> {
        self.client.execute(request)
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
                        debug!("Sending request: {}: {}", stringify!($method), url.as_str());
                    }
                    self.client.$method(url)
                }
            )+
        }
    };
}

impl_client_wrap!(get, post, put, patch, delete, head);