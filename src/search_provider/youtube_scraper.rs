//! Implementation o&f a search provider by scraping YouTube

use std::{time::Duration, io::BufRead, borrow::Cow};

use enum_dispatch::enum_dispatch;
use failure::Fallible;
use headless_chrome::{Browser};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Serialize, Deserialize};

use crate::common::self_setup::SelfSetup;
use super::interface::{ProvideSearch, SearchQuery};

/// The schema for Docker configuration, which spins up a new Docker container
/// and does port-mapping to allow a [Browser] to connect to this forwarded port.
/// 
/// TODO: We could make this config to look for a similarly configured running
/// Docker container and get its port and go into the Browser
/// if we're only interested in a tab of a specific browser
/// 
/// 
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct DockerConfig {
    /// Additional flags to pass to `docker run`.
    /// This is dependent on the docker version the host has, but generally,
    /// here is the [documentation](https://docs.docker.com/engine/reference/run/)
    /// 
    /// This field for advanced usage on some of the options that are not
    /// yet supported by this crate.
    /// 
    /// Default: [--rm, -d, --cap-add=SYS_ADMIN]
    pub additional_flags: Vec<String>,
    /// The path to the image we're using for the headless Chrome.
    /// This should create a container that communicates via remote debugging port.
    /// 
    /// Default: "docker.io/justinribeiro/chrome-headless:latest"
    pub image_path: String,
    /// The port mapping to the remote debugging port. The first
    /// element is the host's port; the second is the container's port.
    /// 
    /// Example: 8080:80 means we can communicate in the 8080 port, while
    /// the remote debugging port running in the container should exports to 80.
    /// 
    /// Note that Docker is sometimes smart enough to satisfy "192.168.1.100:8080:80",
    /// See more of these in [Docker's Documentation](https://docs.docker.com/config/containers/container-networking/)
    /// 
    /// If this is None, Docker will run a "-P" port, which automatically assigns a working port
    /// to ports declared EXPOSE in the Dockerfile. 
    /// 
    /// Either way of configuring, we will then inspect for the port assigned to
    /// the container using `docker inspect --format="{{json .NetworkSettings.Ports}} <container-id>`
    /// 
    /// Default: None
    pub port_mapping: Option<String>
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self { 
            additional_flags: vec!["--rm".to_string(), "-d".to_string(), "--cap-add=SYS_ADMIN".to_string()],
            image_path: "docker.io/justinribeiro/chrome-headless:latest".to_string(),
            port_mapping: None
        }
    }
}

type MyResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;
fn to_boxed_result<T, E: Into<Box<dyn std::error::Error>>>(res: Result<T, E>) -> Result<T, Box<dyn std::error::Error>> {
    res.map_err(|e| e.into())
}

impl DockerConfig {
    fn try_get_debug_ws_url(line: Cow<str>) -> Option<Cow<str>> {
        // find starting index of "ws://"
        line.find("ws://")
            // gets the substring only if it contains /devtools/browser/
            .and_then(|start_idx| {
                line.contains("/devtools/browser/").then_some(line.chars().skip(start_idx).collect::<Cow<str>>())
            })
    }
    fn debug_ws_from_log<'a, E, R, Lines>(docker_logs: Lines) -> MyResult<Cow<'a, str>> 
        where E: Into<Box<dyn std::error::Error>>, 
              R: Into<Result<String, E>>, 
              Lines: Iterator<Item=R> 
    {
        docker_logs.into_iter()
            // find and return the substring "ws://*/devtools/browser/*"
            .find_map(|res_line|{
                // pattern for exit early in a for loop
                // let rline: Result<String, E> = res_line.into();
                match res_line.into() {
                    Err(err) => Some(Err(err)),
                    Ok(line) => Self::try_get_debug_ws_url(line.into()).map(|v| Ok(v))
                }
            })
            // collapse Option<Result<String, Error>> into Result<String, Box<Error>>; if previously is None, then it's an error
            .map(to_boxed_result)
            .unwrap_or_else(||bail!("docker logs has no line matching \'ws://*/devtools/browser/*\'"))
    }

    fn to_proxy_config(ws_url: String) -> ProxyConfig {
        ProxyConfig::new(ws_url).unwrap()
    }
    fn get_ports<AnyStr: AsRef<str>>(container_id: AnyStr) -> MyResult<Vec<u16>> {
        let mut docker_port = std::process::Command::new("docker");
        docker_port.args(["port", container_id.as_ref()]);
        log::info!(r#"Getting docker ports using command `"{docker_port:?}"`"#);
        docker_port.output()
            .map(|output| output.stdout.lines().filter_map(|res_port|{
                // 9222/tcp -> 0.0.0.0:49153
                // 9223/tcp -> 0.0.0.0:12451
                let port = res_port.unwrap();
                port.find(":")
                    .and_then(|idx| { 
                        port.chars()
                          .skip(idx+1)
                          .collect::<Cow<str>>()
                          .parse::<u16>().ok()
                    })
                })
                .collect()
            )
            .map_err(Into::into)
    }
}
impl ConnectBrowserTrait for DockerConfig {
    fn browser(&self) -> Result<Browser, String> {
        let mut docker_run = std::process::Command::new("docker");
        docker_run.arg("run");
        // add port options
        match &self.port_mapping {
            Some(port_map) => docker_run.args(["-p", port_map.as_ref()]),
            None => docker_run.arg("-P")
        };
        let container_id_vec = docker_run.args(self.additional_flags.iter())
            .arg(&self.image_path)
            .output().expect("docker run command failed")
            .stdout;
        let container_id = String::from_utf8_lossy(&container_id_vec);
        // from the given container_id, determine the components to ws url
        // and use ProxyConfig::from_components to construct
        let docker_logs = 
            std::process::Command::new("docker")
            .arg("logs")
            .arg(container_id.as_ref())
            .output().expect("docker logs should yield OK")
            .stdout;
        let ws_url = Self::debug_ws_from_log(docker_logs.lines());
        let ports = Self::get_ports(container_id.as_ref()).map_err(|err| err.to_string())?;
        ws_url.map_err(|e| e.to_string())
        .and_then(|url| {
            // TODO: What's stopping me from putting Cow everywhere?
            let mut conf_comps = Self::to_proxy_config(url.into_owned()).into_components()?;
            conf_comps.ip = "localhost".to_string();
            let mut failures = Vec::<String>::new();
            ports.iter()
                .find_map(|port| -> Option<Browser>{
                    conf_comps.port = Some(*port);
                    ProxyConfig::from(&conf_comps).browser()
                        .map_err(|err| failures.push(err))
                        .ok()
                })
                .ok_or_else(||format!("None of the port worked:\n{failures:?}"))
            // conf_comps.port = ports.first().map(|v| *v);
            // let conf: ProxyConfig = conf_comps.into();
            // conf.browser()
        })

    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProxyConfig {
    debug_ws_url: String
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProxyConfigComponents {
    pub ip: String,
    pub port: Option<u16>,
    pub token: String
}

impl AsRef<ProxyConfigComponents> for ProxyConfigComponents {
    fn as_ref(&self) -> &ProxyConfigComponents {
        &self
    }
}

impl <T: AsRef<ProxyConfigComponents>> From<T> for ProxyConfig {
    fn from(comp: T) -> Self {
        let comp_ref = comp.as_ref();
        Self::from_components(&comp_ref.ip, comp_ref.port, &comp_ref.token)
    }
}

impl ProxyConfigComponents {
    /// Creates a [ProxyConfigComponents] object
    pub fn new<AnyStr0: AsRef<str>, AnyStr2: AsRef<str>>(ip: AnyStr0, port: Option<u16>, token: AnyStr2) -> Self {
        Self {ip: ip.as_ref().to_string(), port, token: token.as_ref().to_string()}
    }
}

impl ConnectBrowserTrait for ProxyConfig {
    fn browser(&self) -> Result<Browser,String> {
        Browser::connect(self.debug_ws_url.to_string()).map_err(|e| e.to_string())
    }
}

impl ProxyConfig {
    /// Checks whether url is valid
    /// ```
    /// use cli_music_player::search_provider::youtube_scraper::*;
    /// 
    /// assert!(ProxyConfig::is_valid_url("ws://0.0.0.0:1214/devtools/browser/some-token-here"));
    /// assert!(ProxyConfig::is_valid_url("ws://public.ip:15/devtools/browser/some-token-1415lg"));
    /// assert!(ProxyConfig::is_valid_url("ws://public.ip.no-port/devtools/browser/some-token-1415lg"));
    /// 
    /// assert!(!ProxyConfig::is_valid_url("ws://no.token.given/devtools/browser/"));
    /// assert!(!ProxyConfig::is_valid_url("http://bad.protocol/devtools/browser/some-token"));
    /// assert!(!ProxyConfig::is_valid_url("ws://no.path.given:15"))
    /// ```
    pub fn is_valid_url<AnyStr: AsRef<str>>(url: AnyStr) -> bool {
        lazy_static! {
            // TODO: Is optional capture group supported?
            static ref URL_REGEX: Regex = Regex::new(r"ws://(?P<url>[^:/]*)(?P<port>:\d*)?/devtools/browser/(?P<token>.+$)").unwrap();
        };
        URL_REGEX.is_match(url.as_ref())
    }
    /// Constructs a new [ProxyConfig] structure
    /// ```
    /// use cli_music_player::search_provider::youtube_scraper::*;
    /// 
    /// fn opt_config<AnyStr: AsRef<str>>(url: AnyStr) -> Option<ProxyConfig> {
    ///     ProxyConfig::new(url).ok()
    /// }
    /// 
    /// assert!(matches!(opt_config("ws://0.0.0.0:1214/devtools/browser/some-token-here"), Some(_)));
    /// assert!(matches!(opt_config("ws://public.ip:15/devtools/browser/some-token-1415lg"), Some(_)));
    /// assert!(matches!(opt_config("ws://public.ip.no-port/devtools/browser/some-token-1415lg"),Some(_)));
    /// 
    /// assert_eq!(None, opt_config("ws://no.token.given/devtools/browser/"));
    /// assert_eq!(None, opt_config("http://bad.protocol/devtools/browser/some-token"));
    /// assert_eq!(None, opt_config("ws://no.path.given:15"))
    /// ```
    pub fn new<AnyStr: AsRef<str>>(debug_ws_url: AnyStr) -> Result<Self, String> {
        Self::is_valid_url(&debug_ws_url).then_some(
            Self {debug_ws_url: debug_ws_url.as_ref().to_string()}
        ).ok_or(format!("url {} doesn't conform to format", debug_ws_url.as_ref()))
    }
    pub fn from_components<AnyStr0: AsRef<str>, AnyStr2: AsRef<str>>(ip: AnyStr0, port: Option<u16>, token: AnyStr2) -> Self {
        let port_str = port.map(|p| format!(":{}", p)).unwrap_or_default();
        Self::new(format!("ws://{}{}/devtools/browser/{}", ip.as_ref(), port_str, token.as_ref())).unwrap()
    }
    /// Turns [ProxyConfig] into [ProxyConfigComponents]
    /// ```
    /// use cli_music_player::search_provider::youtube_scraper::*;
    /// fn to_component<AnyStr: AsRef<str>>(url: AnyStr)->Option<ProxyConfigComponents> {
    ///     ProxyConfig::new(url).and_then(|proxy| proxy.into_components()).ok()
    /// }
    /// assert_eq!(
    ///     Some(ProxyConfigComponents::new("0.0.0.0", Some(1214), "some-token-here")), 
    ///     to_component("ws://0.0.0.0:1214/devtools/browser/some-token-here")
    /// );
    /// assert_eq!(
    ///     Some(ProxyConfigComponents::new("public.ip", Some(15), "some-token-1415lg")),
    ///     to_component("ws://public.ip:15/devtools/browser/some-token-1415lg")
    /// );
    /// 
    /// assert_eq!(
    ///     Some(ProxyConfigComponents::new("public.ip.no-port", None, "some-token-1415lg")),
    ///     to_component("ws://public.ip.no-port/devtools/browser/some-token-1415lg")
    /// );
    /// 
    /// assert_eq!(None, to_component("ws://no.token.given/devtools/browser/"));
    /// assert_eq!(None, to_component("http://bad.protocol/devtools/browser/some-token"));
    /// assert_eq!(None, to_component("ws://no.path.given:15"))
    /// ```
    pub fn into_components(self) -> Result<ProxyConfigComponents, String> {
        let url = self.debug_ws_url;
        let removed_protocol = url.chars().skip("ws://".len()).collect::<Cow<str>>();
        let port_start = removed_protocol.find(":");
        let port_end = removed_protocol.find("/").ok_or("No path, hence token, provided.".to_string())?;
        let addr_end = port_start.unwrap_or(port_end);

        let port_str = port_start.map(|mut start| {
            start += 1; 
            removed_protocol.chars().skip(start).take(port_end-start).collect::<Cow<str>>()
        });
        println!("{port_str:?}");
        Ok(ProxyConfigComponents::new(
            removed_protocol.chars().take(addr_end).collect::<Cow<str>>(), 
            port_str.map(|port| port.parse().unwrap()),
            removed_protocol.chars().skip(port_end).skip("/devtools/browser/".len()).collect::<Cow<str>>()
        ))
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

impl ConnectBrowserTrait for ChromeConfig {
    fn browser(&self) -> Result<Browser,String> {
        let mut conf = headless_chrome::LaunchOptionsBuilder::default();
        conf.headless(self.headless)
            .port(self.port)
            .sandbox(self.sandbox)
            .window_size(self.window_size)
            .path(self.path.clone())
            .idle_browser_timeout(self.idle_browser_time)
            .build()
            .and_then(|opts| Browser::new(opts).map_err(|e| e.to_string()))
    }
}

/// The implementing struct should be the configuration
/// and implements this traits which can create a browser
/// connection to do youtube scraping.
/// 
/// Note that the configuration may be empty, in which
/// this effectively points to [BrowserType] as a
/// single tag that controls how the browser is created/communicated
/// 
/// Generally, this trait refers to the ability to connect to a browser,
/// but the implementation may go even further to attempt to create
/// the instance for the connection, where it would implements
/// [SelfSetup]
#[enum_dispatch]
pub trait ConnectBrowserTrait {
    /// Creates a way to communicate with the browser from the
    /// given configuration object.
    fn browser(&self) -> Result<Browser, String>;
}

/// Represents the way we could connect to a [Browser].
/// 
/// This enum implements the [ConnectBrowserTrait]
#[enum_dispatch(ConnectBrowserTrait)]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum BrowserType {
    /// Uses a proxy; the underlying data is in format: 
    /// "ws://localhost:9222/devtools/browser/019f2fed-ad55-4c34-9ff1-9a61d01011a0"
    Proxy(ProxyConfig),
    Docker(DockerConfig),
    Local(ChromeConfig)
}



impl BrowserType {
    pub fn proxy<AnyStr:AsRef<str>>(proxy_url: AnyStr) -> BrowserType {
        BrowserType::Proxy(ProxyConfig::new(proxy_url).expect("Cannot construct ProxyConfig"))
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
        todo!("This iterates through each schema of BrowerType and attempt to parse each of them")
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
    /// 
    /// Referred: [BrowserType::Local], [ChromeConfig]
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
        log::info!("Results: {result:?}");
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
