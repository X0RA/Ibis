mod app;
mod cli;
mod media;
mod paths;

/// Embedded by Cargo; release CI sets this from the resolved GitHub release.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
pub(crate) fn test_image(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test_images")
        .join(relative)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use clap::Parser;
    let args = cli::Args::parse();
    if args.list {
        let report = media::scan(args.directory.as_deref().expect("required by clap"))?;
        for image in &report.images {
            println!(
                "{}\t{}\t{}",
                image.date_label(),
                image.source,
                image.path.display()
            );
        }
        eprintln!(
            "{} media files; {} warnings",
            report.images.len(),
            report.warnings.len()
        );
        for warning in report.warnings {
            eprintln!("{warning}");
        }
        return Ok(());
    }
    app::run(
        args.path.or(args.directory),
        usize::try_from(u64::from(args.cache_mib) * 1024 * 1024)?,
    )?;
    Ok(())
}
