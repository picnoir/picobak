use std::fs::{create_dir_all, copy};
use std::{fs::File, path::PathBuf};
use std::path::Path;

use clap::Parser;
use exif::{Tag, In, Value};
use chrono::{Utc, DateTime, Datelike, NaiveDateTime};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct CliArgs {
    /// Pictures library directory
    backup_root: String,
    /// Picture to backup
    file_path: String,
    /// Do not create any directory or copy any file. Only prints out the operations it would perform
    #[arg(short, long)]
    dry_run: bool
}

fn main() {
    let cli = CliArgs::parse();

    if !cli.dry_run {
        validate_args(&cli);
    }
    let stdin = std::io::stdin();
    for line in stdin.lines() {
        backup_one_file(&cli, &line.unwrap());
    }
}

/// Backup a file.
fn backup_one_file(cli: &CliArgs, file_path: &str) {
    let filename = Path::new(file_path);
    let file = File::open(filename).unwrap_or_else(
        |_| panic!("ERROR: cannot open the {} file", file_path)
    );
    let datetime = get_picture_datetime(file_path, &file);
    let picture_dir = find_backup_dir(&cli.backup_root, &datetime);

    if !picture_dir.exists() {
        if !cli.dry_run {
            create_dir_all(&picture_dir)
                .unwrap_or_else(
                    |_| panic!("ERROR: cannot create directory at {}", &picture_dir.display())
                );
        } else {
            eprintln!("Would mkdir {}", &picture_dir.display());
        }
    } else if !picture_dir.is_dir() {
        panic!("ERROR: {} already exists and is not a directory", &picture_dir.display())
    }

    let filename_name = filename.file_name()
        .unwrap_or_else(|| panic!("Error: Incorrect file name {}", filename.display()));
    let target_filename = picture_dir.join(filename_name);
    if !target_filename.is_file() {
        if !cli.dry_run {
            copy(filename, &target_filename)
                .unwrap_or_else(|_| panic!("ERROR: cannot copy {} to {}", &filename.display(), &target_filename.display()));
        } else {
            eprintln!("Would copy {} to {}", filename.display(), target_filename.display());
        }
    } else if same_files(filename, &target_filename) {
        eprintln!("File already archived: {}", &filename.display())
    } else {
        panic!("ERROR: {} already exists in {}, but the two files are different",
               &filename.display(),
               &target_filename.display())
    }
}

/// Retrieves when the picture has been shot from the EXIF metadata.
/// If no datetime EXIF data is attached to the file, use the file
/// last modification date.
fn get_picture_datetime(file_path: &str, file: &File) -> DateTime<Utc> {
    let exif_datetime = get_picture_exif_datetime(file);
    exif_datetime.unwrap_or_else(|| get_file_modified_time(file_path, file))
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
    eprintln!("No EXIF information available for {}, falling back to file mtime.", file_path);
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
    if !Path::new(&args.file_path).is_file() {
        panic!("ERROR: {} is not a file", &args.file_path);
    };
    if Path::new(&args.backup_root).is_file() {
        panic!("ERROR: {} is a file, not a valid backup dir", &args.file_path);
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
    let source_modified = source_file.modified().unwrap_or_else(|_| panic!("ERROR: cannot find created datetime for {}", &source.display()));
    let target_modified = target_file.modified().unwrap_or_else(|_| panic!("ERROR: cannot find created datetime for {}", &target.display()));

    source_file.len() == target_file.len() && source_modified == target_modified
}
