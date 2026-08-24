use std::{
    fs::{self, File, remove_file},
    io::BufReader,
    path::PathBuf,
};

macro_rules! get_env_flag {
    ($var:ident, $env_name:literal) => {
        println!("cargo::rerun-if-env-changed={}", $env_name);
        let $var = match std::env::var($env_name) {
            Ok(val) => Some(val),
            Err(_) => None,
        };
    };
    ($var:ident, $env_name:literal, $default:expr) => {
        println!("cargo::rerun-if-env-changed={}", $env_name);
        let $var = match std::env::var($env_name) {
            Ok(val) => val,
            Err(_) => $default,
        };
    };
}

fn main() -> Result<(), std::io::Error> {
    get_env_flag!(inline_fortune_flag, "CARGO_FEATURE_INLINE_FORTUNE");
    get_env_flag!(inline_cowsay_flag, "CARGO_FEATURE_INLINE_COWSAY");

    if inline_cowsay_flag.is_some() {
        cowsay::generate_cowsay_source()?;
    }

    if inline_fortune_flag.is_some() {
        fortune::create_fortune_db()?;
    }

    Ok(())
}

macro_rules! illegal_file_suffixes {
    ($($ext:literal),*) => {
        [
            $(std::ffi::OsStr::new($ext)),*
        ]
    };
}

///Checks if the directory exists. If it doesn't, it creates it
macro_rules! check_dir_exists {
    ($path:expr) => {
        if let Err(_) = std::fs::read_dir($path) {
            std::fs::create_dir($path)?
        }
    };
    ($path:expr, clear_existing) => {
        match fs::read_dir($path) {
            Ok(_) => {
                fs::remove_dir_all($path)?;
                fs::create_dir($path)?;
            }
            Err(_) => fs::create_dir($path)?,
        };
    };
}

mod fortune {
    use std::{
        ffi::OsStr,
        fs::{self, File},
        io::{self, Read, Write},
        path::PathBuf,
    };

    pub fn create_fortune_db() -> Result<(), std::io::Error> {
        get_env_flag!(inline_off_fortune_flag, "CARGO_FEATURE_INLINE_FORTUNE");
        get_env_flag!(fortune_path, "FORTUNE_PATH");
        get_env_flag!(
            fortune_resource_zip_url,
            "FORTUNE_RESOURCE_ZIP_URL",
            String::from("https://github.com/shlomif/fortune-mod/archive/refs/heads/master.zip")
        );
        get_env_flag!(
            fortune_resource_path,
            "FORTUNE_RESOURCE_PATH",
            String::from("fortune-mod-master/fortune-mod/datfiles")
        );
        get_env_flag!(excluded_fortunes, "EXCLUDED_FORTUNES", String::from(""));

        get_env_flag!(max_fortune_line_len, "MAX_FORTUNE_LINE_LENGTH");
        let max_width = max_fortune_line_len.map(|x| {
            u64::from_str_radix(&x, 10)
                .expect("Need a non-decimal Base 10 number for maximum fortune line length")
        });
        get_env_flag!(max_fortune_lines, "MAX_FORTUNE_LINES");
        let max_lines = max_fortune_lines.map(|x| {
            u64::from_str_radix(&x, 10)
                .expect("Need a non-decimal Base 10 number for maximum fortune line count")
        });
        get_env_flag!(use_default_res, "USE_DEFAULT_RESOURCES");

        check_dir_exists!("target/resources");
        check_dir_exists!("target/generated_sources");

        let fortune_location = match fortune_path {
            Some(_) if use_default_res.is_some() => {
                crate::get_source_archive(
                    &fortune_resource_zip_url,
                    "fortune",
                    &fortune_resource_path,
                    &Some(excluded_fortunes.split(";").collect::<Vec<&str>>()),
                )?;
                String::from("target/resources/fortune")
            }
            Some(val) => val,
            None => {
                crate::get_source_archive(
                    &fortune_resource_zip_url,
                    "fortune",
                    &fortune_resource_path,
                    &Some(excluded_fortunes.split(";").collect::<Vec<&str>>()),
                )?;
                String::from("target/resources/fortune")
            }
        };

        crate::fortune::gen_fortune_db(
            fortune_location,
            &max_width,
            &max_lines,
            &inline_off_fortune_flag,
        )?;

        Ok(())
    }

    fn gen_fortune_db(
        path: String,
        max_width: &Option<u64>,
        max_lines: &Option<u64>,
        include_offensive: &Option<String>,
    ) -> Result<(), io::Error> {
        println!("cargo::rerun-if-changed={path}");

        let mut concat_fortunes: String;
        let mut off_concat_fortunes: String;
        match fs::metadata(&path)?.is_file() {
            true => {
                //Assume file contains only non-offensive fortunes
                match File::open(&path) {
                    Ok(mut file) => {
                        concat_fortunes = String::new();
                        let _ = file.read_to_string(&mut concat_fortunes)?;
                        off_concat_fortunes = String::new();
                    }
                    Err(_) => panic!("Could not read specified file defined by FORTUNE_FILE"),
                }
            }
            false => {
                (concat_fortunes, off_concat_fortunes) =
                    get_fortune_strings(&PathBuf::from(path), false);
            }
        }
        concat_fortunes.retain(|c| c != '\r');
        off_concat_fortunes.retain(|c| c != '\r');

        //TODO probably need to pass settings as param no closure capture here
        let fortunes_split: Vec<&str> = concat_fortunes
            .split("\n%\n")
            .filter(|element| check_fortune_constraints(element, max_width, max_lines))
            .collect();
        let num_fortunes = fortunes_split.len();
        let off_fortunes_split: Vec<&str> = off_concat_fortunes
            .split("\n%\n")
            .filter(|element| check_fortune_constraints(element, max_width, max_lines))
            .collect();
        let num_off_fortunes = off_fortunes_split.len();

        let fortune_arr = quote::quote! {
            const FORTUNE_LIST: [&'static str; #num_fortunes] = [
                #(#fortunes_split) ,*
            ];
        };

        let off_fortune_arr = quote::quote! {
            const OFF_FORTUNE_LIST: [&'static str; #num_off_fortunes] = [
                #(#off_fortunes_split) ,*
            ];
        };

        match File::create("target/generated_sources/fortune_db.rs") {
            Ok(mut file) => {
                let _ = file.write_all(fortune_arr.to_string().as_bytes())?;
                if include_offensive.is_some() {
                    let _ = file.write_all(off_fortune_arr.to_string().as_bytes())?;
                }
            }
            Err(err) => panic!("Could not concatenate fortunes into single file: {err}"),
        }

        Ok(())
    }

    fn get_fortune_strings(path: &PathBuf, is_offensive: bool) -> (String, String) {
        let illegal_file_suffixes: [&OsStr; 16] = illegal_file_suffixes!(
            "dat", "pos", "c", "h", "p", "i", "f", "pas", "ftn", "ins.c", "ins.pas", "ins.ftn",
            "sml", "sh", "pl", "csv"
        );
        let mut fortune_buf = String::new();
        let mut off_fortune_buf = String::new();

        let dir_list = fs::read_dir(path).expect("Could not open directory");
        for entry in dir_list.filter(|item| {
            !illegal_file_suffixes.contains(
                  &item
                      .as_ref()
                      .unwrap()
                      .path()
                      .extension()
                      .unwrap_or_default(),
              ) &&
          	//Additional condition to ignore the CMakeLists.txt file specifically in fortune-mod
          	!item.as_ref().unwrap().path().ends_with("CMakeLists.txt")
        }) {
            match entry {
                Ok(item) => match item.metadata().unwrap().is_dir() {
                    true => {
                        if item.file_name() == "off" {
                            off_fortune_buf = off_fortune_buf
                                + get_fortune_strings(&item.path(), true).1.as_str();
                        } else {
                            let (fortunes, off_fortunes) =
                                get_fortune_strings(&item.path(), is_offensive);
                            fortune_buf = fortune_buf + fortunes.as_str();
                            off_fortune_buf = off_fortune_buf + off_fortunes.as_str();
                        }
                    }
                    false => match File::open(item.path()) {
                        Ok(mut file) => {
                            let mut buf = String::new();
                            let _ = file.read_to_string(&mut buf);

                            match is_offensive {
                                true => off_fortune_buf = off_fortune_buf + buf.as_str(),
                                false => fortune_buf = fortune_buf + buf.as_str(),
                            };
                        }
                        Err(_) => panic!(
                            "Could not open a fortune file for copying into internal buffers"
                        ),
                    },
                },
                Err(e) => panic!("Could not identify a file in the fortune directory {e}"),
            }
        }
        (fortune_buf, off_fortune_buf)
    }

    ///Function used for the filter iterators
    fn check_fortune_constraints(
        element: &&str,
        max_width: &Option<u64>,
        max_lines: &Option<u64>,
    ) -> bool {
        let m_w = match max_width {
            Some(val) => {
                element
                    .split("\n")
                    .reduce(|acc, e| if e.len() > acc.len() { e } else { acc })
                    .expect("Could not split the chosen string for constraint validation")
                    .len()
                    <= *val as usize
            }
            None => true,
        };

        let m_l = match max_lines {
            Some(val) => {
                element.chars().fold(0, |acc, e| match e == '\n' {
                    true => acc + 1,
                    false => acc,
                }) <= *val
            }
            None => true,
        };

        m_w && m_l
    }
}

mod cowsay {
    use std::{
        collections::HashMap,
        fs::{self, File},
        io::{self, Read, Write},
        path::PathBuf,
    };

    pub fn generate_cowsay_source() -> Result<(), std::io::Error> {
        check_dir_exists!("target/generated_sources");
        get_env_flag!(cow_path, "COW_PATH");
        get_env_flag!(
            cowsay_resource_zip_url,
            "COWSAY_RESOURCE_ZIP_URL",
            String::from("https://github.com/cowsay-org/cowsay/archive/refs/heads/main.zip")
        );
        get_env_flag!(
            cowsay_resource_path,
            "COWSAY_RESOURCE_PATH",
            String::from("cowsay-main/share/cowsay/cows")
        );
        get_env_flag!(
            excluded_cow_files,
            "EXCLUDED_COWS",
            String::from("three-eyes.cow;udder.cow")
        );

        get_env_flag!(use_default_res, "USE_DEFAULT_RESOURCES");

        let cowpath: PathBuf = match cow_path {
            Some(_) if use_default_res.is_some() => {
                crate::get_source_archive(
                    &cowsay_resource_zip_url,
                    "cowsay",
                    &cowsay_resource_path,
                    &Some(excluded_cow_files.split(";").collect::<Vec<&str>>()),
                )?;
                PathBuf::from("target/resources/cowsay")
            }
            Some(val) => PathBuf::from(val),
            None => {
                crate::get_source_archive(
                    &cowsay_resource_zip_url,
                    "cowsay",
                    &cowsay_resource_path,
                    &Some(excluded_cow_files.split(";").collect::<Vec<&str>>()),
                )?;
                PathBuf::from("target/resources/cowsay")
            }
        };
        println!("cargo::rerun-if-changed={cowpath:?}");

        let cow_data = get_cow_data(&cowpath)?;
        let tokenstream = make_source(cow_data)?;

        match File::create("target/generated_sources/cow_literals.rs") {
            Ok(mut file) => {
                let _ = file.write_all(tokenstream.to_string().as_bytes())?;
            }
            Err(err) => panic!("Could not concatenate fortunes into single file: {err}"),
        }

        Ok(())
    }

    fn get_cow_data(path: &PathBuf) -> Result<HashMap<String, String>, io::Error> {
        let mut total_list: HashMap<String, String> = HashMap::new();
        let dir_list = fs::read_dir(path)?;
        for entry in dir_list {
            match entry {
                Ok(item) => match item.metadata()?.is_dir() {
                    true => {
                        let _ = get_cow_data(&item.path())?.iter_mut().map(|(k, v)| {
                            total_list.insert(k.clone(), v.clone());
                        });
                    }
                    false => {
                        if item.path().extension().unwrap() == "cow" {
                            let key = item
                                .file_name()
                                .to_str()
                                .unwrap()
                                .to_string()
                                .replace(".cow", "");
                            match File::open(item.path()) {
                                Ok(mut file) => {
                                    let mut value = String::new();
                                    let _ = file.read_to_string(&mut value)?;
                                    total_list.insert(key, value);
                                }
                                Err(e) => {
                                    panic!(
                                        "Could not open a cow for inlining: {} {e}",
                                        item.path().display()
                                    )
                                }
                            }
                        }
                    }
                },
                Err(e) => return Err(e),
            }
        }

        Ok(total_list)
    }

    fn make_source(
        cow_data: HashMap<String, String>,
    ) -> Result<proc_macro2::TokenStream, io::Error> {
        let keys = cow_data.keys().into_iter().map(|key| key.clone());
        let vals = cow_data
            .keys()
            .into_iter()
            .map(|key| cow_data.get(key).unwrap().clone());
        let len = keys.len();

        let test = quote::quote! {
            const COW_DATA: [(&'static str, &'static str); #len] = [
                #( (#keys, #vals) ),*
            ];
        };

        Ok(test)
    }
}

/************************************************/
/**************Resource Functions****************/
/************************************************/
fn get_source_archive(
    path: &str,
    resource_name: &str,
    internal_path: &str,
    exclude: &Option<Vec<&str>>,
) -> Result<(), std::io::Error> {
    // Check Initial directoy Structure
    get_env_flag!(force_download, "FORCE_DOWNLOAD");
    let downloads_path = String::from("target/downloads");
    let archive_path = format!("{downloads_path}/{resource_name}.zip");
    let resource_root = String::from("target/resources");
    let resource_destination = format!("{resource_root}/{resource_name}");
    check_dir_exists!(downloads_path.as_str());
    check_dir_exists!(resource_root.as_str());
    check_dir_exists!("target/tmp", clear_existing);
    check_dir_exists!(&resource_destination, clear_existing);

    //Short circuit if we aren't force-redownloading and the resource exists
    match fs::metadata(&archive_path) {
        Ok(_) if force_download.is_some() => remove_file(&archive_path)?,
        Ok(_) => return Ok(()), //Short Circuit
        Err(_) => (),
    };

    let os = std::env::consts::FAMILY;
    let mut proc = match os {
        "windows" => {
            let mut p = std::process::Command::new("powershell.exe");
            p.args(&[
                "-NoLogo",
                "-NoProfile",
                "-Command",
                format!("Invoke-RestMethod {path} -OutFile {archive_path}").as_str(),
            ]);
            p
        }
        "unix" => {
            let mut p = std::process::Command::new("curl");
            p.args(&["-L", path, "--output", &archive_path]);
            p
        }
        _ => panic!(
            "Are you being special and building this on a non-standard operating system. \
        Good for you. But I can't figure out what command to use for downloading files. \
        Consider modifying the get_external_resource function in build.rs since you are similar enough to an Arch Linux user :p"
        ),
    };
    proc.spawn()?.wait()?;

    // Extract the Archive
    let archive_file = match File::open(&archive_path) {
        Ok(file) => BufReader::new(file),
        Err(_) => panic!("Could not do this"),
    };
    let mut zip_archive = zip::ZipArchive::new(archive_file)?;
    zip_archive.extract("target/tmp")?;

    let tmp_path = format!("target/tmp/{internal_path}");
    let copy_opts = fs_extra::dir::CopyOptions::new().overwrite(true);
    let resource_list: Vec<PathBuf> = fs::read_dir(tmp_path)?
        .filter(|file| {
            if let Some(exclude_list) = exclude {
                !exclude_list.contains(
                    &file
                        .as_ref()
                        .expect("Could not get metadata for some of the resources")
                        .file_name()
                        .clone()
                        .to_str()
                        .unwrap(),
                )
            } else {
                true
            }
        })
        .map(|file| {
            file.expect("Could not get metadata for some of the resources")
                .path()
        })
        .collect();

    //Copy Extracted files
    let _ = fs_extra::copy_items(resource_list.as_slice(), resource_destination, &copy_opts)
        .expect("Could not copy resources as expected!");

    Ok(())
}
