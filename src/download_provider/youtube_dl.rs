use crate::common::self_setup::SelfSetup;

use super::interface::{ProvideDownload, DownloadConfig};

pub struct YoutubeDL;

impl SelfSetup for YoutubeDL {
    fn setup(&self) -> Result<(), String> {
        todo!()
    }
}

impl ProvideDownload for YoutubeDL {
    fn download(&self, config: DownloadConfig) -> Result<(),String> {
        todo!()
    }
}