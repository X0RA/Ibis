use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Discover photos and videos recursively and sort them by capture date"
)]
pub struct Args {
    /// Open a media file with its containing folder, or browse a folder
    #[arg(value_name = "PATH", conflicts_with = "directory")]
    pub path: Option<PathBuf>,
    /// Directory to scan recursively (otherwise opens a folder chooser)
    #[arg(short = 'r', long = "recursive", value_name = "DIRECTORY")]
    pub directory: Option<PathBuf>,
    /// Print the sorted media list without opening a window
    #[arg(long, requires = "directory")]
    pub list: bool,
    /// Decoded photo cache budget in MiB (excludes active decoder working memory)
    #[arg(long, default_value_t = 256, value_parser = clap::value_parser!(u32).range(32..=4096))]
    pub cache_mib: u32,
}
