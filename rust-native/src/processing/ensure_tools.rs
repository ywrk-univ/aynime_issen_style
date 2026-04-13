use anyhow::{Context, Result};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const FFMPEG_ZIP_URL: &str =
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip";

const APP_DIR_NAME: &str = "aynime-issen";

/// Get the app-local data directory: %LOCALAPPDATA%/aynime-issen/
/// Hidden from the user (not in Documents).
fn app_local_dir() -> Result<PathBuf> {
    let local_app_data = std::env::var("LOCALAPPDATA")
        .context("LOCALAPPDATA environment variable not set")?;
    Ok(PathBuf::from(local_app_data).join(APP_DIR_NAME))
}

/// Get the tools directory: %LOCALAPPDATA%/aynime-issen/tools/
fn tools_dir() -> Result<PathBuf> {
    Ok(app_local_dir()?.join("tools"))
}

/// Get the default output directory: ~/Documents/aynime-issen/
pub fn default_output_dir() -> PathBuf {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        PathBuf::from(profile).join("Documents").join(APP_DIR_NAME)
    } else {
        PathBuf::from("./output")
    }
}

/// Ensure ffmpeg.exe is available locally. Downloads if not present.
/// Stored in %LOCALAPPDATA%/aynime-issen/tools/ffmpeg/
pub fn ensure_ffmpeg() -> Result<PathBuf> {
    ensure_web_tool(FFMPEG_ZIP_URL, "ffmpeg.exe")
}

fn ensure_web_tool(zip_url: &str, tool_filename: &str) -> Result<PathBuf> {
    let tool_stem = Path::new(tool_filename)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let tool_dir = tools_dir()?.join(&tool_stem);

    // Already exists?
    if let Some(path) = find_file_recursive(&tool_dir, tool_filename) {
        log::info!("Found {}: {}", tool_filename, path.display());
        return Ok(path);
    }

    log::info!("{} not found, downloading from {}", tool_filename, zip_url);

    fs::create_dir_all(&tool_dir)
        .with_context(|| format!("Failed to create directory {}", tool_dir.display()))?;

    let zip_path = tool_dir.join(format!("{}.zip", tool_stem));
    download_file(zip_url, &zip_path)?;

    extract_zip(&zip_path, &tool_dir)?;

    let _ = fs::remove_file(&zip_path);

    find_file_recursive(&tool_dir, tool_filename)
        .with_context(|| format!("{} not found after extraction in {}", tool_filename, tool_dir.display()))
}

fn download_file(url: &str, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest.parent().unwrap())?;

    let part_path = dest.with_extension(format!(
        "{}.part",
        dest.extension().unwrap_or_default().to_string_lossy()
    ));

    let output = std::process::Command::new("curl")
        .args([
            "-L",
            "-f",
            "--connect-timeout", "30",
            "-o",
            &part_path.to_string_lossy(),
            url,
        ])
        .output()
        .context("Failed to run curl. Is curl available?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Download failed: {}", stderr);
    }

    fs::rename(&part_path, dest)
        .with_context(|| format!("Failed to rename {} to {}", part_path.display(), dest.display()))?;

    log::info!("Downloaded: {}", dest.display());
    Ok(())
}

fn extract_zip(zip_path: &Path, dest_dir: &Path) -> Result<()> {
    log::info!("Extracting {} to {}", zip_path.display(), dest_dir.display());

    let file = fs::File::open(zip_path)
        .with_context(|| format!("Failed to open {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .context("Failed to read zip archive")?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("Failed to read zip entry")?;

        let entry_path = match entry.enclosed_name() {
            Some(p) => p.to_owned(),
            None => continue,
        };

        let out_path = dest_dir.join(&entry_path);

        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out_file = fs::File::create(&out_path)
                .with_context(|| format!("Failed to create {}", out_path.display()))?;
            io::copy(&mut entry, &mut out_file)?;
        }
    }

    log::info!("Extraction complete");
    Ok(())
}

fn find_file_recursive(dir: &Path, filename: &str) -> Option<PathBuf> {
    if !dir.exists() {
        return None;
    }

    let mut candidates: Vec<(i32, PathBuf)> = Vec::new();

    fn walk(dir: &Path, filename: &str, candidates: &mut Vec<(i32, PathBuf)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, filename, candidates);
            } else if path.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(filename)) {
                let depth = path.components().count() as i32;
                let has_bin = path
                    .components()
                    .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case("bin"));
                let score = if has_bin { depth - 100 } else { depth };
                candidates.push((score, path));
            }
        }
    }

    walk(dir, filename, &mut candidates);
    candidates.sort_by_key(|(score, _)| *score);
    candidates.into_iter().next().map(|(_, p)| p)
}
