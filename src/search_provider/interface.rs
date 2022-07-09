use enum_dispatch::enum_dispatch;
use serde::{Serialize, Deserialize};

use crate::common::self_setup::SelfSetup;

use super::youtube_scraper::YoutubeScraper;

#[derive(Serialize, Deserialize)]
pub struct SearchQuery {
    /// Guaranteed to be separated by word with no space
    pub keywords: Vec<String>,
}


#[enum_dispatch]
pub trait ProvideSearch: SelfSetup {
    /// Satisfies a query using keywords
    /// it should returns a vector of URLS
    fn search(&self, query: SearchQuery) -> Result<Vec<String>, String>;
}

#[enum_dispatch(SelfSetup, ProvideSearch)]
pub enum SearchProviders {
    YoutubeScraper
}


#[cfg(test)]
mod test {
    use crate::search_provider::youtube_scraper::BrowserType;

    use super::*;
    fn prefer_proxy() -> YoutubeScraper {
        YoutubeScraper::new(vec![
            BrowserType::proxy("ws://localhost:9222/devtools/browser/019f2fed-ad55-4c34-9ff1-9a61d01011a0"),
            BrowserType::default()
        ])
    }
    #[test]
    fn search_provider_init() {
        let sp: SearchProviders = prefer_proxy().into();
        let result = sp.search(SearchQuery {keywords: vec![]});
        // since we put no keyword, the provider may refuse the search.
        log::info!("Result: {result:?}")
    }
}