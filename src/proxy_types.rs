#![allow(non_snake_case)]
use zbus::dbus_proxy;

#[dbus_proxy(
  interface = "org.kde.StatusNotifierWatcher",
  default_path = "/StatusNotifierWatcher"
)]
pub trait StatusNotifierWatcher {
  /// RegisterStatusNotifierHost method
  fn register_status_notifier_host(&self, service: &str) -> zbus::Result<()>;

  /// RegisterStatusNotifierItem method
  fn register_status_notifier_item(&self, service: &str) -> zbus::Result<()>;

  /// StatusNotifierHostRegistered signal
  #[dbus_proxy(signal)]
  fn status_notifier_host_registered(&self) -> zbus::Result<()>;

  /// StatusNotifierHostUnregistered signal
  #[dbus_proxy(signal)]
  fn status_notifier_host_unregistered(&self) -> zbus::Result<()>;

  /// StatusNotifierItemRegistered signal
  #[dbus_proxy(signal)]
  fn status_notifier_item_registered(&self, item_id: &str) -> zbus::Result<()>;

  /// StatusNotifierItemUnregistered signal
  #[dbus_proxy(signal)]
  fn status_notifier_item_unregistered(&self, service: &str) -> zbus::Result<()>;

  /// IsStatusNotifierHostRegistered property
  #[dbus_proxy(property)]
  fn is_status_notifier_host_registered(&self) -> zbus::Result<bool>;

  /// ProtocolVersion property
  #[dbus_proxy(property)]
  fn protocol_version(&self) -> zbus::Result<i32>;

  /// RegisteredStatusNotifierItems property
  #[dbus_proxy(property)]
  fn registered_status_notifier_items(&self) -> zbus::Result<Vec<String>>;
}
