use std::path::PathBuf;

use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct SetupConfig {
    pub include_path: PathBuf,
    pub lib_path: PathBuf,
    pub exec_path: PathBuf,
    pub data_path: PathBuf
}

#[enum_dispatch]
pub trait SelfSetup {
    fn setup(&self) -> Result<(), String>;
}