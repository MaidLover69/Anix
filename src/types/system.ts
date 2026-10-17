/** `offline-status` event and `get_offline_status`: AniList data is served from the saved copy. */
export interface OfflineStatus {
  offline: boolean;
  /** Unix seconds: when the oldest saved data on screen was saved. */
  saved_at: number | null;
}

/** `get_desktop_status`: tray and autostart, for Settings › System. */
export interface DesktopStatus {
  tray_supported: boolean;
  tray_host: boolean;
  tray_active: boolean;
  autostart_installed: boolean;
  autostart_path: string | null;
  exec_target: string | null;
}

export interface VaInfo {
  vaapi_version: string | null;
  driver: string | null;
  decode: string[];
  profiles: string[];
}

/** `get_hardware_report`. */
export interface HardwareReport {
  session: 'wayland' | 'x11' | 'unknown';
  wayland_display: string | null;
  x11_display: string | null;
  xdg_session_type: string | null;
  desktop: string | null;
  webkitgtk_version: string | null;
  dmabuf_disabled: boolean;
  dmabuf_value: string | null;
  dmabuf_set_by_app: boolean;
  gpu_vendor: string | null;
  render_node: string | null;
  libva_driver_name: string | null;
  running_from_appimage: boolean;
  vaapi: {
    vainfo_installed: boolean;
    ok: boolean;
    info: VaInfo | null;
    error: string | null;
  };
  gstreamer: {
    dirs: string[];
    va: string | null;
    vaapi: string | null;
    libav: string | null;
    bundled: boolean;
  };
  tips: string[];
}

export type PluginHealthStatus = 'ok' | 'empty' | 'timeout' | 'verification' | 'error';

/** `test_plugin` / `test_all_plugins`. */
export interface PluginHealth {
  id: string;
  status: PluginHealthStatus;
  latency_ms: number;
  results: number;
  error: string | null;
}

/** `check_plugin_updates`. */
export interface PluginUpdate {
  id: string;
  installed: string;
  available: string;
}

/** One entry of `get_plugin_directory`. */
export interface DirectoryList {
  name: string;
  description: string;
  url: string;
  languages: string[];
}
