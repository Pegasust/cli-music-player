use directories::ProjectDirs;

pub fn project_dirs() -> ProjectDirs { 
    ProjectDirs::from("com", "Pegasust", "cli-music-player").unwrap()
}