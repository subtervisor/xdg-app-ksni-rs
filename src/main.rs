use std::collections::{BTreeMap, HashMap};

use log::{error, info, trace, warn};
use log_err::*;
use notify::{watcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;
use std::time::Duration;
use tokio;
use zbus::{dbus_interface, SignalContext};

mod constants;
mod desktop;
mod proxy_types;
mod util;

struct AppMenuStatusNotifierItem {}

#[dbus_interface(name = "org.kde.StatusNotifierItem")]
impl AppMenuStatusNotifierItem {
  /// Activate method
  async fn activate(&self, _x: i32, _y: i32) {}

  /// ContextMenu method
  async fn context_menu(&self, _x: i32, _y: i32) {}

  /// Scroll method
  async fn scroll(&self, _delta: i32, _orientation: &str) {}

  /// SecondaryActivate method
  async fn secondary_activate(&self, _x: i32, _y: i32) {}

  /// NewAttentionIcon signal
  #[dbus_interface(signal)]
  async fn new_attention_icon(ctxt: &SignalContext<'_>) -> zbus::Result<()>;

  /// NewIcon signal
  #[dbus_interface(signal)]
  async fn new_icon(ctxt: &SignalContext<'_>) -> zbus::Result<()>;

  /// NewMenu signal
  #[dbus_interface(signal)]
  async fn new_menu(ctxt: &SignalContext<'_>) -> zbus::Result<()>;

  /// NewOverlayIcon signal
  #[dbus_interface(signal)]
  async fn new_overlay_icon(ctxt: &SignalContext<'_>) -> zbus::Result<()>;

  /// NewStatus signal
  #[dbus_interface(signal)]
  async fn new_status(ctxt: &SignalContext<'_>, status: &str) -> zbus::Result<()>;

  /// NewTitle signal
  #[dbus_interface(signal)]
  async fn new_title(ctxt: &SignalContext<'_>) -> zbus::Result<()>;

  /// NewToolTip signal
  #[dbus_interface(signal)]
  async fn new_tool_tip(ctxt: &SignalContext<'_>) -> zbus::Result<()>;

  /// AttentionIconName property
  #[dbus_interface(property)]
  async fn attention_icon_name(&self) -> &str {
    ""
  }

  /// AttentionIconPixmap property
  #[dbus_interface(property)]
  async fn attention_icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
    vec![]
  }

  /// AttentionMovieName property
  #[dbus_interface(property)]
  async fn attention_movie_name(&self) -> &str {
    ""
  }

  /// Category property
  #[dbus_interface(property)]
  async fn category(&self) -> &str {
    "ApplicationStatus"
  }

  /// IconName property
  #[dbus_interface(property)]
  async fn icon_name(&self) -> &str {
    "starred"
  }

  /// IconPixmap property
  #[dbus_interface(property)]
  async fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
    vec![]
  }

  /// IconThemePath property
  #[dbus_interface(property)]
  async fn icon_theme_path(&self) -> &str {
    ""
  }

  /// Id property
  #[dbus_interface(property)]
  async fn id(&self) -> &str {
    "WSLAppMenu"
  }

  /// ItemIsMenu property
  #[dbus_interface(property)]
  async fn item_is_menu(&self) -> bool {
    true
  }

  /// Menu property
  #[dbus_interface(property)]
  async fn menu(&self) -> zbus::zvariant::OwnedObjectPath {
    zbus::zvariant::OwnedObjectPath::try_from(
      "/org/ayatana/NotificationItem/wslAppMenuDbusMenu/Menu",
    )
    .log_expect("Failed to parse menu path")
  }

  /// OverlayIconName property
  #[dbus_interface(property)]
  async fn overlay_icon_name(&self) -> &str {
    ""
  }

  /// OverlayIconPixmap property
  #[dbus_interface(property)]
  async fn overlay_icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
    vec![]
  }

  /// Status property
  #[dbus_interface(property)]
  async fn status(&self) -> &str {
    "Active"
  }

  /// Title property
  #[dbus_interface(property)]
  async fn title(&self) -> &str {
    "Apps"
  }

  /*
    /// ToolTip property
    #[dbus_interface(property)]
    async fn tool_tip(&self) -> ZbusResult<(String, Vec<(i32, i32, Vec<u8>)>)>;

    /// WindowId property
    #[dbus_interface(property)]
    async fn window_id(&self) -> ZbusResult<i32>;
  */
}

pub type DbusMenuLayoutEntry = (
  i32,
  std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
  Vec<zbus::zvariant::OwnedValue>,
);

fn get_layout(
  root: i32,
  children: &HashMap<i32, Vec<i32>>,
  props: &HashMap<i32, desktop::MenuProps>,
  property_names: &Vec<&str>,
  recursion_depth: i32,
) -> DbusMenuLayoutEntry {
  let root_props = props
    .get(&root)
    .log_expect("Failed to get props in layout fetch");

  let ctxt = zbus::zvariant::EncodingContext::<byteorder::LE>::new_dbus(0);
  let encoded =
    zbus::zvariant::to_bytes(ctxt, root_props).log_expect("Failed to encode properties");
  let mut root_props: std::collections::HashMap<String, zbus::zvariant::OwnedValue> =
    zbus::zvariant::from_slice(&encoded, ctxt).log_expect("Failed to decode properties");
  let root_props = root_props
    .drain()
    .filter(|(k, _)| property_names.is_empty() || property_names.contains(&k.as_str()))
    .collect();
  let mut entry = (root, root_props, Vec::new());
  let next_depth = if recursion_depth > 0 {
    recursion_depth - 1
  } else {
    recursion_depth
  };
  if next_depth != 0 {
    if let Some(node_children) = children.get(&root) {
      for child in node_children.iter() {
        if let Some(_) = props.get(child) {
          let child = get_layout(*child, children, &props, property_names, next_depth);
          let variant = zbus::zvariant::OwnedValue::from(zbus::zvariant::Value::new(child));
          entry.2.push(variant);
        }
      }
    }
  }
  entry
}

fn update_category_props(
  children: &mut HashMap<i32, Vec<i32>>,
  props: &mut HashMap<i32, desktop::MenuProps>,
) {
  for i in 1..12 {
    props
      .get_mut(&i)
      .log_expect("Failed to get category ref for update")
      .visible = !children
      .get(&i)
      .log_expect("Failed to get children ref for update")
      .is_empty();

    children
      .get_mut(&i)
      .log_expect("Failed to get children for sorting")
      .sort_by_key(|k| {
        props
          .get(k)
          .log_expect("Failed to get properties for sorting")
          .label
          .clone()
      })
  }
}

fn launcher_updated(orig: &desktop::Launcher, new: &desktop::Launcher) -> bool {
  orig.categories.iter().next() != new.categories.iter().next()
    || orig.display != new.display
    || orig.icon != new.icon
    || orig.name != new.name
}

#[derive(Debug)]
struct AppMenuDbusMenu {
  revision: u32,
  children: HashMap<i32, Vec<i32>>,
  props: HashMap<i32, desktop::MenuProps>,
  cache: HashMap<std::ffi::OsString, BTreeMap<usize, desktop::Launcher>>,
  path_map: bimap::BiMap<usize, std::path::PathBuf>,
  counter: LauncherCounter,
}

use zbus::DBusError;
#[derive(DBusError, Debug)]
#[dbus_error(prefix = "org.wsl.AppMenuDbusMenu")]
enum MenuError {
  #[dbus_error(zbus_error)]
  ZBus(zbus::Error),
  LauncherIndexNotFound,
  PropertyNotFound,
}

#[dbus_interface(name = "com.canonical.dbusmenu")]
impl AppMenuDbusMenu {
  /// AboutToShow method
  async fn about_to_show(&self, _id: i32) -> bool {
    false
  }

  /// AboutToShowGroup method
  async fn about_to_show_group(&self, _ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
    (vec![], vec![])
  }

  /// Event method
  async fn event(
    &mut self,
    item_id: i32,
    event_id: &str,
    _data: zbus::zvariant::Value<'_>,
    timestamp: u32,
    #[zbus(signal_context)] ctxt: SignalContext<'_>,
  ) {
    match event_id {
      "clicked" => {
        let sig_res = AppMenuDbusMenu::item_activation_requested(&ctxt, &item_id, &timestamp).await;
        if let Err(err) = sig_res {
          warn!("Failed to signal activation for {}: {}", item_id, err);
        }
        let target_path = self.counter.get_path(&(item_id as usize));
        if target_path.is_some() {
          let target_path = target_path.unwrap();
          let target_entry = self
            .cache
            .get(target_path)
            .log_expect(format!("Failed to find BTree for {:?}", target_path).as_str())
            .iter()
            .next()
            .log_expect(format!("Failed to get BTree entry for {:?}", target_path).as_str());
          let exec = &target_entry.1.exec;
          let mut exec_vec = exec.split(" ").collect::<std::collections::VecDeque<_>>();
          if exec_vec.is_empty() {
            warn!("Exec for {:?} is empty!", target_path);
          } else {
            let mut cmd = std::process::Command::new(exec_vec.pop_front().unwrap());
            let spawn_result = cmd.args(exec_vec).spawn();
            if let Err(err) = spawn_result {
              error!("Failed to exec {:?}: {}", target_path, err);
            }
            return;
          }
        }
        warn!("Got activation request for nonexistent entry: {}", item_id);
      },
      "hovered" => trace!("Ignoring hover"),
      "opened" => trace!("Ignoring open"),
      "closed" => trace!("Ignoring close"),
      _ => info!("Ignoring unknown event: {}", event_id),
    }
  }

  /// GetGroupProperties method
  async fn get_group_properties(
    &self,
    item_ids: Vec<i32>,
    property_names: Vec<&str>,
  ) -> Vec<(
    i32,
    std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
  )> {
    let mut out = Vec::new();
    for i in item_ids.iter() {
      let props = self.props.get(i);
      if let Some(props) = props {
        let ctxt = zbus::zvariant::EncodingContext::<byteorder::LE>::new_dbus(0);
        let encoded =
          zbus::zvariant::to_bytes(ctxt, props).log_expect("Failed to encode properties");
        let mut props: std::collections::HashMap<String, zbus::zvariant::OwnedValue> =
          zbus::zvariant::from_slice(&encoded, ctxt).log_expect("Failed to decode properties");
        let props = props
          .drain()
          .filter(|(k, _)| property_names.is_empty() || property_names.contains(&k.as_str()))
          .collect();
        out.push((*i, props));
      }
    }
    out
  }

  /// GetLayout method
  async fn get_layout(
    &self,
    parent_id: i32,
    recursion_depth: i32,
    property_names: Vec<&str>,
  ) -> Result<(u32, DbusMenuLayoutEntry), MenuError> {
    if let Some(_) = self.props.get(&parent_id) {
      let layout = get_layout(
        parent_id,
        &self.children,
        &self.props,
        &property_names,
        recursion_depth,
      );
      return Ok((self.revision, layout));
    }
    Err(MenuError::LauncherIndexNotFound)
  }

  /// GetProperty method
  async fn get_property(
    &self,
    item_id: i32,
    name: &str,
  ) -> Result<zbus::zvariant::OwnedValue, MenuError> {
    if let Some(item_props) = self.props.get(&item_id) {
      match name {
        "type" => Ok(zbus::zvariant::OwnedValue::from(
          zbus::zvariant::Value::new("standard"),
        )),
        "label" => Ok(zbus::zvariant::OwnedValue::from(
          zbus::zvariant::Value::new(&item_props.label),
        )),
        "enabled" => Ok(zbus::zvariant::OwnedValue::from(
          zbus::zvariant::Value::new(item_props.enabled),
        )),
        "visible" => Ok(zbus::zvariant::OwnedValue::from(
          zbus::zvariant::Value::new(item_props.visible),
        )),
        "icon-name" => Ok(zbus::zvariant::OwnedValue::from(
          zbus::zvariant::Value::new(&item_props.icon_name),
        )),
        "icon-data" => Ok(zbus::zvariant::OwnedValue::from(
          zbus::zvariant::Value::new(&item_props.icon_data),
        )),
        "shortcut" => Ok(zbus::zvariant::OwnedValue::from(
          zbus::zvariant::Value::new(Vec::<String>::new()),
        )),
        "toggle-type" => Ok(zbus::zvariant::OwnedValue::from(
          zbus::zvariant::Value::new(String::new()),
        )),
        "toggle-state" => Ok(zbus::zvariant::OwnedValue::from(
          zbus::zvariant::Value::new(-1i32),
        )),
        "children-display" => Ok(zbus::zvariant::OwnedValue::from(
          zbus::zvariant::Value::new(&item_props.children_display),
        )),
        _ => Err(MenuError::PropertyNotFound),
      }
    } else {
      Err(MenuError::LauncherIndexNotFound)
    }
  }

  /// AddLauncherPath method
  async fn add_launcher_path(
    &mut self,
    path: &str,
    #[zbus(signal_context)] ctxt: SignalContext<'_>,
  ) {
    let locale = sys_locale::get_locale().unwrap_or_else(|| String::from("en-US"));
    let p = std::path::PathBuf::from(path);
    if p.is_file() {
      if let Some(launcher) = desktop::launcher_for_entry(p.clone(), &locale) {
        let cache_name = p.file_stem().unwrap_or_default().to_os_string();
        let menu_idx = self.counter.get_index(&cache_name);
        let prio_cache = self.cache.entry(cache_name).or_default();

        let source = p.parent();
        if source.is_none() {
          warn!("Launcher has no parent: {}", path);
          return;
        }
        let source = source.unwrap();

        let prio_idx = self.path_map.get_by_right(&source.to_path_buf());
        if prio_idx.is_none() {
          warn!("Failed to find priority map entry for {:?}", source);
          return;
        }
        let prio_idx = prio_idx.unwrap();

        let existing_launcher = prio_cache.iter().next();

        if existing_launcher.is_none()
          || (existing_launcher.clone().unwrap().0 >= prio_idx
            && launcher_updated(&launcher, existing_launcher.clone().unwrap().1))
        {
          if let Some(existing_launcher) = existing_launcher {
            let c = existing_launcher
              .1
              .categories
              .iter()
              .next()
              .unwrap_or(&constants::Category::Uncategorized);
            self
              .children
              .get_mut(&(constants::category_idx(*c) as i32))
              .log_expect("Failed to get category reference")
              .retain(|i| *i != menu_idx as i32);
          }
          let entry_props = desktop::launcher_props(&launcher);
          let enc_ctxt = zbus::zvariant::EncodingContext::<byteorder::LE>::new_dbus(0);
          let encoded = zbus::zvariant::to_bytes(enc_ctxt, &entry_props)
            .log_expect("Failed to encode properties");
          let mut props: std::collections::HashMap<String, zbus::zvariant::OwnedValue> =
            zbus::zvariant::from_slice(&encoded, enc_ctxt)
              .log_expect("Failed to decode properties");
          let props = props.drain().map(|(k, v)| (k, v.into())).collect();
          self.props.insert(menu_idx as i32, entry_props);

          let c = launcher
            .categories
            .iter()
            .next()
            .unwrap_or(&constants::Category::Uncategorized);
          self
            .children
            .get_mut(&(constants::category_idx(*c) as i32))
            .log_expect("Failed to get category reference")
            .push(menu_idx as i32);

          self.revision = self.revision + 1;

          update_category_props(&mut self.children, &mut self.props);

          let sig_res = AppMenuDbusMenu::items_properties_updated(
            &ctxt,
            &vec![(menu_idx as i32, props)],
            &vec![],
          )
          .await;
          if let Err(err) = sig_res {
            warn!("Failed to signal property updates for {}: {}", path, err);
          }

          let sig_res = AppMenuDbusMenu::layout_updated(&ctxt, &self.revision, &0).await;
          if let Err(err) = sig_res {
            warn!("Failed to signal layout updates for {}: {}", path, err);
          }
        }

        prio_cache.insert(*prio_idx, launcher);
      } else {
        warn!("Failed to parse {} as launcher", path);
      }
    }
  }

  /// AddLauncherPath method
  async fn remove_launcher_path(
    &mut self,
    path: &str,
    #[zbus(signal_context)] ctxt: SignalContext<'_>,
  ) {
    let p = std::path::PathBuf::from(path);
    let cache_name = p.file_stem().unwrap_or_default().to_os_string();
    let menu_idx = self.counter.get_index(&cache_name);
    let prio_cache = self.cache.entry(cache_name.clone()).or_default();

    let source = p.parent();
    if source.is_none() {
      warn!("Launcher has no parent: {}", path);
      return;
    }
    let source = source.unwrap();

    let prio_idx = self.path_map.get_by_right(&source.to_path_buf());
    if prio_idx.is_none() {
      warn!("Failed to find priority map entry for {:?}", source);
      return;
    }
    let prio_idx = prio_idx.unwrap();

    let entry = prio_cache.get(prio_idx);
    if entry.is_none() {
      info!("Entry not found: {}", path);
      return;
    }
    let entry = entry.unwrap().clone();
    prio_cache.remove(prio_idx);
    if prio_cache.is_empty() {
      prio_cache.insert(
        *prio_idx,
        desktop::tombstone_launcher(p, (*cache_name.to_string_lossy()).to_string()),
      );
    }

    let c = entry
      .categories
      .iter()
      .next()
      .unwrap_or(&constants::Category::Uncategorized);
    self
      .children
      .get_mut(&(constants::category_idx(*c) as i32))
      .log_expect("Failed to get category reference")
      .retain(|i| *i != menu_idx as i32);

    let r_entry = prio_cache.iter().next().unwrap();
    let remain = desktop::launcher_props(r_entry.1);
    if r_entry.0 <= prio_idx && launcher_updated(r_entry.1, &entry) {
      let enc_ctxt = zbus::zvariant::EncodingContext::<byteorder::LE>::new_dbus(0);
      let encoded =
        zbus::zvariant::to_bytes(enc_ctxt, &remain).log_expect("Failed to encode properties");
      let mut props: std::collections::HashMap<String, zbus::zvariant::OwnedValue> =
        zbus::zvariant::from_slice(&encoded, enc_ctxt).log_expect("Failed to decode properties");
      let props = props.drain().map(|(k, v)| (k, v.into())).collect();
      self.props.insert(menu_idx as i32, remain);

      let c = r_entry
        .1
        .categories
        .iter()
        .next()
        .unwrap_or(&constants::Category::Uncategorized);
      self
        .children
        .get_mut(&(constants::category_idx(*c) as i32))
        .log_expect("Failed to get category reference")
        .push(menu_idx as i32);

      self.revision = self.revision + 1;

      update_category_props(&mut self.children, &mut self.props);

      let sig_res =
        AppMenuDbusMenu::items_properties_updated(&ctxt, &vec![(menu_idx as i32, props)], &vec![])
          .await;
      if let Err(err) = sig_res {
        warn!("Failed to signal property updates for {}: {}", path, err);
      }

      let sig_res = AppMenuDbusMenu::layout_updated(&ctxt, &self.revision, &0).await;
      if let Err(err) = sig_res {
        warn!("Failed to signal layout updates for {}: {}", path, err);
      }
    }
  }

  /// ItemActivationRequested signal
  #[dbus_interface(signal)]
  async fn item_activation_requested(
    ctxt: &SignalContext<'_>,
    iid: &i32,
    timestamp: &u32,
  ) -> zbus::Result<()>;

  /// ItemsPropertiesUpdated signal
  #[dbus_interface(signal)]
  async fn items_properties_updated(
    ctxt: &SignalContext<'_>,
    updated: &Vec<(
      i32,
      std::collections::HashMap<String, zbus::zvariant::Value<'_>>,
    )>,
    removed: &Vec<(i32, Vec<String>)>,
  ) -> zbus::Result<()>;

  /// LayoutUpdated signal
  #[dbus_interface(signal)]
  async fn layout_updated(
    ctxt: &SignalContext<'_>,
    revision: &u32,
    parent: &i32,
  ) -> zbus::Result<()>;

  /// IconThemePath property
  #[dbus_interface(property)]
  async fn icon_theme_path(&self) -> Vec<String> {
    vec![]
  }

  /// Status property
  #[dbus_interface(property)]
  async fn status(&self) -> &str {
    "normal"
  }

  /// TextDirection property
  #[dbus_interface(property)]
  async fn text_direction(&self) -> &str {
    "ltr"
  }

  /// Version property
  #[dbus_interface(property)]
  async fn version(&self) -> u32 {
    3
  }
}

#[derive(Debug)]
struct LauncherCounter {
  count: usize,
  map: bimap::BiMap<std::ffi::OsString, usize>,
}

impl LauncherCounter {
  fn get_index(&mut self, key: &std::ffi::OsString) -> usize {
    let res = self.map.get_by_left(key);
    if res.is_some() {
      *res.unwrap()
    } else {
      self.map.insert(key.clone(), self.count);
      let ret = self.count;
      self.count = self.count + 1;
      ret
    }
  }

  fn get_path(&mut self, index: &usize) -> Option<&std::ffi::OsString> {
    self.map.get_by_right(index)
  }
}
#[tokio::main]
async fn main() {
  util::init::init_logging();

  let locale = sys_locale::get_locale().unwrap_or_else(|| String::from("en-US"));

  let app_dirs = util::init::get_app_dirs()
    .drain(..)
    .enumerate()
    .collect::<bimap::BiMap<usize, std::path::PathBuf>>();
  let mut launcher_counter = LauncherCounter {
    count: 12,
    map: bimap::BiMap::new(),
  };

  let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
  let mut props: HashMap<i32, desktop::MenuProps> = HashMap::new();
  children.insert(0, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
  props.insert(0, desktop::root_props());
  enum_iterator::all::<constants::Category>().for_each(|c| {
    props.insert(
      constants::category_idx(c) as i32,
      desktop::category_props(c),
    );
  });
  for i in 1..12 {
    children.insert(i, Vec::new());
  }
  let mut cache: HashMap<std::ffi::OsString, BTreeMap<usize, desktop::Launcher>> = HashMap::new();

  for dir in app_dirs.iter() {
    match dir.1.read_dir() {
      Ok(entries) => {
        for e in entries {
          match e {
            Ok(entry) => {
              let p = entry.path();
              if let Some(launcher) = desktop::launcher_for_entry(p.clone(), &locale) {
                let name = p.file_stem().unwrap_or_default().to_os_string();
                let prio_cache = cache.entry(name).or_default();
                prio_cache.insert(*dir.0, launcher);
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

  for entry in cache.iter() {
    let active_entry = entry
      .1
      .iter()
      .next()
      .log_expect(format!("Failed to get initial entry for {:?}", entry.0).as_str());
    let entry_props = desktop::launcher_props(active_entry.1);
    let idx = launcher_counter.get_index(entry.0);
    props.insert(idx as i32, entry_props);
    if active_entry.1.categories.is_empty() {
      children
        .get_mut(&(constants::category_idx(constants::Category::Uncategorized) as i32))
        .log_expect("Failed to get category reference")
        .push(idx as i32);
    } else {
      let c = active_entry
        .1
        .categories
        .iter()
        .next()
        .log_expect("Failed to get first category");
      children
        .get_mut(&(constants::category_idx(*c) as i32))
        .log_expect("Failed to get category reference")
        .push(idx as i32);
    }
  }

  info!("Loaded {} menu entries", cache.len());

  update_category_props(&mut children, &mut props);

  let (tx, rx) = channel();

  // Create a watcher object, delivering debounced events.
  // The notification back-end is selected based on the platform.
  let mut watcher = watcher(tx, Duration::from_secs(10)).unwrap();

  // Add a path to be watched. All files and directories at that path and
  // below will be monitored for changes.
  for dir in app_dirs.iter() {
    watcher
      .watch(dir.1, RecursiveMode::Recursive)
      .log_expect(format!("Failed to watch {:?}", dir.1).as_str());
  }

  let menu_struct = AppMenuDbusMenu {
    revision: 0,
    children,
    props,
    cache,
    path_map: app_dirs,
    counter: launcher_counter,
  };

  let dbus = zbus::ConnectionBuilder::session()
    .log_expect("Failed to connect to DBUS session")
    .name("org.wsl.AppMenuDbusMenu");
  let connection = dbus
    .log_expect("Failed to claim DBUS name")
    .serve_at(
      "/org/ayatana/NotificationItem/wslAppMenuDbusMenu/Menu",
      menu_struct,
    )
    .log_expect("Failed to set up DBUS menu")
    .serve_at(
      "/org/ayatana/NotificationItem/wslAppMenuDbusMenu",
      AppMenuStatusNotifierItem {},
    )
    .log_expect("Failed to set up icon")
    .build()
    .await
    .log_expect("Failed to launch DBUS menu service");

  let watcher_ref = proxy_types::StatusNotifierWatcherProxy::new(&connection)
    .await
    .log_expect("Failed to get watcher reference");

  watcher_ref
    .register_status_notifier_item("/org/ayatana/NotificationItem/wslAppMenuDbusMenu")
    .await
    .log_expect("Failed to register with watcher");

  let object_server = connection.object_server();
  let iface_ref = object_server
    .interface::<_, AppMenuDbusMenu>("/org/ayatana/NotificationItem/wslAppMenuDbusMenu/Menu")
    .await
    .log_expect("Failed to get reference to menu interface");

  loop {
    let evt = rx.recv();
    use notify::DebouncedEvent::*;
    let mut iface = iface_ref.get_mut().await;
    match evt {
      Ok(event) => match event {
        Create(path) => {
          info!("New launcher at {:?}", path);
          iface
            .add_launcher_path(&path.to_string_lossy(), iface_ref.signal_context().clone())
            .await;
        },
        Write(path) => {
          info!("Updated launcher at {:?}", path);
          iface
            .add_launcher_path(&path.to_string_lossy(), iface_ref.signal_context().clone())
            .await;
        },
        NoticeRemove(path) => {
          info!("Removed launcher at {:?}", path);
          iface
            .remove_launcher_path(&path.to_string_lossy(), iface_ref.signal_context().clone())
            .await;
        },
        _ => {},
      },
      Err(e) => println!("Watcher error: {:?}", e),
    }
  }
}
