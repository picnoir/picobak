# Picobak

Picobak is a small CLI utility to help you backup and organize your pictures on a filesystem. It uses the pictures [EXIF](https://en.wikipedia.org/wiki/Exif) metadata to store the files in a `year/month/day` directory tree like this:

```txt
2023
|
|
|-- 02
    |-- 19
        |-- pic2.jpg
    |-- 20
        |-- pic1.jpg
        |-- pic2.jpg
        |-- pic3.jpg
(...)
```

This program is heavily inspired by Shotwell's backup feature. I actually used that for years to organize my pictures. Sadly, it became more and more unstable along the years, it often fails midway-through the backup. Its implementation is too intimidating for me to try to fix and maintain it. In contrast, this utility is meant to stay small in terms of features scope and codebase. Nevertheless, Shotwell is a great program overall, kudos to the original authors, they have made my life simpler for years <3.

## Usage

Overall:

```txt
picobak BACKUP_ROOT PICTURE_TO_BACKUP
```

You can couple this tool with [GNU parallel](https://www.gnu.org/software/parallel/) to concurently backup multiple images and fully utilize a multicore system:

```txt
ls dir-containing-pictures | parallel -j $(nproc) picobak /my/pic-backup-root/
```
