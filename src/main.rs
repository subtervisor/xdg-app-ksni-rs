use std::collections::{BTreeMap, HashMap, VecDeque};
use std::env;
use std::path::PathBuf;

use backtrace::Backtrace;
use freedesktop_desktop_entry::DesktopEntry;
use log::{error, info, warn, LevelFilter};
use log_err::*;
//use notify::{watcher, RecursiveMode, Watcher};

fn env_or(name: &str, default: &str) -> String {
  let var = env::var(name).unwrap_or_default();
  if var.is_empty() {
    default.to_string()
  } else {
    var
  }
}

fn init_logging() {
  if systemd_journal_logger::connected_to_journal() {
    systemd_journal_logger::init().unwrap();
  } else {
    simple_logger::SimpleLogger::new().init().unwrap();
  }
  let log_level = env_or("LOG_LEVEL", "INFO").to_uppercase();
  match log_level.as_str() {
    "ERROR" => log::set_max_level(LevelFilter::Error),
    "DEBUG" => log::set_max_level(LevelFilter::Debug),
    "TRACE" => log::set_max_level(LevelFilter::Trace),
    "INFO" => log::set_max_level(LevelFilter::Info),
    _ => {
      log::set_max_level(LevelFilter::Info);
      warn!(
        "Unknown log level '{}' passed in, defaulting to info",
        log_level
      );
    },
  }
  let stock_hook = std::panic::take_hook();
  std::panic::set_hook(Box::new(move |info| {
    stock_hook(info);
    if let Some(s) = info.payload().downcast_ref::<&str>() {
      error!("Exiting due to panic in thread {}: {}", thread_id::get(), s);
    } else {
      error!("Exiting due to panic in thread {}", thread_id::get());
    }
    if let Some(location) = info.location() {
      error!(
        "Panic location: {}:{}:{}",
        location.file(),
        location.line(),
        location.column()
      );
    } else {
      error!("Panic location: Unknown");
    }
    for line in format!("Backtrace:\n{:?}", Backtrace::new()).lines() {
      error!("{}", line);
    }
    std::process::exit(1);
  }));
}

fn get_app_dirs() -> Vec<PathBuf> {
  let xdg_dirs = xdg::BaseDirectories::new().log_expect("Failed to init XDG directories");
  let mut dirs: VecDeque<_> = xdg_dirs
    .get_data_dirs()
    .drain(..)
    .map(|p| p.join("applications"))
    .collect();
  if let Ok(home_data) = xdg_dirs.create_data_directory("applications") {
    dirs.push_front(home_data);
  }
  return dirs.drain(..).filter(|p| p.is_dir()).collect();
}

fn xdg_unescape(input: &str, multi: bool) -> String {
  let mut out = String::new();
  let mut control = false;
  for c in input.chars() {
    if control {
      match c {
        's' => out.push(' '),
        't' => out.push('\t'),
        'r' => out.push('\r'),
        'n' => out.push('\n'),
        '\\' => out.push('\\'),
        ';' => {
          if multi {
            out.push(';');
          }
        },
        _ => {
          out.push('\\');
          out.push(c);
        },
      }
      control = false;
    } else {
      if c == '\\' {
        control = true;
      } else {
        out.push(c);
      }
    }
  }
  if control {
    out.push('\\');
  }
  out
}

fn xdg_split(input: &str) -> Vec<String> {
  let mut out = Vec::new();
  let mut current = String::new();
  for c in input.chars() {
    if c == ';' {
      let last = current.pop();
      if last == Some('\\') {
        current.push(c);
      } else {
        if let Some(c) = last {
          current.push(c);
        }
        out.push(xdg_unescape(&current, true));
        current = String::new();
      }
    } else {
      current.push(c);
    }
  }
  out
}

#[derive(Debug)]
#[allow(dead_code)]
struct Launcher {
  path: PathBuf,
  name: String,
  categories: Vec<String>,
  exec: String,
  icon: Option<String>,
  display: bool,
  only_show_in: Vec<String>,
  terminal: bool,
}

fn main() {
  init_logging();

  let locale = sys_locale::get_locale().unwrap_or_else(|| String::from("en-US"));

  let app_dirs = get_app_dirs()
    .drain(..)
    .enumerate()
    .collect::<bimap::BiMap<_, _>>();

  let mut cache: HashMap<std::ffi::OsString, BTreeMap<usize, Launcher>> = HashMap::new();

  for dir in app_dirs.iter() {
    match dir.1.read_dir() {
      Ok(entries) => {
        for e in entries {
          match e {
            Ok(entry) => {
              let p = entry.path();
              let ext = p.extension().unwrap_or_default().to_str();
              let name = p.file_stem().unwrap_or_default().to_os_string();
              if p.is_file() && ext == Some("desktop") && !name.is_empty() {
                match std::fs::read_to_string(&p) {
                  Ok(data) => match DesktopEntry::decode(&p, &data) {
                    Ok(desk) => {
                      let entry_name = desk
                        .name(Some(&locale))
                        .or(desk.generic_name(Some(&locale)))
                        .or(Some(std::borrow::Cow::from(desk.appid)))
                        .unwrap();
                      let entry_type = desk.type_().unwrap_or("Application");
                      let entry_exec = desk.exec();
                      if entry_exec.is_none() {
                        info!("{} ({:?}) lacks exec key", entry_name, &p);
                        continue;
                      }
                      if entry_type == "Application" {
                        let prio_cache = cache.entry(name).or_default();
                        prio_cache.insert(
                          *dir.0,
                          Launcher {
                            name: xdg_unescape(&entry_name, false),
                            categories: xdg_split(desk.categories().unwrap_or("")),
                            exec: xdg_unescape(entry_exec.unwrap(), false),
                            icon: desk.icon().map(|s| xdg_unescape(s, false)),
                            display: !desk.no_display(),
                            only_show_in: xdg_split(desk.only_show_in().unwrap_or("")),
                            terminal: desk.terminal(),
                            path: p,
                          },
                        );
                      }
                    },
                    Err(e) => warn!("Failed to parse {:?}: {}", p, e),
                  },
                  Err(e) => warn!("Failed to read desktop entry {:?}: {}", p, e),
                }
              }
            },
            Err(e) => {
              warn!("Failed while reading {:?}: {}", dir, e);
            },
          }
        }
      },
      Err(e) => {
        warn!("Failed to read {:?}: {}", dir, e);
      },
    }
  }
  info!("Result: {:#?}", cache);
}
