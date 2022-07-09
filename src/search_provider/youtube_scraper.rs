//! Implementation o&f a search provider by scraping YouTube
use std::time::Duration;

use failure::Fallible;
use headless_chrome::{Browser, LaunchOptionsBuilder};
use serde::{Serialize, Deserialize};

use crate::common::self_setup::SelfSetup;
use super::interface::{ProvideSearch, SearchQuery};

/// Implementation of [Docker's restart policy](
/// https://docs.docker.com/engine/reference/run/#restart-policies---restart)
pub enum RestartPolicy {
    /// Default value: doesn't restart when container exits
    No,
    /// Restarts only when container exists and return code != 0.
    /// Optionally may limit the amount of retries
    /// 
    /// NOTE: the retry delay starts from 100ms and doubles until it hits
    /// retry limit or 1 minute.
    OnFailure(Option<u8>),
    /// Restarts regardless of return code and on daemon start-up.
    /// This will restarts indefinitely
    /// 
    /// NOTE: the retry delay starts from 100ms and doubles until it it 1 minute
    /// or is removed
    Always,
    /// Restarts regardless of the return code and on daemon start-up.
    /// The attempt to restart will stop if the container is put to stop state
    /// (by using `docker stop` or `docker rm`)
    /// 
    /// NOTE: the retry delay starts from 100ms and doubles until it it 1 minute
    /// or is removed
    UnlessStopped
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct DockerConfig {
    /// Additional flags to pass to `docker run`.
    /// This is dependent on the docker version the host has, but generally,
    /// here is the [documentation](https://docs.docker.com/engine/reference/run/)
    /// 
    /// This field for advanced usage on some of the options that are not
    /// yet supported by this crate.
    /// 
    /// Default: []
    additional_flags: Vec<String>,
    /// Whether to run the container in detached mode.
    /// Since the DockerConfig does not have info on the underlying
    /// connection mode provided by the container, prefer setting this
    /// to None and manipulate the connection type instead.
    /// 
    /// TODO: Making this optional is a hacky way around creating
    /// a factory/builder infrastructure that allows fine-grain and
    /// coarse-grain configuration
    /// 
    /// Default: None
    detached_mode: Option<bool>,

}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {  }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)] // must be declared after Deserialize!
pub struct ChromeConfig {
    /// Whether to run this in headless mode. If set false,
    /// this application runs in headful mode, which spawns a
    /// GUI of the Chrome browser.
    /// 
    /// Default: true
    // #[serde(default="ChromeConfig::const_true")]
    headless: bool,
    /// A useful option for poorly configured user (like Docker container).
    /// If we are not sure of permissions, should set this to true.
    /// 
    /// Default: true
    // #[serde(default="ChromeConfig::const_true")]
    sandbox: bool,
    /// Specifies the window size to be rendered to the webkit
    /// This is useful if we need to test webpage for multiple resolutions
    /// If none, we would probably pass this through to default value
    /// from `chrome`
    /// 
    /// Default: None
    // #[serde(default="ChromeConfig::const_none")]
    window_size: Option<(u32, u32)>,
    /// The debugging port to launch. If None, pass through to chrome,
    /// which may use any opening port.
    /// 
    /// Default: None
    // #[serde(default="ChromeConfig::const_none")]
    port: Option<u16>,
    /// Specifies the path to Chrome/Chromium executable
    /// If None, the underlying crate detects the path
    /// 
    /// Default: None
    // #[serde(default="ChromeConfig::const_none")]
    path: Option<std::path::PathBuf>,
    /// How long to keep WebSocket to the browser after the last time
    /// receiving any event from it
    /// 
    /// Default: 30 secs
    // #[serde(default="ChromeConfig::const_30_secs")]
    idle_browser_time: Duration
}

impl Default for ChromeConfig {
    fn default() -> Self {
        Self { 
            headless: true, 
            sandbox: true, 
            window_size: None, 
            port: None, 
            path: None, 
            idle_browser_time: Duration::from_secs(30) 
        }
    }
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum BrowserType {
    /// Uses a proxy; the underlying data is in format: 
    /// "ws://localhost:9222/devtools/browser/019f2fed-ad55-4c34-9ff1-9a61d01011a0"
    Proxy(String),
    Docker(DockerConfig),
    Local(ChromeConfig)
}



impl BrowserType {
    pub fn proxy<AnyStr:AsRef<str>>(proxy_url: AnyStr) -> BrowserType {
        BrowserType::Proxy(proxy_url.as_ref().to_string())
    }
    pub fn docker(config: DockerConfig) -> BrowserType {
        BrowserType::Docker(config)
    }
    pub fn local(config: ChromeConfig) -> BrowserType {
        BrowserType::Local(config)
    }
    /// Automatically parses an object into fitting BrowserType
    /// Returns None if cannot do so.
    pub fn auto(value: serde_json::Value) -> Option<BrowserType> {
        // dumb version parse the exact BrowserType schema
        serde_json::from_value::<BrowserType>(value.clone())
            .or_else(|_err| Self::try_fit(value))
            .ok()
    }
    fn try_fit(value: serde_json::Value) -> Result<BrowserType, serde_json::error::Error> {
        todo!()
    }
}

impl Default for BrowserType {
    fn default() -> Self {
        BrowserType::Local(Default::default())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct YoutubeScraper {
    /// The prioritized backends to try and fallback.
    /// Will happily accept all values of [BrowserType]
    /// 
    /// Default: `vec![BrowserType::Local(ChromeConfig::default())]`
    backends: Vec<BrowserType>
}

impl Default for YoutubeScraper {
    fn default() -> Self {
        Self { backends: vec![Default::default()] }
    }
}

impl SelfSetup for YoutubeScraper {
    fn setup(&self) -> Result<(),String> {
        Ok(())
    }
}

impl YoutubeScraper {
    pub fn new(backends: Vec<BrowserType>) -> Self {
        Self { backends }
    }

    fn attempt_proxy(&self)-> Fallible<Browser> {
        Browser::connect(
            "ws://localhost:9222/devtools/browser/019f2fed-ad55-4c34-9ff1-9a61d01011a0"
            .to_string()
        ).or_else(|_err| Browser::default())
    }
    fn get_links(&self, query: &SearchQuery) -> Result<Vec<String>, failure::Error> {        
        // TODO: WSL doesn't work. Attempt to use a proxy if possible.
        // Otherwise, create even more ways to customize launching headless chrome.
        let browser = self.attempt_proxy()?;
        let url = format!(
            "https://www.youtube.com/results?search_query={}",
            query.keywords.join("+")
        );
        log::info!("url: {url}");

        // NOTE: we cannot use a simple wget-like engine (rust::reqwest is one instance) because
        // YouTube seems to manipulate the DOM at client-side
        // so we need some JavaScript engine to run through the given HTML.
        let tab = browser.wait_for_initial_tab()?;
        tab.navigate_to(&url)?;
        let elems = tab.wait_for_elements("a#video-title")?;
        let velems = elems.iter()
            .filter_map(|e| 
                e.get_attributes().ok()
                  .and_then(|e| e)
                  .and_then(|mut attrs| attrs.remove("href"))
                  .and_then(|watch_url| Some(format!("https://www.youtube.com{watch_url}")))
            ).collect::<Vec<_>>();
        log::info!("elems: {velems:?}");
        Ok(velems)
    }
}

impl ProvideSearch for YoutubeScraper {
    fn search(&self, query: SearchQuery) -> Result<Vec<String>, String> {
        self.get_links(&query).map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod test {
    use crate::search_provider::interface::SearchProviders;

    use super::*;

    fn provide_search_test<AnyStr: AsRef<str>>(provider: SearchProviders, keywords: Vec<String>, expect_contains: AnyStr) 
        -> Result<(), String> 
    {
        let result = provider.search(SearchQuery {keywords})?;
        let expected_str = expect_contains.as_ref();
        assert!(result.len() >= 1, "Result should yield at least 1 link");
        assert!(result.iter().any(|link| link.contains(expected_str)), "Result ({result:?}) should contain \"{expected_str:?}\"");
        Ok(())
    }
    fn split_to_vec<AnyStr: AsRef<str>>(s: AnyStr) -> Vec<String> {
        s.as_ref().split(" ").map(|x| x.to_string()).collect::<Vec<_>>()
    }
    #[test]
    fn youtube_scraper_test() {
        let scraper = YoutubeScraper::default().into();
        provide_search_test(scraper, 
            split_to_vec("ortopilot insomnia"), 
            "ldi3geT3uzw").expect("Provided result does not contain expected substring");

    }
}
