use std::path::PathBuf;

use freedesktop_desktop_entry::DesktopEntry;
use log::{error, info, warn};

use crate::constants;
use crate::util;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Launcher {
  pub path: PathBuf,
  pub name: String,
  pub categories: Vec<constants::Category>,
  pub exec: String,
  pub icon: Option<String>,
  pub display: bool,
}

pub fn tombstone_launcher(path: PathBuf, name: String) -> Launcher {
  Launcher {
    path,
    name,
    categories: vec![],
    exec: String::new(),
    icon: None,
    display: false,
  }
}

fn category_str_convert(vec: Vec<String>) -> Vec<constants::Category> {
  vec
    .iter()
    .filter_map(|s| constants::CATEGORY_MAP.get(&s).cloned())
    .collect()
}

use zbus::zvariant::{DeserializeDict, OwnedValue, SerializeDict, Type, Value};
#[derive(SerializeDict, DeserializeDict, Type, Debug, Clone, Value, OwnedValue)]
#[zvariant(signature = "a{sv}")]
pub struct MenuProps {
  pub label: String,
  pub visible: bool,
  pub enabled: bool,
  #[zvariant(rename = "icon-name")]
  pub icon_name: String,
  #[zvariant(rename = "icon-data")]
  pub icon_data: Vec<u8>,
  #[zvariant(rename = "type")]
  pub entry_type: String,
  #[zvariant(rename = "children-display")]
  pub children_display: String,
}

pub fn launcher_props(launcher: &Launcher) -> MenuProps {
  let mut props = MenuProps {
    label: launcher.name.clone(),
    visible: launcher.display,
    icon_name: String::new(),
    entry_type: "standard".to_string(),
    children_display: String::new(),
    icon_data: vec![],
    enabled: true,
  };

  if launcher.icon.is_some() {
    let icon_ref = launcher.icon.as_ref().unwrap();
    if icon_ref.contains("/") {
      let icon_path = std::path::Path::new(icon_ref);
      if icon_path.exists() && icon_path.is_file() && icon_path.extension().is_some() {
        let ext = icon_path.extension().unwrap();
        if ext == "svg" {
          let mut svg_opts = usvg::Options::default();
          svg_opts.resources_dir = std::fs::canonicalize(icon_path)
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));
          svg_opts.fontdb.load_system_fonts();
          let svg_data = std::fs::read(icon_path).unwrap();
          let rtree = usvg::Tree::from_data(&svg_data, &svg_opts.to_ref());
          if rtree.is_err() {
            let err = rtree.err();
            error!("Failed to parse SVG {:?}: {:?}", icon_path, err);
          } else {
            let rtree = rtree.unwrap();
            let pixmap_size = rtree.svg_node().size.to_screen_size();
            let pixmap = tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height());
            if pixmap.is_none() {
              error!("Failed to make skia bitmap");
            } else {
              let mut pixmap = pixmap.unwrap();
              let render = resvg::render(
                &rtree,
                usvg::FitTo::Original,
                tiny_skia::Transform::default(),
                pixmap.as_mut(),
              );
              if render.is_none() {
                error!("Failed to render SVG");
              } else {
                let png_data = pixmap.encode_png();
                if png_data.is_err() {
                  let err = png_data.err().unwrap();
                  error!("Failed to convert {:?} to PNG: {:?}", icon_path, err);
                } else {
                  let png_data = png_data.unwrap();
                  props.icon_data = png_data;
                }
              }
            }
          }
        } else {
          use image::io::Reader as ImageReader;
          use std::io::Cursor;
          let data = ImageReader::open(icon_path);
          if data.is_err() {
            let err = data.err().unwrap();
            error!("Failed to read image at {:?}: {}", icon_path, err);
          } else {
            let data = data.unwrap().decode();
            if data.is_err() {
              let err = data.err().unwrap();
              error!("Failed to parse image at {:?}: {}", icon_path, err);
            } else {
              let data = data.unwrap();
              let mut png_bytes: Vec<u8> = Vec::new();
              let decode_res = data.write_to(
                &mut Cursor::new(&mut png_bytes),
                image::ImageOutputFormat::Png,
              );
              if decode_res.is_err() {
                let err = decode_res.err().unwrap();
                error!("Failed to convert image at {:?}: {}", icon_path, err);
              } else {
                props.icon_data = png_bytes;
              }
            }
          }
        }
      } else {
        warn!("Icon at {:?} not found", icon_path);
      }
    } else {
      props.icon_name = icon_ref.clone();
    }
  }
  props
}

pub fn category_props(c: constants::Category) -> MenuProps {
  MenuProps {
    label: constants::category_string(c).to_string(),
    visible: true,
    icon_name: String::new(),
    entry_type: "standard".to_string(),
    children_display: "submenu".to_string(),
    icon_data: vec![],
    enabled: true,
  }
}

pub fn root_props() -> MenuProps {
  MenuProps {
    label: String::new(),
    visible: true,
    icon_name: String::new(),
    entry_type: "standard".to_string(),
    children_display: "submenu".to_string(),
    icon_data: vec![],
    enabled: true,
  }
}

pub fn launcher_for_entry(p: PathBuf, locale: &str) -> Option<Launcher> {
  let ext = p.extension().unwrap_or_default().to_str();
  let name = p.file_stem().unwrap_or_default();
  if p.is_file() && ext == Some("desktop") && !name.is_empty() {
    match std::fs::read_to_string(&p) {
      Ok(data) => match DesktopEntry::decode(&p, &data) {
        Ok(desk) => {
          let entry_name = desk
            .name(Some(&locale))
            .or(desk.generic_name(Some(&locale)))
            .or(Some(std::borrow::Cow::from(desk.appid)))
            .unwrap();
          info!("Entry: {} ({})", entry_name, desk.no_display());
          let entry_type = desk.type_().unwrap_or("Application");
          let entry_exec = desk.exec();
          if entry_exec.is_none() {
            info!("{} ({:?}) lacks exec key", entry_name, &p);
            return None;
          }
          if entry_type == "Application" {
            let only_show_in = util::xdg::split(desk.only_show_in().unwrap_or(""));
            let icon = desk.icon().map(|s| util::xdg::unescape(s, false));
            let name = util::xdg::unescape(&entry_name, false);
            return Some(Launcher {
              categories: category_str_convert(util::xdg::split(desk.categories().unwrap_or(""))),
              exec: util::xdg::exec_substitute(
                &util::xdg::unescape(entry_exec.unwrap(), false),
                icon.clone(),
                &name,
                &p,
              ),
              name: name,
              icon: icon,
              display: !desk.no_display()
                && !desk.terminal()
                && (only_show_in.is_empty() || only_show_in.contains(&util::init::get_only_show())),
              path: p,
            });
          }
        },
        Err(e) => warn!("Failed to parse {:?}: {}", p, e),
      },
      Err(e) => warn!("Failed to read desktop entry {:?}: {}", p, e),
    }
  }
  None
}
