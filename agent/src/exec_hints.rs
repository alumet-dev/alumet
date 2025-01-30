use std::{
    fs::{self, File, Metadata},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub fn handle_permission_denied(external_command: String) -> String {
    let file_open_result = File::open(external_command.clone());
    let file_correctly_opened = if let Err(_err) = file_open_result {
        // Can't open the file, let's check it's parent
        let current_parent = match find_a_parent_with_perm_issue(external_command.clone()) {
            Ok(parent) => parent,
            Err(err) => return err,
        };
        let metadata: Metadata = current_parent
            .metadata()
            .unwrap_or_else(|_| panic!("Unable to retrieve metadata of file: {}", current_parent.display()));
        let missing_permissions = check_missing_permissions(metadata.permissions().mode(), 0o555);
        if missing_permissions & 0o500 != 0 || missing_permissions & 0o050 != 0 || missing_permissions & 0o005 != 0 {
            log::warn!(
                "folder '{}' is missing the following permissions:  'rx'",
                current_parent.display()
            );
            log::info!("💡 Hint: try 'chmod +rx {}'", current_parent.display());
        }
        return format!("Error when trying to read the file: {}", external_command.clone());
    } else if let Ok(file) = file_open_result {
        // Can open the file
        file
    } else {
        return "Error when trying to read the file".to_owned();
    };

    // Get file metadata to see missing permissions
    let file_metadata = file_correctly_opened
        .metadata()
        .unwrap_or_else(|_| panic!("Unable to retrieve metadata for: {}", external_command));
    let missing_permissions = check_missing_permissions(file_metadata.permissions().mode(), 0o505);
    let missing_right_str;
    if missing_permissions & 0o500 != 0 || missing_permissions & 0o050 != 0 || missing_permissions & 0o005 != 0 {
        missing_right_str = "rx"
    } else if missing_permissions & 0o400 != 0 || missing_permissions & 0o040 != 0 || missing_permissions & 0o004 != 0 {
        missing_right_str = "r"
    } else if missing_permissions & 0o100 != 0 || missing_permissions & 0o010 != 0 || missing_permissions & 0o001 != 0 {
        missing_right_str = "x"
    } else {
        missing_right_str = "rx"
    }
    log::error!(
        "file '{}' is missing the following permissions:  '{}'",
        external_command,
        missing_right_str
    );
    log::info!("💡 Hint: try 'chmod +{} {}'", missing_right_str, external_command);
    format!("Error happened about file's permission {}", external_command)
}

pub fn handle_not_found(external_command: String, args: Vec<String>) -> String {
    fn resolve_application_path() -> std::io::Result<PathBuf> {
        std::env::current_exe()?.canonicalize()
    }
    log::error!("Command '{}' not found", external_command);
    let directory_entries_iter = match fs::read_dir(".") {
        Ok(directory) => directory,
        Err(err) => {
            log::error!("Error when trying to read current directory: {}", err);
            return format!("Error when trying to read current directory: {}", err);
        }
    };
    let app_path = resolve_application_path()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_owned()))
        .unwrap_or(String::from("path/to/agent"));

    let mut lowest_distance = usize::MAX;
    let mut best_element = None;

    for entry_result in directory_entries_iter {
        let entry = entry_result.unwrap();
        let entry_type = entry.file_type().unwrap();
        if entry_type.is_file() {
            let entry_string = entry.file_name().into_string().unwrap();
            let path = Path::new(&external_command);
            let external_command_corrected_name: String = if path.is_absolute() {
                path.file_name()
                    .and_then(|os_str| os_str.to_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| external_command.to_string())
            } else {
                external_command
                    .strip_prefix("./")
                    .unwrap_or(&external_command)
                    .to_string()
            };

            let distance = super::word_distance::distance_with_adjacent_transposition(
                &external_command_corrected_name.clone(),
                &entry_string.clone(),
            );
            if distance < 3 && distance < lowest_distance {
                lowest_distance = distance;
                best_element = Some((entry_string, distance));
            }
        }
    }
    match best_element {
        Some((element, distance)) => {
            let argument_list = args
                .iter()
                .map(|arg| {
                    if arg.contains(' ') {
                        format!("\"{}\"", arg)
                    } else {
                        arg.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            if distance == 0 {
                log::info!(
                    "💡 Hint: A file named '{}' exists in the current directory. Prepend ./ to execute it.",
                    element
                );
                log::info!("Example: {} exec ./{} {}", app_path, element, argument_list);
            } else {
                log::warn!("💡 Hint: Did you mean ./{} {}", element, argument_list);
            }
            return "File not found but another appears to match".to_string();
        }
        None => {
            log::warn!(
                "💡 Hint: No matching file exists in the current directory. Please try again we a different one."
            );
        }
    }
    "Sorry but the file was not found".to_string()
}

fn check_missing_permissions(current_permissions: u32, required_permissions: u32) -> u32 {
    required_permissions & !current_permissions
}

fn find_a_parent_with_perm_issue(path: String) -> anyhow::Result<std::path::PathBuf, String> {
    // Current parent can change if a parent of the parent don't have the correct rights
    let mut current_parent = match std::path::Path::new(&path).parent() {
        Some(parent) => parent,
        None => return Err("".to_string()),
    };
    // Through this loop I will iterate over parent of parent until I can retrieve metadata, it will show the first folder
    // that I can't execute and suggest to the user to grant execution rights.
    let mut counter_stop = 0;
    loop {
        if counter_stop >= 100 {
            break;
        }
        counter_stop += 1;
        match current_parent.metadata() {
            Ok(_) => return Ok(current_parent.to_path_buf()),
            Err(_) => {
                current_parent = match current_parent.parent() {
                    Some(parent) => parent,
                    None => return Err("Unable to retrieve a parent for your file".to_string()),
                }
            }
        }
    }
    Err("Unable to retrieve a parent for your file".to_string())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use fs::Permissions;
    use tempfile::tempdir;

    use super::*;

    fn reset_permissions(folder_a: PathBuf, folder_b: PathBuf, file: &File) {
        fs::set_permissions(&folder_a, Permissions::from_mode(0o777)).expect("Can't change folder's permissions");
        fs::set_permissions(&folder_b, Permissions::from_mode(0o777)).expect("Can't change folder's permissions");
        file.set_permissions(Permissions::from_mode(0o777))
            .expect("Can't change file's permissions");
    }

    #[test]
    fn test_check_missing_permissions() {
        // Check user perms
        assert_eq!(0o000, check_missing_permissions(0o700, 0o700));
        assert_eq!(0o000, check_missing_permissions(0o400, 0o400));
        assert_eq!(0o000, check_missing_permissions(0o200, 0o200));
        assert_eq!(0o000, check_missing_permissions(0o100, 0o100));
        assert_eq!(0o100, check_missing_permissions(0o600, 0o700));
        assert_eq!(0o200, check_missing_permissions(0o500, 0o700));
        assert_eq!(0o400, check_missing_permissions(0o300, 0o700));
        assert_eq!(0o500, check_missing_permissions(0o200, 0o700));
        assert_eq!(0o600, check_missing_permissions(0o100, 0o700));
        assert_eq!(0o700, check_missing_permissions(0o000, 0o700));
        assert_eq!(0o100, check_missing_permissions(0o200, 0o300));
        assert_eq!(0o100, check_missing_permissions(0o000, 0o100));
        assert_eq!(0o200, check_missing_permissions(0o000, 0o200));
        assert_eq!(0o400, check_missing_permissions(0o000, 0o400));

        // Check group perms
        assert_eq!(0o000, check_missing_permissions(0o070, 0o070));
        assert_eq!(0o000, check_missing_permissions(0o070, 0o070));
        assert_eq!(0o000, check_missing_permissions(0o040, 0o040));
        assert_eq!(0o000, check_missing_permissions(0o020, 0o020));
        assert_eq!(0o000, check_missing_permissions(0o010, 0o010));
        assert_eq!(0o010, check_missing_permissions(0o060, 0o070));
        assert_eq!(0o020, check_missing_permissions(0o050, 0o070));
        assert_eq!(0o040, check_missing_permissions(0o030, 0o070));
        assert_eq!(0o050, check_missing_permissions(0o020, 0o070));
        assert_eq!(0o060, check_missing_permissions(0o010, 0o070));
        assert_eq!(0o070, check_missing_permissions(0o000, 0o070));
        assert_eq!(0o010, check_missing_permissions(0o020, 0o030));
        assert_eq!(0o010, check_missing_permissions(0o000, 0o010));
        assert_eq!(0o020, check_missing_permissions(0o000, 0o020));
        assert_eq!(0o040, check_missing_permissions(0o000, 0o040));

        // Check other perms
        assert_eq!(0o000, check_missing_permissions(0o007, 0o007));
        assert_eq!(0o000, check_missing_permissions(0o004, 0o004));
        assert_eq!(0o000, check_missing_permissions(0o002, 0o002));
        assert_eq!(0o000, check_missing_permissions(0o001, 0o001));
        assert_eq!(0o001, check_missing_permissions(0o006, 0o007));
        assert_eq!(0o002, check_missing_permissions(0o005, 0o007));
        assert_eq!(0o004, check_missing_permissions(0o003, 0o007));
        assert_eq!(0o005, check_missing_permissions(0o002, 0o007));
        assert_eq!(0o006, check_missing_permissions(0o001, 0o007));
        assert_eq!(0o007, check_missing_permissions(0o000, 0o007));
        assert_eq!(0o001, check_missing_permissions(0o002, 0o003));
        assert_eq!(0o001, check_missing_permissions(0o000, 0o001));
        assert_eq!(0o002, check_missing_permissions(0o000, 0o002));
        assert_eq!(0o004, check_missing_permissions(0o000, 0o004));
    }

    #[test]
    fn test_handle_permission_denied() {
        let tmp = tempdir().expect("Failed to create a temporary directory");
        let root: std::path::PathBuf = tmp.path().join("river_folder/");
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let river_song_folder = root.join("song_folder/");
        std::fs::create_dir_all(&river_song_folder).unwrap();

        let path_file = river_song_folder.join("script.sh");
        let path_file_string = path_file.clone().into_os_string().into_string().unwrap();
        std::fs::write(
            path_file.clone(),
            format!(
                "#!/bin/sh\n
                echo \"Hello\"\n
                sleep 2"
            ),
        )
        .unwrap();

        let file = match File::open(&path_file) {
            Err(why) => panic!("couldn't open {}: {}", path_file.display(), why),
            Ok(file) => file,
        };

        let message_expect_folder = "Can't change folder's permissions";
        let message_expect_file = "Can't change file's permissions";

        file.set_permissions(Permissions::from_mode(0o555))
            .expect(message_expect_file);
        fs::set_permissions(&river_song_folder, Permissions::from_mode(0o555)).expect(message_expect_folder);
        fs::set_permissions(&root, Permissions::from_mode(0o555)).expect(message_expect_folder);
        assert_eq!(
            format!("Error happened about file's permission {}", path_file_string.clone()),
            handle_permission_denied(path_file_string.clone())
        );
        reset_permissions(root.clone(), river_song_folder.clone(), &file);

        file.set_permissions(Permissions::from_mode(0o444))
            .expect(message_expect_file);
        fs::set_permissions(&river_song_folder, Permissions::from_mode(0o555)).expect(message_expect_folder);
        fs::set_permissions(&root, Permissions::from_mode(0o555)).expect(message_expect_folder);
        assert_eq!(
            format!("Error happened about file's permission {}", path_file_string.clone()),
            handle_permission_denied(path_file_string.clone())
        );
        reset_permissions(root.clone(), river_song_folder.clone(), &file);

        file.set_permissions(Permissions::from_mode(0o555))
            .expect(message_expect_file);
        fs::set_permissions(&river_song_folder, Permissions::from_mode(0o555)).expect(message_expect_folder);
        fs::set_permissions(&root, Permissions::from_mode(0o444)).expect(message_expect_folder);
        assert_eq!(
            format!("Error when trying to read the file: {}", path_file_string.clone()),
            handle_permission_denied(path_file_string.clone())
        );
        reset_permissions(root.clone(), river_song_folder.clone(), &file);

        file.set_permissions(Permissions::from_mode(0o555))
            .expect(message_expect_file);
        fs::set_permissions(&river_song_folder, Permissions::from_mode(0o555)).expect(message_expect_folder);
        fs::set_permissions(&root, Permissions::from_mode(0o111)).expect(message_expect_folder);
        assert_eq!(
            format!("Error happened about file's permission {}", path_file_string.clone()),
            handle_permission_denied(path_file_string.clone())
        );
        reset_permissions(root.clone(), river_song_folder.clone(), &file);

        file.set_permissions(Permissions::from_mode(0o555))
            .expect(message_expect_file);
        fs::set_permissions(&river_song_folder, Permissions::from_mode(0o111)).expect(message_expect_folder);
        fs::set_permissions(&root, Permissions::from_mode(0o111)).expect(message_expect_folder);
        assert_eq!(
            format!("Error happened about file's permission {}", path_file_string.clone()),
            handle_permission_denied(path_file_string.clone())
        );
        reset_permissions(root.clone(), river_song_folder.clone(), &file);

        file.set_permissions(Permissions::from_mode(0o555))
            .expect(message_expect_file);
        fs::set_permissions(&river_song_folder, Permissions::from_mode(0o111)).expect(message_expect_folder);
        fs::set_permissions(&root, Permissions::from_mode(0o555)).expect(message_expect_folder);
        assert_eq!(
            format!("Error happened about file's permission {}", path_file_string.clone()),
            handle_permission_denied(path_file_string.clone())
        );
        reset_permissions(root.clone(), river_song_folder.clone(), &file);

        file.set_permissions(Permissions::from_mode(0o555))
            .expect(message_expect_file);
        fs::set_permissions(&river_song_folder, Permissions::from_mode(0o000)).expect(message_expect_folder);
        fs::set_permissions(&root, Permissions::from_mode(0o555)).expect(message_expect_folder);
        assert_eq!(
            format!("Error when trying to read the file: {}", path_file_string.clone()),
            handle_permission_denied(path_file_string.clone())
        );
        reset_permissions(root.clone(), river_song_folder.clone(), &file);

        file.set_permissions(Permissions::from_mode(0o555))
            .expect(message_expect_file);
        fs::set_permissions(&river_song_folder, Permissions::from_mode(0o555)).expect(message_expect_folder);
        fs::set_permissions(&root, Permissions::from_mode(0o000)).expect(message_expect_folder);
        assert_eq!(
            format!("Error when trying to read the file: {}", path_file_string.clone()),
            handle_permission_denied(path_file_string.clone())
        );
        reset_permissions(root.clone(), river_song_folder.clone(), &file);
    }

    #[test]
    fn test_handle_not_found() {
        let tmp = tempdir().expect("Failed to create a temporary directory");
        let root: std::path::PathBuf = tmp.path().join("river_folder/");
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, Permissions::from_mode(0o777)).expect("Can't change folder's permissions");
        std::env::set_current_dir(&root).expect("Error when trying to modify working directory");

        let path_file = root.join("script.sh");
        let path_non_existing_file = root.join("scripts.sh");
        let path_file_string = path_file.clone().into_os_string().into_string().unwrap();
        let path_non_existing_file_string = path_non_existing_file.clone().into_os_string().into_string().unwrap();
        std::fs::write(
            path_file.clone(),
            format!(
                "#!/bin/sh\n
                echo \"Hello\"\n
                sleep 2"
            ),
        )
        .unwrap();
        let file = match File::open(&path_file) {
            Err(why) => panic!("couldn't open {}: {}", path_file.display(), why),
            Ok(file) => file,
        };
        file.set_permissions(Permissions::from_mode(0o777))
            .expect("Can't modify file's permissions");
        let args: Vec<String> = vec![];

        assert_eq!(
            "File not found but another appears to match",
            handle_not_found(path_file_string.clone(), args.clone())
        );
        assert_eq!(
            "File not found but another appears to match",
            handle_not_found(path_non_existing_file_string.clone(), args.clone())
        );
        assert_eq!(
            "File not found but another appears to match",
            handle_not_found("scripts.sh".to_owned(), args.clone())
        );
        assert_eq!(
            "File not found but another appears to match",
            handle_not_found("./script.sh".to_owned(), args.clone())
        );
        assert_eq!(
            "Sorry but the file was not found",
            handle_not_found("./scriptAAAAAAAAAA.sh".to_owned(), args.clone())
        );

        let path_file_distance2 = root.join("scriptAA.sh");
        std::fs::write(
            path_file_distance2.clone(),
            format!(
                "#!/bin/sh\n
            echo \"Bye\"\n
            sleep 2"
            ),
        )
        .unwrap();
        assert_eq!(
            "File not found but another appears to match",
            handle_not_found("./scripts.sh".to_owned(), args.clone())
        );
        assert_eq!(
            "File not found but another appears to match",
            handle_not_found("./script.sh".to_owned(), args.clone())
        );
    }
}
