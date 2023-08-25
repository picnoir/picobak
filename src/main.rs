use std::fs::{create_dir_all, copy};
use std::{fs::File, path::PathBuf};
use std::path::Path;
use std::fmt;
use std::sync::Mutex;

use clap::Parser;
use exif::{Tag, In, Value};
use chrono::{Utc, DateTime, Datelike, NaiveDateTime};
use indicatif::ParallelProgressIterator;
use rayon::prelude::*;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct CliArgs {
    /// Pictures library directory
    backup_root: String,
    /// Picture to backup
    file_path: Option<String>,
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
            .into_iter()
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
    let mut success: Vec<BackupSuccess> = Vec::new();
    let mut failures: Vec<BackupFailure> = Vec::new();
    results.into_iter().for_each(|e| match e {
        Ok(s) => success.push(s),
        Err(f) => failures.push(f)
    });
    eprintln!("Backup Statistics:");
    eprintln!("");
    eprintln!("Success: {}", success.len());
    eprintln!("Failures: {}", failures.len());
    if failures.len() != 0 {
        eprintln!("");
        eprintln!("WARNING: unable to backup some files:");
        failures.iter().for_each(|f| eprintln!("{}", f));
    }
}

/// Backup a file.
fn backup_file(cli: &CliArgs, file_path: &str) -> Result<BackupSuccess, BackupFailure> {
    let filename = Path::new(file_path);
    let file = File::open(filename).map_err(
        |e| BackupFailure::CopyError(format!("cannot open the {} file: {}", file_path, e.to_string()))
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
            Err(_) => {
                eprintln!("ERROR: cannot copy {} to {}", &filename.display(), &target_filename.display());
                Err(BackupFailure::CopyError(String::from(file_path)))
            }
        }
    } else if same_files(filename, &target_filename) {
        Ok(BackupSuccess::AlreadyBackup(String::from(file_path)))
    } else {
        Err(BackupFailure::AlreadyBackupButDifferent(String::from(file_path)))
    }
}

fn upsert_picture_directory(picture_dir: &PathBuf) {
    // Prevent concurrent directory creation by locking a mutex.
    let _ = CREATE_DIR_MUTEX.lock();
    if !picture_dir.exists() {
            create_dir_all(&picture_dir)
            .unwrap_or_else(
                |e| panic!("ERROR: cannot create the backup directory {}: {}", &picture_dir.display(), e.to_string())
            );
    } else if !picture_dir.is_dir() {
        panic!("ERROR: {} already exists and is not a directory. Can't use it to store a picture.", &picture_dir.display())
    }
}

/// Retrieves when the picture has been shot from the EXIF metadata.
/// If no datetime EXIF data is attached to the file, use the file
/// last modification date.
fn get_picture_datetime(file_path: &str, file: &File) -> (DateTime<Utc>, PictureDatetimeOrigin) {
    let exif_datetime = get_picture_exif_datetime(file);
    match exif_datetime {
        Some(dt) => (dt, PictureDatetimeOrigin::Exif),
        None => (get_file_modified_time(file_path, file), PictureDatetimeOrigin::FilesystemMetadata)
    }
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
