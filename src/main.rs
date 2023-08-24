use std::fs::{create_dir_all, copy};
use std::{fs::File, path::PathBuf};
use std::path::Path;

use clap::Parser;
use exif::{Tag, In, Value, DateTime};

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

    let filename = Path::new(&cli.file_path);
    let file = File::open(&filename).expect(&format!("ERROR: cannot open the {} file", &cli.file_path));
    let datetime = get_picture_datetime(&cli.file_path, &file);
    let picture_dir = find_backup_dir(&cli.backup_root, &datetime);

    if !picture_dir.exists() {
        if !cli.dry_run {
            create_dir_all(&picture_dir)
                .expect(&format!("ERROR: cannot create directory at {}", &picture_dir.display()));
        } else {
            eprint!("Would mkdir {}", &picture_dir.display());
        }
    } else if !picture_dir.is_dir() {
        panic!("ERROR: {} already exists and is not a directory", &picture_dir.display())
    }

    let target_filename = picture_dir
        .join(filename.file_name()
              .expect(&format!("Error: Incorrect file name {}", filename.display())));
    if !target_filename.is_file() {
        if !cli.dry_run {
            copy(&filename, &target_filename)
                .expect(&format!("ERROR: cannot copy {} to {}", &filename.display(), &target_filename.display()));
        } else {
            eprint!("Would copy {} to {}", filename.display(), target_filename.display());
        }
    } else {
        if same_files(&filename, &target_filename) {
            eprint!("File already archived: {}", &filename.display())
        } else {
            panic!("ERROR: {} already exists in {}, but the two files are different",
                   &filename.display(),
                   &target_filename.display())
        }
    }
}

/// Retrieves when the picture has been shot from the EXIF metadata.
fn get_picture_datetime(file_path: &str, file: &File) -> DateTime {
    let mut bufreader = std::io::BufReader::new(file);
    let exifreader = exif::Reader::new();
    let exif = exifreader.read_from_container(&mut bufreader)
        .expect(&format!("ERROR: cannot read EXIF metadata for picture {}", file_path));
    let datetime_field = exif.get_field(Tag::DateTimeOriginal, In::PRIMARY)
        .expect(&format!("ERROR: missing datetime tag for picture {}", file_path));
    match datetime_field.value {
        Value::Ascii(ref vec) if !vec.is_empty() =>
            if let Ok(datetime) = DateTime::from_ascii(&vec[0]) {
                datetime
            } else {
                panic!("ERROR: incorrect datetime format for file {}", file_path)
            }
        _ =>
            panic!("ERROR: cannot parse ASCII from datetime EXIF field for file {}", file_path)
    }
}

/// Directory in which we want to save the picture.
fn find_backup_dir(backup_root: &str, datetime: &DateTime) -> PathBuf {
    let backup_root = Path::new(backup_root);
    backup_root
        .join(datetime.year.to_string())
        .join(datetime.month.to_string())
        .join(datetime.day.to_string())
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
    let source_file = File::open(&source)
        .expect(&format!("Error: cannot open file {}", &source.display()))
        .metadata()
        .expect(&format!("Error: cannot get metadata of  file {}", &source.display()));
    let target_file = File::open(&target)
        .expect(&format!("Error: cannot open file {}", &target.display()))
        .metadata()
        .expect(&format!("Error: cannot get metadata of  file {}", &target.display()));
    let source_created = source_file.created().expect(&format!("ERROR: cannot find created datetime for {}", &source.display()));
    let target_created = target_file.created().expect(&format!("ERROR: cannot find created datetime for {}", &target.display()));

    source_file.len() == target_file.len() && source_created == target_created
}
