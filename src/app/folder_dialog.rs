use std::path::PathBuf;

pub fn choose_folder() -> Result<Option<PathBuf>, String> {
    Ok(rfd::FileDialog::new()
        .set_title("Choose a photo or video folder")
        .pick_folder())
}
