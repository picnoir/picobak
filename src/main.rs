use std::fs::{create_dir_all, copy};
use std::process::Command;
use std::{fs::File, path::PathBuf};
use std::path::Path;
use std::fmt;
use std::sync::Mutex;

use clap::Parser;
use exif::{Tag, In, Value};
use chrono::{Utc, DateTime, Datelike, NaiveDateTime};
use indicatif::ParallelProgressIterator;
use rayon::prelude::*;
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct CliArgs {
    /// Pictures library directory
    backup_root: String,
    /// Picture to backup. Alternatively, you can send a list of
    /// pictures to backup via stdin.
    file_path: Option<String>,
}

/// Structure used to parse the JSON output of the exiftool program.
#[derive(Debug, Deserialize)]
struct ExifToolEntry {
    #[serde(rename(deserialize = "CreateDate"))]
    create_date: Option<String>
}

enum BackupSuccess {
    AlreadyBackup(String),
    Backup(String, PictureDatetimeOrigin)
}

enum BackupFailure {
    AlreadyBackupButDifferent(String),
    CopyError(String),
    IncorrectFilename(String)
}

enum PictureDatetimeOrigin {
    Exif,
    ExifTool,
    FilesystemMetadata
}

static CREATE_DIR_MUTEX: Mutex<()> = Mutex::new(());

impl fmt::Display for BackupFailure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::AlreadyBackupButDifferent(s) => write!(f, "{}: already exists in the photo library but has a different content", s),
            Self::CopyError(s) => write!(f, "Copy error, {}", s),
            Self::IncorrectFilename(s) => write!(f, "Incorrect Filename, {}", s)
        }
    }
}

fn main() {
    let cli = CliArgs::parse();

    validate_args(&cli);
    let stdin = std::io::stdin();
    let filepaths = match cli.file_path {
        Some(ref fp) => vec!(Ok(fp.to_string())),
        None => stdin.lines()
            .map(|l| l.map_err(|_|BackupFailure::IncorrectFilename(String::from("Can't parse filename from stdin"))))
            .collect()
    };

    let filepaths_len = filepaths.len() as u64;
    let res: Vec<Result<BackupSuccess, BackupFailure>> = filepaths
        .into_par_iter()
        .progress_count(filepaths_len)
        .map(|filepathres| {
            let filepath = filepathres?;
            backup_file(&cli, &filepath)
        })
        .collect();

    display_backup_result(res)
}

fn display_backup_result(results: Vec<Result<BackupSuccess, BackupFailure>>) {
    let mut nb_copy_exif: u32 = 0;
    let mut nb_copy_exiftool: u32 = 0;
    let mut nb_copy_filesystem: u32 = 0;
    let mut nb_duplicates: u32 = 0;
    let mut failures: Vec<BackupFailure> = Vec::new();
    results.into_iter().for_each(|e| match e {
        Ok(s) => {
            match s {
                BackupSuccess::AlreadyBackup(_) => nb_duplicates +=1,
                BackupSuccess::Backup(_, origin) => match origin {
                    PictureDatetimeOrigin::Exif => nb_copy_exif +=1,
                    PictureDatetimeOrigin::ExifTool => nb_copy_exiftool +=1,
                    PictureDatetimeOrigin::FilesystemMetadata => nb_copy_filesystem +=1
                }
            }
        },
        Err(f) => failures.push(f)
    });
    eprintln!("Backup Statistics:");
    eprintln!("==================");
    eprintln!("Duplicates: {}", nb_duplicates);
    eprintln!("Copied: {}", nb_copy_exif + nb_copy_filesystem);
    eprintln!("To classify these newly copied files, we used:");
    eprintln!("   {}: EXIF metadata", nb_copy_exif);
    eprintln!("   {}: the exiftool program", nb_copy_exiftool);
    eprintln!("   {}: filesystem metadata", nb_copy_filesystem);
    eprintln!("Failures: {}", failures.len());
    if !failures.is_empty() {
        eprintln!("\nWARNING: unable to backup some files:");
        failures.iter().for_each(|f| eprintln!("{}", f));
    }
}

/// Backup a file.
fn backup_file(cli: &CliArgs, file_path: &str) -> Result<BackupSuccess, BackupFailure> {
    let filename = Path::new(file_path);
    let file = File::open(filename).map_err(
        |e| BackupFailure::CopyError(format!("cannot open the {} file: {}", file_path, e))
    )?;
    let (datetime, origin) = get_picture_datetime(file_path, &file);

    let picture_dir = find_backup_dir(&cli.backup_root, &datetime);
    upsert_picture_directory(&picture_dir);

    let filename_name = filename.file_name()
        .ok_or_else(|| BackupFailure::IncorrectFilename(
            format!("Incorrect file name {}", filename.display())))?;
    let target_filename = picture_dir.join(filename_name);
    if !target_filename.is_file() {
        match copy(filename, &target_filename) {
            Ok(_) => Ok(BackupSuccess::Backup(
                target_filename.into_os_string().into_string().unwrap(),
                origin)),
            Err(_) => Err(BackupFailure::CopyError(String::from(file_path)))

        }
    } else if same_files(filename, &target_filename) {
        Ok(BackupSuccess::AlreadyBackup(String::from(file_path)))
    } else {
        Err(BackupFailure::AlreadyBackupButDifferent(format!("{} => {}", file_path, target_filename.display())))
    }
}

fn upsert_picture_directory(picture_dir: &PathBuf) {
    // Prevent concurrent directory creation by locking a mutex.
    let _lock = CREATE_DIR_MUTEX.lock();
    if !picture_dir.exists() {
            create_dir_all(picture_dir)
            .unwrap_or_else(
                |e| panic!("ERROR: cannot create the backup directory {}: {}", &picture_dir.display(), e)
            );
    } else if !picture_dir.is_dir() {
        panic!("ERROR: {} already exists and is not a directory. Can't use it to store a picture.", &picture_dir.display())
    }
}

/// Retrieves when the picture has been shot from the EXIF metadata.
/// If no datetime EXIF data is attached to the file, use the file
/// last modification date.
fn get_picture_datetime(file_path: &str, file: &File) -> (DateTime<Utc>, PictureDatetimeOrigin) {
    // Try exif crate.
    get_picture_exif_datetime(file).map(|dt| (dt, PictureDatetimeOrigin::Exif))
        // Exif failed, shell out to exiftool.
        .or_else(|| {
                 get_picture_exiftool_datetime(file_path)
                .map(|dt| (dt, PictureDatetimeOrigin::ExifTool))})
        // Exiftool failed as well. Fallback to Unix datetime.
        .unwrap_or((get_file_modified_time(file_path, file), PictureDatetimeOrigin::FilesystemMetadata))
}

/// Retrieves the picture EXIF datetime.
fn get_picture_exif_datetime(file: &File) -> Option<DateTime<Utc>> {
    let mut bufreader = std::io::BufReader::new(file);
    let exifreader = exif::Reader::new();
    let exif = exifreader.read_from_container(&mut bufreader).ok()?;
    let datetime_field = exif.get_field(Tag::DateTimeOriginal, In::PRIMARY)?;
    match datetime_field.value {
        Value::Ascii(ref vec) if !vec.is_empty() => {
            // Meh… I know…
            let str_date = String::from_utf8(vec[0].to_vec()).unwrap();
            NaiveDateTime::parse_from_str(&str_date, "%Y:%m:%d %H:%M:%S")
                .map(|naive_datetime| DateTime::from_utc(naive_datetime, Utc))
                .ok()
        },
        _ => None
    }
}

/// Shells out to the exiftool CLI. Despite its name, exiftool parses
/// much more metadata than exif. Such as MOV metadata.
fn get_picture_exiftool_datetime(file_path: &str) -> Option<DateTime<Utc>> {
    let output = Command::new("exiftool")
        .args(["-j", "-P", "-CreateDate", file_path])
        .output()
        .ok()?;
    if !output.status.success() {
        return None
    }
    let stdout_str = String::from_utf8(output.stdout).ok()?;
    let parsed_output: Vec<ExifToolEntry> = serde_json::from_str(&stdout_str).ok()?;
    if parsed_output.len() != 1 {
        None
    } else {
        let entry = parsed_output.first()?;
        let date: &str = entry.create_date.as_ref()?;
        NaiveDateTime::parse_from_str(date, "%Y:%m:%d %H:%M:%S")
            .map(|naive_datetime| DateTime::from_utc(naive_datetime, Utc))
            .ok()
    }
}

/// If we cannot load the EXIF creation datetime, we end up using the
/// last modified time of the file.
fn get_file_modified_time(file_path: &str, file: &File) -> DateTime<Utc> {
    let systemtime = file.metadata()
        .unwrap_or_else(|_| panic!("Cannot retrieve UNIX file metadata for {}", file_path))
        .modified()
        .unwrap_or_else(|_| panic!("Cannot retrieve modified time for {}", file_path));
    systemtime.into()
}

/// Return directory in which we want to save the picture.
fn find_backup_dir(backup_root: &str, datetime: &DateTime<Utc>) -> PathBuf {
    let backup_root = Path::new(backup_root);
    backup_root
        .join(format!("{:04}", datetime.year()))
        .join(format!("{:02}", datetime.month()))
        .join(format!("{:02}", datetime.day()))
}

/// Sanity function making sure the user did not give us complete
/// garbage data.
fn validate_args(args: &CliArgs) {
    match &args.file_path {
        Some(file_path) => {
            if !Path::new(&file_path).is_file() {
                panic!("ERROR: {} is not a file", &file_path);
            };
        }
        None => ()
    };

    if Path::new(&args.backup_root).is_file() {
        panic!("ERROR: {} is a file, not a valid backup dir", &args.backup_root);
    };

    let exif_tool_in_path = Command::new("bash")
        .args(["-c", "command exiftool"])
        .output()
        .ok()
        .map(|e| e.status.success())
        .unwrap();
    if !exif_tool_in_path {
        eprintln!("Exiftool doesn't seem to be present in $PATH. Install it if you want to be able to extract more pictures metadata");
    }
}

/// Compare two files and check if they're the same. We're not really
/// comparing the whole file, it'd be too expensive. We assume that if
/// two pictures have the same EXIF data, the same size and the same
/// creation date, they're the same.
fn same_files(source: &Path, target: &Path) -> bool {
    let source_file = File::open(source)
        .unwrap_or_else(|_| panic!("Error: cannot open file {}", &source.display()))
        .metadata()
        .unwrap_or_else(|_| panic!("Error: cannot get metadata of  file {}", &source.display()));
    let target_file = File::open(target)
        .unwrap_or_else(|_| panic!("Error: cannot open file {}", &target.display()))
        .metadata()
        .unwrap_or_else(|_| panic!("Error: cannot get metadata of  file {}", &target.display()));
    source_file.len() == target_file.len()
}
