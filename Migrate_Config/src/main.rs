use std::{
    ffi::OsStr,
    fs::OpenOptions,
    io::{Write, stdin, stdout},
    path::{Path, PathBuf},
};

use lofty::{
    config::WriteOptions,
    file::AudioFile,
    prelude::{TagExt, TaggedFileExt},
    read_from_path,
    tag::{ItemKey, Tag, TagType},
};
use rusqlite::{Connection, Error};
use walkdir::WalkDir;

// This is a quick script to migrate the changes made in the database current version `0.2.0` to `0.3.0`

fn migrate_config(db_path: &str) -> Result<(), Error> {
    let conn = Connection::open(db_path)?;

    // Prompt before dropping colours_table
    if prompt_yes_no(
        "Do you want to drop the 'colours_table' if it exists? (This is removed in version `0.3.0`)",
    ) {
        if conn
            .execute("DROP TABLE IF EXISTS colours_table;", [])
            .is_ok()
        {
            println!("Dropped colours_table");
        } else {
            println!("Failed to drop colours_table");
        }
    } else {
        println!("Skipped dropping colours_table");
    }

    // Prompt before dropping download_args column
    if prompt_yes_no(
        "Do you want to drop the 'download_args' column from 'music_config'? (This has been revamped in version `0.3.0`)",
    ) {
        if conn
            .execute("ALTER TABLE music_config DROP COLUMN download_args;", [])
            .is_ok()
        {
            println!("Dropped download_args column from music_config");
        } else {
            println!("Failed to drop download_args column from music_config");
        }
    } else {
        println!("Skipped dropping download_args column");
    }

    Ok(())
}

fn prompt_yes_no(question: &str) -> bool {
    loop {
        print!("{} (y/n): ", question);
        stdout().flush().expect("Failed to flush stdout");

        let mut input = String::new();
        if stdin().read_line(&mut input).is_err() {
            println!("Failed to read input. Please try again.");
            continue;
        }

        match input.trim().to_lowercase().as_str() {
            "y" | "yes" => return true,
            "n" | "no" => return false,
            _ => println!("Please enter 'y' or 'n'."),
        }
    }
}

fn get_metadata_key<T>(tag: &Tag, key: &str) -> Option<T>
where
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    let tag_types = [
        TagType::Ape,
        TagType::Id3v1,
        TagType::Id3v2,
        TagType::Mp4Ilst,
        TagType::VorbisComments,
        TagType::RiffInfo,
        TagType::AiffText,
    ];

    for tag_type in &tag_types {
        let item_key = ItemKey::from_key(*tag_type, key);
        if let Some(value) = tag.get_string(&item_key) {
            return value.parse::<T>().ok();
        }
    }
    None
}

fn is_audio_file(path: &Path) -> bool {
    match path.extension().and_then(OsStr::to_str) {
        Some(ext) => matches!(
            ext.to_ascii_lowercase().as_str(),
            "mp3" | "flac" | "ogg" | "wav" | "m4a" | "aiff" | "opus"
        ),
        None => false,
    }
}

fn process_file(path: &Path, log_file: &mut std::fs::File) {
    match read_from_path(path) {
        Ok(mut tagged_file) => {
            let tag = if let Some(primary) = tagged_file.primary_tag_mut() {
                primary
            } else if let Some(first) = tagged_file.first_tag_mut() {
                first
            } else {
                eprintln!("No tag found in file: {}", path.display());
                return;
            };

            if let Some(user_rating) = get_metadata_key::<u8>(tag, "USERRATING") {
                if let Some(filename) = path.file_name().and_then(OsStr::to_str) {
                    println!("{}: USERRATING -> Rating", filename);
                }

                tag.insert_text(ItemKey::Popularimeter, user_rating.to_string());

                for tag_type in [
                    TagType::Ape,
                    TagType::Id3v1,
                    TagType::Id3v2,
                    TagType::Mp4Ilst,
                    TagType::VorbisComments,
                    TagType::RiffInfo,
                    TagType::AiffText,
                ] {
                    let item_key = ItemKey::from_key(tag_type, "USERRATING");
                    if tag.contains(&item_key) {
                        tag.remove_key(&item_key);
                    }
                }

                if let Err(e) = tagged_file.save_to_path(path, WriteOptions::default()) {
                    eprintln!("Failed to save {}: {}", path.display(), e);
                } else {
                    // Log the full path of the edited file
                    if let Err(e) = writeln!(log_file, "{}", path.display()) {
                        eprintln!("Failed to write to log file: {}", e);
                    }
                }
            }
        }
        Err(err) => {
            println!("Skipping unsupported file error: {}", err);
        }
    }
}

fn main() -> std::io::Result<()> {
    // Get the path of the current executable
    let exe_path = std::env::current_exe()?;

    // Get the directory containing the executable
    let exe_dir = exe_path
        .parent()
        .expect("Executable must be in some directory");

    // Construct the full path to the database file as a String
    let db_path = exe_dir
        .join("Config.sqlite3")
        .to_str()
        .expect("Path is not valid UTF-8")
        .to_string();

    // Use db_path in your migrate_config function
    if let Err(err) = migrate_config(&db_path) {
        eprintln!("Error: {}", err);
    }

    // Construct the full path for the SCache
    let file_path = exe_dir.join("SCache.sqlite3");

    // Delete the file if it exists
    if file_path.exists() {
        std::fs::remove_file(&file_path)?;
        println!("Deleted SCache.sqlite3 from exe dir");
    } else {
        println!("SCache.sqlite3 does not exist in exe dir no action taken");
    }

    // Prompt user for folder path or 'q' to quit
    let target_dir: PathBuf = loop {
        println!(
            "This update changes `user_rating` metadata field to `rating` if you used the rating button or field and want to migrate your songs to use `rating` instead of `user_rating`"
        );
        println!(
            "Then input the directory the songs are in below and it will migrate the `user_rating` metadata back to the files in the `rating` field"
        );
        println!("Enter a folder path to process (or 'q' to quit): ");
        stdout().flush()?;

        let mut input = String::new();
        stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.eq_ignore_ascii_case("q") {
            println!("Quitting...");
            return Ok(());
        }

        let path = Path::new(input);

        if path.is_dir() {
            break path.to_path_buf();
        } else {
            println!(
                "Error: '{}' is not a valid directory. Please try again.",
                input
            );
        }
    };

    // Construct the full path for the log file
    let log_path = exe_dir.join("log.txt");

    // Open (or create) the log file in append mode
    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    println!("Logging progress to: {}", log_path.display());

    for entry in WalkDir::new(&target_dir).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.is_file() && is_audio_file(path) {
            process_file(path, &mut log_file);
        }
    }

    println!("Processing complete.");

    Ok(())
}
