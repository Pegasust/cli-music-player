use std::{path::{PathBuf}, str::FromStr};

use directories::ProjectDirs;
use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};

use crate::common::{self_setup::SelfSetup, factory::Factory, config::project_dirs};

use super::youtube_dl::YoutubeDL;

#[derive(Serialize, Deserialize)]
pub struct DownloadConfig {
    pub uri: String,
    pub local_path: PathBuf
}

// TODO: Implement macros:
// #[default_factory(local_path=".")]
// struct DownloadConfigDefault;
// #[forward_factory]
// struct DownloadConfigForward;
// #[enum_dispatch_specialize(Factory<DownloadConfig>)]
// pub trait DownloadConfigFactory;

// TODO: this should become a macro.
// In retrospect, this seems like a good opportunity to inject validation
// For example: put in a pattern to check the string is in valid format
// or try to recommend the well-formed input.
#[derive(Default)]
pub struct DownloadConfigForward;
impl DownloadConfigForward {
    /// Creates an instance of [DownloadConfigForward]
    /// ```
    /// use cli_music_player::download_provider::DownloadConfigForward;
    /// let forward_factory = DownloadConfigForward::new(); 
    /// ```
    pub fn new()->Self {DownloadConfigForward{}}
}

impl Factory<DownloadConfig> for DownloadConfigForward {
    /// Generates by forwarding the declaration of a json object
    fn generate(&self, args: serde_json::Value) -> DownloadConfig {
        serde_json::from_value(args).unwrap()
    }
}

pub struct DownloadConfigFromURI {
    local_path: PathBuf
}

impl DownloadConfigFromURI {
    fn from_dir(local_path: PathBuf) -> Self {
        DownloadConfigFromURI{local_path}
    }
    pub fn new(local_path: PathBuf)->Self {
        local_path.is_dir().then_some(local_path).and_then(|path| Some(Self::from_dir(path)))
            .unwrap_or_default()
    }
    pub fn from_str<AnyStr: AsRef<str>>(local_path: AnyStr)->Result<Self, String> {
        // validate whether local_path is valid path
        let path_str = local_path.as_ref();
        let local = PathBuf::from_str(path_str);
        local.and_then(|path| Ok(Self::new(path)))
            .map_err(|except| format!("Error doing PathBuf::from_str on {path_str:?}: {except:?}"))
    }
}

impl Default for DownloadConfigFromURI {
    fn default() -> Self {
        // aggressively make dir if not exist
        let default: PathBuf = project_dirs().data_dir().to_path_buf();
        if !default.exists() {
            if let Err(err) = std::fs::create_dir_all(default.clone()) {
                panic!("Cannot std::fs::create_dir_all({default:?}): {err:?}");
            }
        }
        Self::from_dir(default)
    }
}
impl Factory<DownloadConfig> for DownloadConfigFromURI {
    /// Generates a download config from a specified URI
    /// 
    fn generate(&self, args: serde_json::Value) -> DownloadConfig {
        let map = args.as_object().unwrap();
        let uri = map.get("uri").and_then(|uri| uri.as_str());
        DownloadConfig {
            uri: uri.unwrap().to_string(),
            local_path: self.local_path.clone()
        }
    }
}

// NOTE: we need to specialize this; otherwise, enum_dispatch
// thinks Factory<DownloadConfig> as a generic pass-through.
pub trait DownloadConfigFactory: Factory<DownloadConfig> {}
impl <T: Factory<DownloadConfig>>DownloadConfigFactory for T {}

#[enum_dispatch(DownloadConfigFactory)]
pub enum DownloadConfigFactoryEnum {
    DownloadConfigForward
}

#[enum_dispatch]
pub trait ProvideDownload where Self: SelfSetup {
    /// Downloads based on the given config
    fn download(&self, config: DownloadConfig) -> Result<(), String>;
}

#[enum_dispatch(SelfSetup, ProvideDownload)]
pub enum DownloadProviders {
    YoutubeDL
}