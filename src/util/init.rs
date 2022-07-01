use log::{error, warn, LevelFilter};
use log_err::*;

fn env_or(name: &str, default: &str) -> String {
  let var = std::env::var(name).unwrap_or_default();
  if var.is_empty() {
    default.to_string()
  } else {
    var
  }
}

pub fn init_logging() {
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
    for line in format!("Backtrace:\n{:?}", backtrace::Backtrace::new()).lines() {
      error!("{}", line);
    }
    std::process::exit(1);
  }));
}

pub fn get_app_dirs() -> Vec<std::path::PathBuf> {
  let xdg_dirs = xdg::BaseDirectories::new().log_expect("Failed to init XDG directories");
  let mut dirs: std::collections::VecDeque<_> = xdg_dirs
    .get_data_dirs()
    .drain(..)
    .map(|p| p.join("applications"))
    .collect();
  if let Ok(home_data) = xdg_dirs.create_data_directory("applications") {
    dirs.push_front(home_data);
  }
  return dirs.drain(..).filter(|p| p.is_dir()).collect();
}

pub fn get_only_show() -> String {
  env_or("ONLY_SHOW", "GNOME")
}
